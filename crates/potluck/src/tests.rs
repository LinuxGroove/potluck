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
    app.world_mut().resource_mut::<net::Net>().host_with(net::HostOptions { name: "Ken's world".into(), port, tls: None, beacon: false, rules: Default::default() }).unwrap();

    let (tx, rx) = channel();
    let to_host = transport::connect(&format!("127.0.0.1:{port}"), port, tx).unwrap();
    to_host.send(net::frame_msg(&Msg::Hello { name: "Drew".into(), protocol: net::PROTOCOL, pass: None })).unwrap();

    let (mut welcome, mut world, mut mossback, mut herd) = (None, None, false, std::collections::HashSet::new());
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(20) && !(welcome.is_some() && world.is_some() && mossback && herd.len() == 3) {
        app.update();
        std::thread::sleep(Duration::from_millis(10));
        for w in rx.try_iter() {
            match net::decode(w) {
                Some(Event::Msg(_, Msg::Welcome { id, seed, host, .. })) => welcome = Some((id, seed, host)),
                Some(Event::World(_, bytes)) => world = Voxels::decode(crate::SEED, &bytes),
                Some(Event::Msg(_, Msg::Creation(p))) => mossback |= p.manifest.name == "Mossback",
                Some(Event::Msg(_, Msg::Bodies { list, .. })) => herd.extend(list.iter().filter(|b| b.creation == "mossback").map(|b| b.id)),
                _ => {}
            }
        }
    }
    let (_, seed, host) = welcome.expect("the host says welcome");
    assert_eq!((seed, host.as_str()), (crate::SEED, "Ken's world"));
    let world = world.expect("the whole world arrives");
    assert!(world.encode() == app.world().resource::<Voxels>().encode(), "block for block the host's world");
    assert!(mossback, "the creations running in the world come with it");
    assert_eq!(herd.len(), 3, "and so do its creatures: the three mossbacks");
}

#[test]
fn every_baked_block_texture_is_read() {
    let (maps, size) = crate::textures::baked();
    assert_eq!(maps.len(), crate::textures::BAKED.len(), "some baked block textures couldn't be read");
    assert!(size.is_power_of_two() && size >= crate::textures::TILE);
}

#[test]
fn grass_grows_tufts_that_sway_and_hangs_over_edges() {
    let mut images = Assets::<Image>::default();
    let atlas = crate::textures::build(&mut images);
    let w = world();
    let (x, z) = (world::START.x as i32, world::START.z as i32);
    let c = IVec3::new(x, w.top(x, z), z) / world::CHUNK;
    let thick = crate::mesh::mesh_chunk(&w, &atlas, c, crate::mesh::Cover::default(), true);
    let bare = crate::mesh::mesh_chunk(&w, &atlas, c, crate::mesh::Cover { density: 0.0, shadows: false }, true);
    let cover = thick.layers.into_iter().nth(crate::mesh::COVER).unwrap();
    let lips = bare.layers.into_iter().nth(crate::mesh::COVER).unwrap();
    assert!(!lips.is_empty(), "grass hangs over the edges of terraces even with no tufts");
    assert!(cover.into_mesh(false).has_morph_targets(), "tufts sway in the wind");
    assert!(!lips.into_mesh(false).has_morph_targets(), "grass lips don't");
}

// ---------- the market, from the game ----------

#[test]
fn a_qr_code_has_its_quiet_edge_and_dark_squares() {
    let img = crate::ui::qr_image("http://192.168.1.20:47180/signin/abcdefghijklmnopqrstuvwxyz012345").expect("a QR code");
    let size = img.texture_descriptor.size.width as usize;
    assert_eq!(size, img.texture_descriptor.size.height as usize);
    let data = img.data.as_ref().unwrap();
    assert_eq!(&data[..4], &[255, 255, 255, 255], "white round the edge");
    let dark = data.chunks(4).filter(|p| p[0] < 128).count();
    assert!(dark > size * size / 5 && dark < size * size * 2 / 3, "{dark} dark pixels of {}", size * size);
}

