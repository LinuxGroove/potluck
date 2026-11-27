//! Worlds shared from home, peer to peer. The host's game listens with an
//! iroh endpoint (QUIC dialled by an Ed25519 key, through home routers by
//! hole punching, with a relay over HTTPS as the fallback), and a friend's
//! game dials it by the key the market gave for the world's code. The
//! frames are the ones WebSocket carries (`net::frame_msg`), each with its
//! length in front, on one stream, and what arrives comes out as the same
//! `Wire` events, so the world never knows which way a player came. The
//! market only introduces the two games: after that nothing goes through
//! LinuxGroove but what the relay carries for the few who can't go direct.

use std::net::SocketAddr;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use iroh::endpoint::{presets, Connection, RecvStream, SendStream};
use iroh::{Endpoint, EndpointAddr, EndpointId, RelayMap, RelayMode, RelayUrl, SecretKey, TransportAddr};

use crate::transport::{Link, Wire};

/// What the two games speak on the connection: length-prefixed frames.
pub const ALPN: &[u8] = b"potluck/1";
/// The biggest frame either side takes: a whole world fits.
const MAX_FRAME: usize = 64 << 20;
/// How long a friend's game tries to reach a host.
const DIAL_FOR: Duration = Duration::from_secs(25);
/// How often a connection's path is looked at, for Direct or Through LinuxGroove.
const LOOK_EVERY: Duration = Duration::from_secs(2);

/// The runtime iroh's tasks run on: two threads, made the first time a
/// world is shared or joined.
pub(crate) fn rt() -> &'static tokio::runtime::Runtime {
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RT.get_or_init(|| tokio::runtime::Builder::new_multi_thread().worker_threads(2).thread_name("potluck-p2p").enable_all().build().expect("a runtime for sharing worlds"))
}

/// The relay: where both games wait to be introduced, and what carries
/// the connections that can't go direct.
#[derive(Clone, Debug)]
pub struct Relay {
    map: RelayMap,
    url: RelayUrl,
    /// A test relay, with a certificate of its own making.
    #[cfg_attr(not(test), allow(dead_code))]
    insecure: bool,
}

/// The relays tests run, by address (tests run side by side).
#[cfg(test)]
static TEST_RELAYS: Mutex<Vec<Relay>> = Mutex::new(Vec::new());

impl Relay {
    /// The relay at the https address the market gives.
    pub fn parse(url: &str) -> Option<Relay> {
        let url = url.trim();
        if url.is_empty() {
            return None;
        }
        #[cfg(test)]
        if let Some(r) = TEST_RELAYS.lock().unwrap().iter().find(|r| r.url() == url.trim_end_matches('/')) {
            return Some(r.clone());
        }
        let url: RelayUrl = url.parse().ok()?;
        Some(Relay { map: RelayMap::from(url.clone()), url, insecure: false })
    }

    pub fn url(&self) -> String {
        self.url.to_string().trim_end_matches('/').to_string()
    }

    /// A relay the tests run, known by its address from now on.
    #[cfg(test)]
    pub fn testing(map: RelayMap, url: RelayUrl) -> Relay {
        let r = Relay { map, url, insecure: true };
        TEST_RELAYS.lock().unwrap().push(r.clone());
        r
    }
}

/// A new key for a share or a join: each is made afresh, so nothing ties
/// one evening's worlds to the next.
pub fn new_key() -> SecretKey {
    SecretKey::generate()
}

/// A key as the market and passes carry it: 64 hex digits.
pub fn key_text(k: &SecretKey) -> String {
    k.public().to_string()
}

