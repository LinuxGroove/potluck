//! Tests that need no window: the world, creations running in the sandbox,
//! and connections between games and servers, plain and over TLS.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex, Once};
use std::time::{Duration, Instant};

use bevy::prelude::*;

use crate::creations::{Creation, Library, Manifest, Origin};
use crate::creatures::{self, Herd};
use crate::noise::Rng;
use crate::script::{KnobValue, PlayerInfo, Scripts};
use crate::transport::{self, Wire};
use crate::world::{self, Voxels};

fn home() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let dir = std::env::temp_dir().join(format!("potluck-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        // set once, before any test reads it
        unsafe { std::env::set_var("POTLUCK_HOME", dir) };
    });
}

fn mossback() -> Creation {
    home();
    Library::load().list.remove("mossback").expect("the base game has the mossback")
}

fn world() -> Voxels {
    static WORLD: Mutex<Option<Vec<u8>>> = Mutex::new(None);
    let mut w = WORLD.lock().unwrap();
    let bytes = w.get_or_insert_with(|| world::generate(crate::SEED).encode()).clone();
    Voxels::decode(crate::SEED, &bytes).unwrap()
}

fn run(scripts: &mut Scripts, voxels: &mut Voxels, herd: &mut Herd, secs: f32) {
    let mut rng = Rng::new(1);
    for _ in 0..(secs * 20.0) as usize {
        creatures::step(herd, voxels, 0.05, &mut rng);
        scripts.tick(voxels, herd, 0.05);
    }
}

#[test]
fn the_world_is_the_same_every_time_and_survives_saving() {
    let a = world::generate(crate::SEED).encode();
    let b = world::generate(crate::SEED).encode();
    assert_eq!(a, b, "the same seed makes the same world");
    let mut w = Voxels::decode(crate::SEED, &a).unwrap();
    let top = w.top(world::GREEN.x as i32, world::GREEN.z as i32);
    assert!(top > world::SEA, "the Green is above the sea");
    w.set(IVec3::new(10, 100, 10), crate::blocks::LANTERN);
    let back = Voxels::decode(crate::SEED, &w.encode()).unwrap();
    assert_eq!(back.get(10, 100, 10), crate::blocks::LANTERN);
    assert!(back.lanterns.contains(&IVec3::new(10, 100, 10)), "lanterns are found again on loading");
    assert!(Voxels::decode(crate::SEED, &a[..a.len() / 2]).is_none(), "a damaged world is refused");
}

#[test]
fn mossbacks_spawn_wander_and_buck_their_riders() {
    let mut voxels = world();
    let mut herd = Herd::default();
    let mut scripts = Scripts::default();
    scripts.start(&mossback(), &mut herd).unwrap();
    run(&mut scripts, &mut voxels, &mut herd, 2.0);
    assert_eq!(herd.bodies.len(), 3, "count = 3 keeps three about");
    let home = world::paddock();
    assert!(herd.bodies.values().all(|b| b.pos.distance(home) < 12.0), "they start in the paddock");
    let start: Vec<Vec3> = herd.bodies.values().map(|b| b.pos).collect();
    run(&mut scripts, &mut voxels, &mut herd, 10.0);
    let moved = herd.bodies.values().zip(&start).any(|(b, s)| b.pos.distance(*s) > 0.5);
    assert!(moved, "they wander");

    let id = *herd.bodies.keys().next().unwrap();
    let me = PlayerInfo { id: 7, name: "Ken".into(), pos: herd.bodies[&id].pos };
    scripts.players = vec![me.clone()];
    herd.bodies.get_mut(&id).unwrap().rider = Some(7);
    scripts.event(&mut voxels, &mut herd, id, "ridden", &me);
    assert_eq!(herd.bodies[&id].says.as_ref().map(|s| s.0.as_str()), Some("Mmmf?"));
    let mut thrown = false;
    for _ in 0..120 {
        run(&mut scripts, &mut voxels, &mut herd, 1.0);
        if let Some((pid, v)) = scripts.out.throws.first() {
            assert_eq!(*pid, 7);
            assert!(v.y > 0.0, "a buck throws you up");
            thrown = true;
            break;
        }
    }
    assert!(thrown, "sooner or later a mossback bucks");
}