#[test]
fn remixes_know_what_they_came_from() {
    home();
    let mut lib = Library::load();
    let id = lib.remix("mossback", "Tester").unwrap();
    let m = &lib.list[&id].manifest;
    assert_eq!(m.from, Some(crate::creations::FromRef { code: None, base: Some("mossback".into()), version: 0 }));
    let toml: Manifest = toml::from_str(&lib.list[&id].files["potluck.toml"]).unwrap();
    assert_eq!(toml.from, m.from, "and say so in their potluck.toml");
    let mut c = creation(BOX);
    c.id = "from-a-code".into();
    c.origin = Origin::Code("ABCD2345".into());
    lib.add(c);
    lib.marks.insert("from-a-code".into(), crate::creations::Mark { code: "ABCD2345".into(), version: 3, everyone: false });
    let id = lib.remix("from-a-code", "Tester").unwrap();
    assert_eq!(lib.list[&id].manifest.from, Some(crate::creations::FromRef { code: Some("ABCD2345".into()), base: None, version: 3 }));
}

/// A stand-in for the game server's device sign-in, enough for the market's
/// dev accounts: the session token for a device is `dev:DEVICE:DEVICE`.
fn fake_game_server() -> String {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = format!("http://{}", l.local_addr().unwrap());
    std::thread::spawn(move || {
        for mut s in l.incoming().flatten() {
            let mut buf = Vec::new();
            let mut chunk = [0u8; 4096];
            while let Ok(n) = s.read(&mut chunk) {
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&chunk[..n]);
                let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") else { continue };
                let head = String::from_utf8_lossy(&buf[..end]).to_lowercase();
                let len: usize = head.lines().find_map(|l| l.strip_prefix("content-length:")).and_then(|v| v.trim().parse().ok()).unwrap_or(0);
                if buf.len() < end + 4 + len {
                    continue;
                }
                let body: serde_json::Value = serde_json::from_slice(&buf[end + 4..end + 4 + len]).unwrap_or_default();
                let id = body["id"].as_str().unwrap_or("someone");
                let out = serde_json::json!({ "token": format!("dev:{id}:{id}") }).to_string();
                let _ = write!(s, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{out}", out.len());
                break;
            }
        }
    });
    addr
}

/// The market, in this process, against the Forgejo the environment names
/// (or none, for what needs no git hosting: passes, the server list,
/// boards). It looks at servers on this machine.
fn start_market(forgejo: Option<(&str, &str)>, run: &str) -> (String, Arc<potluck_share::market::App>) {
    let (tx, rx) = channel();
    let (seed, forgejo, token) = match forgejo {
        Some((f, t)) => (true, f.to_string(), t.to_string()),
        None => (false, "http://127.0.0.1:9".to_string(), String::new()),
    };
    let run = run.to_string();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
        rt.block_on(async move {
            use potluck_share::{checks, config, market};
            let dir = std::env::temp_dir().join(format!("potluck-market-{run}"));
            std::fs::create_dir_all(&dir).unwrap();
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}", listener.local_addr().unwrap());
            let mut cfg = config::Config::default();
            cfg.public_url = url.clone();
            cfg.data = dir.clone();
            cfg.dev_accounts = true;
            cfg.invite_only = false;
            cfg.forgejo.url = forgejo.trim_end_matches('/').to_string();
            cfg.forgejo.token = token;
            cfg.forgejo.email_domain = "test.invalid".into();
            cfg.private_servers = true;
            let app = Arc::new(market::App::open(cfg, checks::Words::parse("grumbleweed\n")).unwrap());
            app.db().execute("INSERT INTO accounts (user_id, handle, name, created, beta, trusted) VALUES ('potluck', ?1, 'Potluck', 0, 1, 1)", [format!("potluck-{run}")]).unwrap();
            if seed {
                app.seed_base().await.unwrap();
            }
            tx.send((url, app.clone())).unwrap();
            potluck_share::serve(listener, app).await.unwrap();
        });
    });
    rx.recv_timeout(Duration::from_secs(60)).expect("the market starts")
}