async fn bind(secret: SecretKey, relay: Option<&Relay>, through_relay: bool, alpns: Vec<Vec<u8>>) -> Result<Endpoint, String> {
    // no n0 address lookup: the market gives each side the other's key and addresses
    let mut b = Endpoint::builder(presets::Minimal).secret_key(secret).alpns(alpns);
    b = match relay {
        Some(r) => b.relay_mode(RelayMode::Custom(r.map.clone())),
        None => b.relay_mode(RelayMode::Disabled),
    };
    #[cfg(test)]
    if relay.is_some_and(|r| r.insecure) {
        b = b.ca_tls_config(iroh::tls::CaTlsConfig::insecure_skip_verify());
    }
    // Always connect through LinuxGroove: no addresses of ours go to anyone
    if through_relay && relay.is_some() {
        b = b.clear_ip_transports();
    }
    b.bind().await.map_err(|e| format!("couldn't start sharing ({e})"))
}

/// Whether a connection's traffic goes through the relay now.
fn relayed(conn: &Connection) -> bool {
    let paths = conn.paths();
    paths.iter().find(|p| p.is_selected()).is_none_or(|p| p.is_relay())
}

/// Tells the world's code which way a connection goes, now and whenever it
/// changes, until it closes.
async fn watch_path(conn: Connection, id: u32, peer: String, tx: Sender<Wire>) {
    let mut was = None;
    loop {
        let now = relayed(&conn);
        if was != Some(now) {
            was = Some(now);
            if tx.send(Wire::Link(id, Link { peer: peer.clone(), relayed: now })).is_err() {
                return;
            }
        }
        tokio::select! {
            _ = conn.closed() => return,
            _ = tokio::time::sleep(LOOK_EVERY) => {}
        }
    }
}

/// Reads frames until the stream ends.
async fn read_frames(mut recv: RecvStream, id: u32, tx: Sender<Wire>) {
    let mut len = [0u8; 4];
    loop {
        if recv.read_exact(&mut len).await.is_err() {
            return;
        }
        let n = u32::from_le_bytes(len) as usize;
        if n > MAX_FRAME {
            return;
        }
        let mut buf = vec![0u8; n];
        if recv.read_exact(&mut buf).await.is_err() {
            return;
        }
        if tx.send(Wire::Data(id, buf)).is_err() {
            return;
        }
    }
}

/// Writes what the game sends until it stops sending (its side of the
/// channel is dropped) or the stream breaks.
async fn write_frames(mut send: SendStream, out: Receiver<Vec<u8>>) {
    // the game's side is a std channel: a blocking thread hands it over
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
    tokio::task::spawn_blocking(move || {
        while let Ok(b) = out.recv() {
            if tx.send(b).is_err() {
                return;
            }
        }
    });
    while let Some(b) = rx.recv().await {
        if b.len() > MAX_FRAME {
            continue;
        }
        if send.write_all(&(b.len() as u32).to_le_bytes()).await.is_err() || send.write_all(&b).await.is_err() {
            return;
        }
    }
    let _ = send.finish();
}

/// A world being shared: the endpoint friends dial. Dropping it stops.
pub struct Sharing {
    ep: Endpoint,
    key: String,
}

impl Sharing {
    /// Opens the world to friends who dial `secret`'s key, through `relay`.
    /// Each friend comes out on `tx` as a player, numbered from `next` like
    /// the WebSocket ones.
    pub fn start(secret: SecretKey, relay: Option<Relay>, through_relay: bool, tx: Sender<Wire>, next: Arc<Mutex<u32>>) -> Result<Sharing, String> {
        let key = key_text(&secret);
        let ep = rt().block_on(bind(secret, relay.as_ref(), through_relay, vec![ALPN.to_vec()]))?;
        let accepting = ep.clone();
        rt().spawn(async move {
            while let Some(incoming) = accepting.accept().await {
                let (tx, next) = (tx.clone(), next.clone());
                tokio::spawn(async move {
                    let Ok(conn) = incoming.await else { return };
                    if conn.alpn() != ALPN {
                        conn.close(1u8.into(), b"not potluck");
                        return;
                    }
                    let peer = conn.remote_id().to_string();
                    // a friend's game opens one stream and says hello on it
                    let Ok(Ok((send, recv))) = tokio::time::timeout(Duration::from_secs(20), conn.accept_bi()).await else {
                        conn.close(2u8.into(), b"no stream");
                        return;
                    };
                    let id = {
                        let mut n = next.lock().unwrap();
                        *n += 1;
                        *n
                    };
                    let (out_tx, out_rx) = std::sync::mpsc::channel();
                    if tx.send(Wire::Joined(id, out_tx)).is_err() {
                        return;
                    }
                    let _ = tx.send(Wire::Link(id, Link { peer: peer.clone(), relayed: relayed(&conn) }));
                    tokio::spawn(watch_path(conn.clone(), id, peer, tx.clone()));
                    let writer = tokio::spawn(write_frames(send, out_rx));
                    read_frames(recv, id, tx.clone()).await;
                    let _ = tx.send(Wire::Gone(id));
                    writer.abort();
                    conn.close(0u8.into(), b"bye");
                });
            }
        });
        Ok(Sharing { ep, key })
    }

