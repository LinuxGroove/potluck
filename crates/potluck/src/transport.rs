//! Connections between games and servers: WebSocket messages over TCP, or
//! over TLS when a server has a certificate, so a server can sit at an
//! https address (`wss://potluck.example.com`) or behind any web proxy.
//! The same port answers plain web requests with the server's status.

use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::path::Path;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tungstenite::{Message, WebSocket};

/// What a connection thread tells the game.
pub enum Wire {
    Joined(u32, Sender<Vec<u8>>),
    Data(u32, Vec<u8>),
    Gone(u32),
}

/// A server side stream: plain TCP or TLS.
pub enum Stream {
    Plain(TcpStream),
    Tls(Box<rustls::StreamOwned<rustls::ServerConnection, TcpStream>>),
}

impl Stream {
    fn tcp(&self) -> &TcpStream {
        match self {
            Stream::Plain(s) => s,
            Stream::Tls(s) => &s.sock,
        }
    }
}

impl Read for Stream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Stream::Plain(s) => s.read(buf),
            Stream::Tls(s) => s.read(buf),
        }
    }
}

impl Write for Stream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Stream::Plain(s) => s.write(buf),
            Stream::Tls(s) => s.write(buf),
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        match self {
            Stream::Plain(s) => s.flush(),
            Stream::Tls(s) => s.flush(),
        }
    }
}

/// A stream with some bytes already read off the front, given back first.
pub struct Prefixed<S> {
    head: Vec<u8>,
    at: usize,
    inner: S,
}

impl<S: Read> Read for Prefixed<S> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.at < self.head.len() {
            let n = (self.head.len() - self.at).min(buf.len());
            buf[..n].copy_from_slice(&self.head[self.at..self.at + n]);
            self.at += n;
            return Ok(n);
        }
        self.inner.read(buf)
    }
}

impl<S: Write> Write for Prefixed<S> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// Sends and receives on one WebSocket until it closes. Reads time out
/// every few milliseconds so queued messages go out promptly.
fn pump<S: Read + Write>(mut ws: WebSocket<S>, id: u32, tx: Sender<Wire>, out: Receiver<Vec<u8>>) {
    'outer: loop {
        let mut wrote = false;
        for f in out.try_iter() {
            if ws.write(Message::Binary(f.into())).is_err() {
                break 'outer;
            }
            wrote = true;
        }
        if wrote && ws.flush().is_err() {
            break;
        }
        match ws.read() {
            Ok(Message::Binary(b)) => {
                if tx.send(Wire::Data(id, b.to_vec())).is_err() {
                    break;
                }
            }
            Ok(Message::Close(_)) => break,
            Ok(_) => {}
            Err(tungstenite::Error::Io(e)) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => {
                // nothing to read yet; make sure a half-written frame goes out
                let _ = ws.flush();
            }
            Err(_) => break,
        }
    }
    let _ = ws.close(None);
    let _ = tx.send(Wire::Gone(id));
}

/// Reads up to the end of an HTTP request's head.
fn read_head<S: Read>(s: &mut S) -> io::Result<Vec<u8>> {
    let mut head = Vec::new();
    let mut b = [0u8; 1024];
    while !head.windows(4).any(|w| w == b"\r\n\r\n") {
        let n = s.read(&mut b)?;
        if n == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        head.extend_from_slice(&b[..n]);
        if head.len() > 16 * 1024 {
            return Err(io::ErrorKind::InvalidData.into());
        }
    }
    Ok(head)
}

/// Answers one connection: a WebSocket joins the world; anything else gets
/// the server's status page.
fn serve(mut s: Stream, id: u32, tx: Sender<Wire>, status: Arc<Mutex<String>>) {
    let _ = s.tcp().set_read_timeout(Some(Duration::from_secs(8)));
    let Ok(head) = read_head(&mut s) else { return };
    let text = String::from_utf8_lossy(&head).to_lowercase();
    if !text.contains("upgrade: websocket") {
        let json = text.starts_with("get /status.json");
        let st = status.lock().unwrap().clone();
        let body = if json { st } else { status_page(&st) };
        let kind = if json { "application/json" } else { "text/html; charset=utf-8" };
        let reply = format!("HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n{body}", body.len());
        let _ = s.write_all(reply.as_bytes());
        let _ = s.flush();
        return;
    }
    let _ = s.tcp().set_nodelay(true);
    let stream = Prefixed { head, at: 0, inner: s };
    let Ok(ws) = tungstenite::accept(stream) else { return };
    let _ = ws.get_ref().inner.tcp().set_read_timeout(Some(Duration::from_millis(8)));
    let (otx, orx) = channel();
    if tx.send(Wire::Joined(id, otx)).is_err() {
        return;
    }
    pump(ws, id, tx, orx);
}