fn done(jobs: &crate::share::ShareJobs, what: &str) -> crate::share::Done {
    let start = Instant::now();
    loop {
        if let Some(d) = jobs.poll().into_iter().next() {
            if let crate::share::Done::Failed(e) = &d {
                panic!("{what}: {e}");
            }
            return d;
        }
        assert!(start.elapsed() < Duration::from_secs(60), "{what} took too long");
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The market's done test, played from the game: Ada remixes Mossback and
/// shares it to Everyone, Bo loads it by its code, remixes it and suggests
/// his change, and Ada merges it from the game into her folder. Needs a
/// Forgejo: set POTLUCK_TEST_FORGEJO and POTLUCK_TEST_FORGEJO_TOKEN.
#[test]
fn a_remix_of_a_remix_is_suggested_and_merged_from_the_game() {
    use crate::creations::Mark;
    use crate::share::{Done, ShareJobs, Who};
    let (Ok(fj), Ok(token)) = (std::env::var("POTLUCK_TEST_FORGEJO"), std::env::var("POTLUCK_TEST_FORGEJO_TOKEN")) else {
        eprintln!("skipped: set POTLUCK_TEST_FORGEJO and POTLUCK_TEST_FORGEJO_TOKEN (deploy/market/test.sh does)");
        return;
    };
    home();
    let run = crate::settings::random_hex(5);
    let (market, _) = start_market(Some((&fj, &token)), &run);
    let server = fake_game_server();
    let who = |name: &str| Who { server: server.clone(), market: market.clone(), device: format!("{}-{run}", name.to_lowercase()), name: format!("{name} {run}") };
    let (ada, bo) = (who("Ada"), who("Bo"));
    let (ada_jobs, bo_jobs) = (ShareJobs::default(), ShareJobs::default());
    let mut lib = Library::load();

    let a_id = lib.remix("mossback", &ada.name).unwrap();
    let mut a = lib.list[&a_id].clone();
    a.files.get_mut("main.luau").unwrap().push_str("\n-- Ada's: a little greener\n");
    lib.replace_files(&a_id, a.files.clone()).unwrap();
    ada_jobs.share(&ada, a.payload(), true, None, a.manifest.from.clone());
    let Done::Shared { code: a_code, version, everyone, message, .. } = done(&ada_jobs, "Ada shares") else { panic!("not a share") };
    assert_eq!((version, everyone), (1, true));
    assert!(message.contains("moderator"), "{message}");
    lib.mark(&a_id, Mark { code: a_code.clone(), version, everyone });

    bo_jobs.load(&bo, &a_code, None);
    let Done::Loaded { code, version, payload } = done(&bo_jobs, "Bo loads Ada's") else { panic!("not a load") };
    let c = Creation::from_payload(payload, Origin::Code(code.clone())).unwrap();
    assert!(c.files["main.luau"].contains("a little greener"));
    let loaded = lib.add(c);
    lib.marks.insert(loaded.clone(), Mark { code: code.clone(), version, everyone: false });
    let b_id = lib.remix(&loaded, &bo.name).unwrap();
    let mut b = lib.list[&b_id].clone();
    assert_eq!(b.manifest.from.as_ref().and_then(|f| f.code.clone()), Some(a_code.clone()), "Bo's remix knows it came from Ada's");
    b.files.get_mut("main.luau").unwrap().push_str("-- Bo's: they hum when you pat them\n");
    bo_jobs.suggest(&bo, b.payload(), false, None, b.manifest.from.clone(), "Humming".into());
    let Done::Suggested { .. } = done(&bo_jobs, "Bo suggests") else { panic!("not a suggestion") };

    ada_jobs.suggestions(&ada, &a_id, &a_code);
    let Done::Suggestions { list, .. } = done(&ada_jobs, "Ada's suggestions") else { panic!("not a list") };
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].title, "Humming");
    ada_jobs.merge(&ada, &a_id, list[0].id);
    let Done::Merged { files, version, .. } = done(&ada_jobs, "Ada merges") else { panic!("not a merge") };
    assert_eq!(version, 2);
    lib.replace_files(&a_id, files).unwrap();
    let dir = lib.list[&a_id].dir.clone().unwrap();
    let main = std::fs::read_to_string(dir.join("main.luau")).unwrap();
    assert!(main.contains("they hum when you pat them") && main.contains("a little greener"), "both changes are in Ada's folder");
    let toml: Manifest = toml::from_str(&std::fs::read_to_string(dir.join("potluck.toml")).unwrap()).unwrap();
    assert_eq!(toml.by, ada.name, "and it's still Ada's");

    ada_jobs.link(&ada, "/me/profile", "Your profile");
    let Done::Link { url, .. } = done(&ada_jobs, "the profile link") else { panic!("not a link") };
    assert!(url.starts_with(&format!("{market}/signin/")), "{url}");
    assert!(crate::ui::qr_image(&url).is_some());
}