    /// The key friends dial, as hex.
    pub fn key(&self) -> &str {
        &self.key
    }

    /// Where on its own networks the host can be reached, for friends on
    /// the same one (the market passes them on with the key).
    pub fn addrs(&self) -> Vec<String> {
        self.ep.addr().ip_addrs().map(|a| a.to_string()).collect()
    }
}

impl Drop for Sharing {
    fn drop(&mut self) {
        let ep = self.ep.clone();
        rt().spawn(async move { ep.close().await });
    }
}

/// Dials a shared world: the host's key, the addresses the market had for
/// it, and the relay. Returns at once with the channel to send frames on;
/// they wait until the connection's up. What comes back arrives on `tx`
/// as player 0, as from `transport::connect`, with a `Wire::Link` once
/// it's up and a `Wire::Gone` when it ends or never comes up.
pub fn dial(secret: SecretKey, relay: Option<Relay>, through_relay: bool, host: &str, addrs: &[String], tx: Sender<Wire>) -> Result<Sender<Vec<u8>>, String> {
    let host: EndpointId = host.parse().map_err(|_| "that world's key isn't one".to_string())?;
    let mut to: Vec<TransportAddr> = vec![];
    if let Some(r) = &relay {
        to.push(TransportAddr::Relay(r.url.clone()));
    }
    if !through_relay {
        to.extend(addrs.iter().filter_map(|a| a.parse::<SocketAddr>().ok()).map(TransportAddr::Ip));
    }
    if to.is_empty() {
        return Err("there's no way to reach that world from here".into());
    }
    let addr = EndpointAddr::from_parts(host, to);
    let (out_tx, out_rx) = std::sync::mpsc::channel::<Vec<u8>>();
    rt().spawn(async move {
        let ep = match bind(secret, relay.as_ref(), through_relay, vec![]).await {
            Ok(ep) => ep,
            Err(_) => {
                let _ = tx.send(Wire::Gone(0));
                return;
            }
        };
        let conn = match tokio::time::timeout(DIAL_FOR, ep.connect(addr, ALPN)).await {
            Ok(Ok(c)) => c,
            _ => {
                let _ = tx.send(Wire::Gone(0));
                ep.close().await;
                return;
            }
        };
        let Ok((send, recv)) = conn.open_bi().await else {
            let _ = tx.send(Wire::Gone(0));
            ep.close().await;
            return;
        };
        let peer = conn.remote_id().to_string();
        tokio::spawn(watch_path(conn.clone(), 0, peer, tx.clone()));
        let mut writer = tokio::spawn(write_frames(send, out_rx));
        let mut reader = tokio::spawn(read_frames(recv, 0, tx.clone()));
        // either side ending ends it: the game leaving drops its sender
        tokio::select! {
            _ = &mut writer => {}
            _ = &mut reader => {}
        }
        writer.abort();
        reader.abort();
        let _ = tx.send(Wire::Gone(0));
        conn.close(0u8.into(), b"bye");
        ep.close().await;
    });
    Ok(out_tx)
}