#[test]
fn knobs_and_hot_reload_change_creatures_but_keep_them() {
    let mut voxels = world();
    let mut herd = Herd::default();
    let mut scripts = Scripts::default();
    let mut c = mossback();
    scripts.start(&c, &mut herd).unwrap();
    run(&mut scripts, &mut voxels, &mut herd, 1.0);
    let ids: Vec<u64> = herd.bodies.keys().copied().collect();
    let knobs = &scripts.vms["mossback"].knobs;
    assert!(knobs.iter().any(|k| k.key == "buck" && k.value == KnobValue::Number(6.0)));

    // Tweak: a new shell colour
    c.knobs.insert("shell".into(), KnobValue::Color("#c03030".into()));
    scripts.start(&c, &mut herd).unwrap();
    let shell = herd.bodies[&ids[0]].parts.iter().find(|p| p.name == "shell").unwrap().color;
    assert!(shell[0] > 0.5 && shell[1] < 0.3, "the shell takes the knob's colour: {shell:?}");

    // an edit to the code: pink moss
    let code = c.files["main.luau"].replace("color = \"#6fa64a\"", "color = \"#ff7ab8\"");
    c.files.insert("main.luau".into(), code);
    scripts.start(&c, &mut herd).unwrap();
    let mut now: Vec<u64> = herd.bodies.keys().copied().collect();
    now.sort();
    let mut before = ids.clone();
    before.sort();
    assert_eq!(now, before, "the same creatures carry on after a reload");
    let moss = herd.bodies[&ids[0]].parts.iter().find(|p| p.name == "moss").unwrap().color;
    assert!(moss[0] > 0.9 && moss[1] < 0.6, "the moss is pink now: {moss:?}");
}

fn creation(code: &str) -> Creation {
    let mut files: BTreeMap<String, String> = BTreeMap::new();
    files.insert("potluck.toml".into(), "name = \"Test\"\nby = \"Tests\"\nkind = \"creature\"\nmain = \"main.luau\"\n".into());
    files.insert("main.luau".into(), code.into());
    let manifest: Manifest = toml::from_str(&files["potluck.toml"]).unwrap();
    Creation { id: "test".into(), manifest, files, dir: None, origin: Origin::Mine, stamp: 0, knobs: BTreeMap::new(), code: None }
}

const BOX: &str = r##"
local world = require("@potluck/world")
local Thing = world.creature {
    name = "Thing", count = 1, home = "paddock",
    body = { { "box", size = { 1, 1, 1 }, at = { 0, 0.5, 0 }, color = "#ffffff" } },
}
"##;

#[test]
fn a_creation_that_never_stops_is_stopped() {
    let mut voxels = world();
    let mut herd = Herd::default();
    let mut scripts = Scripts::default();
    scripts.start(&creation(&format!("{BOX}\nfunction Thing:tick(dt) while true do end end")), &mut herd).unwrap();
    let t = Instant::now();
    run(&mut scripts, &mut voxels, &mut herd, 0.5);
    assert!(t.elapsed() < Duration::from_secs(5), "the interrupt ends a loop that never stops");
    let vm = &scripts.vms["test"];
    assert!(vm.error.as_deref().unwrap_or("").contains("too long"), "the creator is told: {:?}", vm.error);
}