/// M3's server list from both ends: Ada lists her server, which lets in
/// only players the market vouches for. Bo gets a pass for it and comes in
/// under his market name; Cy, with no pass, is turned away. The market lists
/// the server once it has reached its status page, the game finds it there,
/// and a creation's board on the server reaches the creation's page.
#[test]
fn a_listed_server_lets_in_who_the_market_vouches_for() {
    use crate::hosting::{Join, Rules};
    use crate::net::{self, Event, Msg};
    use crate::share::{Done, MarketLink, ShareJobs, Who};
    home();
    let run = crate::settings::random_hex(5);
    let (market, mkt) = start_market(None, &run);
    let server = fake_game_server();
    let who = |name: &str| Who { server: server.clone(), market: market.clone(), device: format!("{}-{run}", name.to_lowercase()), name: format!("{name} {run}") };

    // Riverbend: signed-in players only, at an address the passes name
    let port = free_port();
    let address = format!("ws://127.0.0.1:{port}");
    let key = MarketLink::new(&market, "").key().expect("the market's key");
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    crate::sim(&mut app, crate::SEED);
    app.insert_resource(crate::settings::Settings { name: "Riverbend".into(), ..Default::default() });
    app.update();
    let rules = Rules { max_players: 50, join: Join::SignedIn, address: address.clone(), market_key: Some(key), ..Default::default() };
    app.world_mut().resource_mut::<net::Net>().host_with(net::HostOptions { name: "Riverbend".into(), port, tls: None, beacon: false, rules }).unwrap();

    // a player knocks: Some(name in the world) when welcomed, or why not
    let mut knock = |pass: Option<String>, name: &str| -> Result<String, String> {
        let (tx, rx) = channel();
        let to_host = transport::connect(&address, port, tx).unwrap();
        to_host.send(net::frame_msg(&Msg::Hello { name: name.into(), protocol: net::PROTOCOL, pass })).unwrap();
        let t = Instant::now();
        let mut welcomed = false;
        while t.elapsed() < Duration::from_secs(20) {
            app.update();
            std::thread::sleep(Duration::from_millis(10));
            for w in rx.try_iter() {
                match net::decode(w) {
                    Some(Event::Msg(_, Msg::Welcome { .. })) => welcomed = true,
                    Some(Event::Msg(_, Msg::Refused(why))) => return Err(why),
                    Some(Event::Msg(_, Msg::Roster(all))) if welcomed => return Ok(all.last().cloned().unwrap_or_default()),
                    _ => {}
                }
            }
        }
        panic!("{name} heard nothing back");
    };

    let bo = who("Bo");
    let jobs = ShareJobs::default();
    jobs.pass(&bo, &address);
    let Done::Pass { pass, .. } = done(&jobs, "Bo's pass") else { panic!("not a pass") };
    assert!(pass.is_some(), "the market gives signed-in players a pass");
    assert_eq!(knock(pass.clone(), "bo the impostor").as_deref(), Ok(bo.name.as_str()), "Bo comes in under the name the market knows");
    let why = knock(None, "Cy").expect_err("no pass, no entry");
    assert!(why.contains("signed in"), "{why}");
    jobs.pass(&bo, "ws://elsewhere.example.org");
    let Done::Pass { pass: elsewhere, .. } = done(&jobs, "a pass for another server") else { panic!("not a pass") };
    let why = knock(elsewhere, "Bo").expect_err("a pass for another server doesn't open this one");
    assert!(why.contains(&address), "it says which address to use: {why}");

    // Ada lists Riverbend: a token from Your servers, then a heartbeat
    let ada = potluck_share::db::ensure_account(&mkt.db(), &format!("dev-ada-{run}"), &format!("Ada {run}")).unwrap();
    let (_, token) = mkt.add_server(&ada, "Riverbend").unwrap();
    let link = MarketLink::new(&market, &token);
    let beat = serde_json::json!({ "name": "Riverbend", "about": "Rodeo nights", "address": address, "players": 1, "max_players": 50, "join": "signed-in", "creations": [{ "name": "Rodeo", "code": "ABCD2345" }], "version": "test" });
    let said = link.heartbeat(&beat).expect("the heartbeat");
    assert_eq!(said["listed"], true, "listed once the market reached its status page: {said}");
    assert!(MarketLink::new(&market, "not-a-token").heartbeat(&beat).is_err(), "only tokens the market made");

    jobs.servers(&who("Cy"));
    let Done::Servers(list) = done(&jobs, "the server list") else { panic!("not a list") };
    let listed = list.iter().find(|s| s.name == "Riverbend").expect("Riverbend is on the list");
    assert_eq!((listed.address.as_str(), listed.join.as_str(), listed.creations.clone()), (address.as_str(), "signed-in", vec!["Rodeo".to_string()]));

    // a board kept on Riverbend shows on the creation's page
    let handle = bo.name.to_lowercase().replace(' ', "-");
    link.boards(&serde_json::json!({ "boards": [{ "creation": "ABCD2345", "board": "longest ride", "scores": [{ "name": bo.name, "handle": handle, "value": 12.5 }, { "name": "Cy", "value": 30.0 }] }] })).expect("boards");
    let mut r = ureq::get(format!("{market}/api/c/ABCD2345/boards")).call().unwrap();
    let v: serde_json::Value = serde_json::from_str(&r.body_mut().read_to_string().unwrap()).unwrap();
    let scores = &v["boards"][0]["scores"];
    assert_eq!(v["boards"][0]["name"], "longest ride", "{v}");
    assert_eq!((scores[0]["name"].as_str(), scores[0]["server"].as_str(), scores[1]["value"].as_f64()), (Some("Cy"), Some("Riverbend"), Some(12.5)), "best first, with the server it was on: {v}");
}