fn status_page(json: &str) -> String {
    let v: serde_json::Value = serde_json::from_str(json).unwrap_or_default();
    let name = v.get("name").and_then(|n| n.as_str()).unwrap_or("Potluck server");
    let players: Vec<String> = v.get("players").and_then(|p| p.as_array()).map(|a| a.iter().filter_map(|p| p.as_str().map(html)).collect()).unwrap_or_default();
    let creations: Vec<String> = v.get("creations").and_then(|p| p.as_array()).map(|a| a.iter().filter_map(|p| p.as_str().map(html)).collect()).unwrap_or_default();
    format!(
        "<!doctype html><meta charset=utf-8><meta name=viewport content='width=device-width'><title>{n}</title>\
<style>body{{font:16px system-ui,sans-serif;background:#12161c;color:#f5f0e6;max-width:40em;margin:3em auto;padding:0 1em}}h1{{color:#ffc35c}}code{{background:#ffffff14;padding:.1em .4em;border-radius:6px}}</style>\
<h1>{n}</h1><p>A Potluck world. In Potluck, choose <b>Join a friend's world</b> and type this page's address.</p>\
<p><b>{np} playing</b>: {pl}</p><p><b>Creations</b>: {cr}</p><p>Version <code>{ver}</code></p>",
        n = html(name),
        np = players.len(),
        pl = if players.is_empty() { "nobody yet".into() } else { players.join(", ") },
        cr = creations.join(", "),
        ver = html(v.get("version").and_then(|n| n.as_str()).unwrap_or("")),
    )
}

fn html(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

pub struct TlsFiles<'a> {
    pub cert: &'a Path,
    pub key: &'a Path,
}

pub fn tls_config(files: &TlsFiles) -> Result<Arc<rustls::ServerConfig>, String> {
    use rustls::pki_types::pem::PemObject;
    use rustls::pki_types::{CertificateDer, PrivateKeyDer};
    let certs: Vec<CertificateDer> = CertificateDer::pem_file_iter(files.cert)
        .map_err(|e| format!("can't read {}: {e}", files.cert.display()))?
        .collect::<Result<_, _>>()
        .map_err(|e| format!("can't read {}: {e}", files.cert.display()))?;
    let key = PrivateKeyDer::from_pem_file(files.key).map_err(|e| format!("can't read {}: {e}", files.key.display()))?;
    let cfg = rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .map_err(|e| e.to_string())?
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| format!("the certificate and key don't work together: {e}"))?;
    Ok(Arc::new(cfg))
}

/// Accepts players on a port, with TLS if a config is given. Connection
/// ids come from `next`, shared between listeners.
pub fn listen(port: u16, tls: Option<Arc<rustls::ServerConfig>>, tx: Sender<Wire>, status: Arc<Mutex<String>>, next: Arc<Mutex<u32>>) -> Result<(), String> {
    let listener = TcpListener::bind(("0.0.0.0", port)).map_err(|e| format!("port {port} is busy ({e})"))?;
    std::thread::spawn(move || {
        for tcp in listener.incoming().flatten() {
            let id = {
                let mut n = next.lock().unwrap();
                *n += 1;
                *n
            };
            let tx = tx.clone();
            let status = status.clone();
            let tls = tls.clone();
            std::thread::spawn(move || {
                let s = match tls {
                    Some(cfg) => match rustls::ServerConnection::new(cfg) {
                        Ok(conn) => Stream::Tls(Box::new(rustls::StreamOwned::new(conn, tcp))),
                        Err(_) => return,
                    },
                    None => Stream::Plain(tcp),
                };
                serve(s, id, tx, status);
            });
        }
    });
    Ok(())
}

/// Turns what a player typed into a WebSocket address: `192.168.1.20`,
/// `host:port`, `ws://...`, `wss://...` or an `https://` page.
pub fn url_for(addr: &str, default_port: u16) -> String {
    let a = addr.trim();
    if let Some(rest) = a.strip_prefix("https://") {
        return format!("wss://{}", rest.trim_end_matches('/'));
    }
    if let Some(rest) = a.strip_prefix("http://") {
        return format!("ws://{}", rest.trim_end_matches('/'));
    }
    if a.starts_with("ws://") || a.starts_with("wss://") {
        return a.to_string();
    }
    let host_part = a.split('/').next().unwrap_or(a);
    if host_part.contains(':') { format!("ws://{a}") } else { format!("ws://{a}:{default_port}") }
}

/// Connects to a world. The connection runs on its own thread.
pub fn connect(addr: &str, default_port: u16, tx: Sender<Wire>) -> Result<Sender<Vec<u8>>, String> {
    let url = url_for(addr, default_port);
    let secure = url.starts_with("wss://");
    let hostport = url.split("://").nth(1).unwrap_or("").split('/').next().unwrap_or("").to_string();
    let hostport = if hostport.contains(':') { hostport } else { format!("{hostport}:{}", if secure { 443 } else { default_port }) };
    let sa = hostport.to_socket_addrs().map_err(|e| e.to_string())?.next().ok_or("no such address")?;
    let tcp = TcpStream::connect_timeout(&sa, Duration::from_secs(5)).map_err(|e| e.to_string())?;
    let _ = tcp.set_nodelay(true);
    let _ = tcp.set_read_timeout(Some(Duration::from_secs(8)));
    let (ws, _) = tungstenite::client_tls_with_config(url.as_str(), tcp, None, None).map_err(|e| match e {
        tungstenite::HandshakeError::Failure(e) => e.to_string(),
        _ => "the world didn't answer".into(),
    })?;
    match ws.get_ref() {
        tungstenite::stream::MaybeTlsStream::Plain(s) => {
            let _ = s.set_read_timeout(Some(Duration::from_millis(8)));
        }
        tungstenite::stream::MaybeTlsStream::Rustls(s) => {
            let _ = s.sock.set_read_timeout(Some(Duration::from_millis(8)));
        }
        _ => {}
    }
    let (otx, orx) = channel();
    std::thread::spawn(move || pump(ws, 1, tx, orx));
    Ok(otx)
}