#[test]
fn a_creation_cannot_reach_outside_the_sandbox() {
    let mut voxels = world();
    let mut herd = Herd::default();
    let mut scripts = Scripts::default();
    let code = format!(
        "{BOX}\nfunction Thing:spawned()\n  local ok = pcall(function() return io.open('/etc/passwd') end)\n  local ex = os.execute ~= nil or os.exit ~= nil or loadstring ~= nil or getfenv ~= nil or setfenv ~= nil\n  self:say((ok and 'io' or '') .. (ex and 'ex' or '') .. 'sealed')\nend"
    );
    scripts.start(&creation(&code), &mut herd).unwrap();
    run(&mut scripts, &mut voxels, &mut herd, 0.5);
    let said: Vec<String> = herd.bodies.values().filter_map(|b| b.says.as_ref().map(|s| s.0.clone())).collect();
    assert_eq!(said, vec!["sealed".to_string()], "no files, no programs, no loading code");
}

#[test]
fn addresses_players_type_become_websocket_urls() {
    assert_eq!(transport::url_for("192.168.1.20", 47126), "ws://192.168.1.20:47126");
    assert_eq!(transport::url_for("192.168.1.20:5000", 47126), "ws://192.168.1.20:5000");
    assert_eq!(transport::url_for("https://potluck.example.org/", 47126), "wss://potluck.example.org");
    assert_eq!(transport::url_for("http://box.lan:47126", 47126), "ws://box.lan:47126");
    assert_eq!(transport::url_for(" wss://p.example.org:8443 ", 47126), "wss://p.example.org:8443");
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

fn wait<T>(rx: &Receiver<T>, what: &str) -> T {
    rx.recv_timeout(Duration::from_secs(10)).unwrap_or_else(|_| panic!("timed out waiting for {what}"))
}

fn status() -> Arc<Mutex<String>> {
    Arc::new(Mutex::new(r#"{"name":"Test <world>","players":["Ken"],"creations":["Mossback"],"version":"0.1"}"#.into()))
}

/// A server and a game pass messages both ways.
fn talk(server: &Receiver<Wire>, client: &Receiver<Wire>, to_server: &std::sync::mpsc::Sender<Vec<u8>>) {
    let Wire::Joined(id, to_client) = wait(server, "the join") else { panic!("expected a join first") };
    to_server.send(b"hello from the game".to_vec()).unwrap();
    match wait(server, "the game's message") {
        Wire::Data(from, b) => {
            assert_eq!(from, id);
            assert_eq!(b, b"hello from the game");
        }
        _ => panic!("expected data"),
    }
    to_client.send(vec![7; 300_000]).unwrap();
    match wait(client, "the world's message") {
        Wire::Data(_, b) => assert_eq!(b.len(), 300_000, "a whole world's worth arrives in one piece"),
        _ => panic!("expected data"),
    }
}

#[test]
fn games_join_a_server_and_browsers_see_its_status() {
    let port = free_port();
    let (stx, srx) = channel();
    transport::listen(port, None, stx, status(), Arc::new(Mutex::new(0))).unwrap();
    let (ctx, crx) = channel();
    let to_server = transport::connect(&format!("127.0.0.1:{port}"), port, ctx).unwrap();
    talk(&srx, &crx, &to_server);

    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.write_all(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
    let mut page = String::new();
    s.read_to_string(&mut page).unwrap();
    assert!(page.starts_with("HTTP/1.1 200"), "{page}");
    assert!(page.contains("Test &lt;world&gt;") && page.contains("1 playing"), "{page}");
}

#[test]
fn games_join_over_tls_and_browsers_see_the_https_page() {
    let dir = std::env::temp_dir().join(format!("potluck-tls-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    let (cert_pem, key_pem) = (dir.join("fullchain.pem"), dir.join("privkey.pem"));
    std::fs::write(&cert_pem, cert.cert.pem()).unwrap();
    std::fs::write(&key_pem, cert.signing_key.serialize_pem()).unwrap();
    let cfg = transport::tls_config(&transport::TlsFiles { cert: &cert_pem, key: &key_pem }).unwrap();

    let port = free_port();
    let (stx, srx) = channel();
    transport::listen(port, Some(cfg), stx, status(), Arc::new(Mutex::new(0))).unwrap();

    // a client that trusts the test certificate
    let mut roots = rustls::RootCertStore::empty();
    roots.add(cert.cert.der().clone()).unwrap();
    let client = Arc::new(
        rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_root_certificates(roots)
            .with_no_client_auth(),
    );

    // wss://
    let tcp = TcpStream::connect(("127.0.0.1", port)).unwrap();
    tcp.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let url = format!("wss://localhost:{port}");
    let (mut ws, _) = tungstenite::client_tls_with_config(url.as_str(), tcp, None, Some(tungstenite::Connector::Rustls(client.clone()))).unwrap();
    let Wire::Joined(_, to_client) = wait(&srx, "the join over TLS") else { panic!("expected a join") };
    ws.send(tungstenite::Message::Binary(b"secret".to_vec().into())).unwrap();
    match wait(&srx, "the message over TLS") {
        Wire::Data(_, b) => assert_eq!(b, b"secret"),
        _ => panic!("expected data"),
    }
    to_client.send(b"welcome".to_vec()).unwrap();
    match ws.read().unwrap() {
        tungstenite::Message::Binary(b) => assert_eq!(&b[..], b"welcome"),
        m => panic!("expected binary, got {m:?}"),
    }

    // https://
    let tcp = TcpStream::connect(("127.0.0.1", port)).unwrap();
    let conn = rustls::ClientConnection::new(client, "localhost".try_into().unwrap()).unwrap();
    let mut tls = rustls::StreamOwned::new(conn, tcp);
    tls.write_all(b"GET /status.json HTTP/1.1\r\nHost: localhost\r\n\r\n").unwrap();
    let mut reply = Vec::new();
    let _ = tls.read_to_end(&mut reply);
    let reply = String::from_utf8_lossy(&reply);
    assert!(reply.starts_with("HTTP/1.1 200") && reply.contains("application/json") && reply.contains("\"Mossback\""), "{reply}");
}

#[test]
fn a_friend_joins_a_hosted_world_and_gets_it_with_its_creations() {
    use crate::net::{self, Event, Msg};
    home();
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    crate::sim(&mut app, crate::SEED);
    app.insert_resource(crate::settings::Settings { name: "Ken's world".into(), ..Default::default() });
    app.update();
    let port = free_port();
    app.world_mut().resource_mut::<net::Net>().host_with(net::HostOptions { name: "Ken's world".into(), port, tls: None, beacon: false }).unwrap();

    let (tx, rx) = channel();
    let to_host = transport::connect(&format!("127.0.0.1:{port}"), port, tx).unwrap();
    to_host.send(net::frame_msg(&Msg::Hello { name: "Drew".into(), protocol: net::PROTOCOL })).unwrap();

    let (mut welcome, mut world, mut mossback, mut herd) = (None, None, false, 0);
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(20) && !(welcome.is_some() && world.is_some() && mossback && herd == 3) {
        app.update();
        std::thread::sleep(Duration::from_millis(10));
        for w in rx.try_iter() {
            match net::decode(w) {
                Some(Event::Msg(_, Msg::Welcome { id, seed, host, .. })) => welcome = Some((id, seed, host)),
                Some(Event::World(_, bytes)) => world = Voxels::decode(crate::SEED, &bytes),
                Some(Event::Msg(_, Msg::Creation(p))) => mossback |= p.manifest.name == "Mossback",
                Some(Event::Msg(_, Msg::Herd(list))) => herd = list.len(),
                _ => {}
            }
        }
    }
    let (_, seed, host) = welcome.expect("the host says welcome");
    assert_eq!((seed, host.as_str()), (crate::SEED, "Ken's world"));
    let world = world.expect("the whole world arrives");
    assert!(world.encode() == app.world().resource::<Voxels>().encode(), "block for block the host's world");
    assert!(mossback, "the creations running in the world come with it");
    assert_eq!(herd, 3, "and so do its creatures");
}