/// `@potluck/game`: a creation keeps a board of each player's best and
/// sends players on to servers this one links to, and nothing else.
#[test]
fn creations_keep_boards_and_send_players_on() {
    let mut voxels = world();
    let mut herd = Herd::default();
    let mut scripts = Scripts::default();
    scripts.shared.lock().unwrap().links = vec!["rodeo".into()];
    scripts.players = vec![PlayerInfo { id: 7, name: "Ada".into(), pos: Vec3::new(0.0, 40.0, 0.0) }];
    let code = format!(
        "{BOX}\nlocal game = require(\"@potluck/game@0.1\")\nlocal sent = false\nfunction Thing:tick(dt)\n  if sent then return end\n  sent = true\n  local ada = world.players()[1]\n  game.score(\"longest ride\", ada, 12)\n  game.score(\"fastest lap\", ada, 31.5, {{ lower = true }})\n  local ok = pcall(game.send, ada, \"nowhere\")\n  self:say(ok and 'sent nowhere' or table.concat(game.links(), ','))\n  game.send(ada, \"rodeo\", \"Off to the rodeo!\")\nend"
    );
    scripts.start(&creation(&code), &mut herd).unwrap();
    run(&mut scripts, &mut voxels, &mut herd, 0.2);
    assert_eq!(scripts.vms["test"].error, None);
    let said: Vec<String> = herd.bodies.values().filter_map(|b| b.says.as_ref().map(|s| s.0.clone())).collect();
    assert_eq!(said, vec!["rodeo".to_string()], "only the links the server has");
    let scores: Vec<_> = scripts.out.scores.iter().map(|(c, b, lower, p, v)| (c.as_str(), b.as_str(), *lower, *p, *v)).collect();
    assert_eq!(scores, vec![("test", "longest ride", false, 7, 12.0), ("test", "fastest lap", true, 7, 31.5)]);
    assert_eq!(scripts.out.sends, vec![(7, "rodeo".to_string(), "Off to the rodeo!".to_string())]);
}

/// The base game is creations: a dozen of them load, run a minute of the
/// world while a player walks up to one creature after another, pats or
/// bops it (and rides the rideable ones), and none of them errors.
#[test]
fn every_base_creation_runs_and_answers_players() {
    home();
    let lib = Library::load();
    let base: Vec<&Creation> = lib.list.values().filter(|c| c.origin == Origin::Base).collect();
    assert!(base.len() >= 12, "a dozen creations in the base game, not {}", base.len());
    let mut voxels = world();
    voxels.changes.clear();
    let mut herd = Herd::default();
    let mut scripts = Scripts::default();
    for c in &base {
        scripts.start(c, &mut herd).unwrap_or_else(|e| panic!("{} doesn't start: {e}", c.manifest.name));
        assert!(c.manifest.needs.contains_key("world"), "{} says which modules it needs", c.manifest.name);
    }
    run(&mut scripts, &mut voxels, &mut herd, 2.0);
    let kinds: std::collections::BTreeSet<String> = herd.bodies.values().map(|b| b.kind.clone()).collect();
    for c in &base {
        assert!(herd.bodies.values().any(|b| b.creation == c.id), "{}'s creatures appear", c.manifest.name);
    }
    for second in 0..60usize {
        let ids: Vec<u64> = herd.bodies.keys().copied().collect();
        let id = ids[(second * 7) % ids.len()];
        let b = herd.bodies[&id].clone();
        let ada = PlayerInfo { id: 7, name: "Ada".into(), pos: b.pos + Vec3::X * 1.5 };
        scripts.players = vec![ada.clone()];
        if b.rideable && second % 3 == 0 {
            herd.bodies.get_mut(&id).unwrap().rider = Some(7);
            scripts.event(&mut voxels, &mut herd, id, "ridden", &ada);
        } else {
            scripts.event(&mut voxels, &mut herd, id, if second % 2 == 0 { "used" } else { "hit" }, &ada);
        }
        run(&mut scripts, &mut voxels, &mut herd, 1.0);
        for (id, vm) in &scripts.vms {
            assert_eq!(vm.error.as_deref(), None, "{id} has an error");
        }
    }
    assert!(kinds.len() >= 13, "every kind of creature shows up: {kinds:?}");
    assert!(herd.bodies.len() < 64 * base.len(), "nothing multiplies without end");
    // gardeners and moles change the world a little, never a lot
    let changed = voxels.changes.len();
    assert!(changed < 40, "{changed} blocks changed in a minute");

    // the two mini games: a tag, and a ride on the Rodeo Ram that ends on the board
    scripts.out.scores.clear();
    let find = |herd: &Herd, kind: &str| herd.bodies.values().find(|b| b.kind == kind).map(|b| (b.id, b.pos)).unwrap();
    let (bot, at) = find(&herd, "Tag Bot");
    let ada = PlayerInfo { id: 7, name: "Ada".into(), pos: at + Vec3::X };
    scripts.players = vec![ada.clone()];
    scripts.event(&mut voxels, &mut herd, bot, "hit", &ada);
    assert!(scripts.out.scores.iter().any(|s| s.1 == "tags" && s.3 == 7 && s.4 == 1.0), "a tag is scored: {:?}", scripts.out.scores);
    let (ram, at) = find(&herd, "Rodeo Ram");
    let ada = PlayerInfo { pos: at, ..ada };
    scripts.players = vec![ada.clone()];
    herd.bodies.get_mut(&ram).unwrap().rider = Some(7);
    scripts.event(&mut voxels, &mut herd, ram, "ridden", &ada);
    for _ in 0..60 {
        run(&mut scripts, &mut voxels, &mut herd, 0.5);
        if herd.bodies[&ram].rider.is_none() {
            break;
        }
    }
    assert!(herd.bodies[&ram].rider.is_none(), "nobody stays on the Rodeo Ram for half a minute");
    run(&mut scripts, &mut voxels, &mut herd, 0.5);
    assert!(scripts.out.scores.iter().any(|s| s.1 == "longest ride" && s.3 == 7 && s.4 > 0.0), "the ride is on the board: {:?}", scripts.out.scores);
}
