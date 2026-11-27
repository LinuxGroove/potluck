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
    let vm = &scripts.vms["test"];
    assert_eq!(said, vec!["sealed".to_string()], "no files, no programs, no loading code ({:?}, {:?})", vm.error, vm.log);
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

/// With POTLUCK_DUMP_TEX=<dir>, writes every drawn texture there to look at.
#[test]
fn dump_textures_when_asked() {
    if let Some(dir) = std::env::var_os("POTLUCK_DUMP_TEX") {
        crate::textures::dump(std::path::Path::new(&dir)).unwrap();
    }
}

#[test]
fn every_baked_block_texture_is_read() {
    let (maps, size) = crate::textures::baked();
    assert_eq!(maps.len(), crate::textures::BAKED.len(), "some baked block textures couldn't be read");
    assert!(size.is_power_of_two() && size >= crate::textures::TILE);
}

#[test]
fn grass_grows_tufts_that_sway_and_hangs_over_edges() {
    use crate::mesh::Detail;
    let mut images = Assets::<Image>::default();
    let atlas = crate::textures::build(&mut images);
    let w = world();
    let (x, z) = (world::START.x as i32, world::START.z as i32);
    let c = IVec3::new(x, w.top(x, z), z) / world::CHUNK;
    let thick = crate::mesh::mesh_chunk(&w, &atlas, c, crate::mesh::Cover::default(), Detail::Near);
    let bare = crate::mesh::mesh_chunk(&w, &atlas, c, crate::mesh::Cover { density: 0.0, shadows: false, ..default() }, Detail::Near);
    assert!(thick.varies);
    let cover = thick.layers.into_iter().nth(crate::mesh::COVER).unwrap();
    let lips = bare.layers.into_iter().nth(crate::mesh::COVER).unwrap();
    assert!(!lips.is_empty(), "grass hangs over the edges of terraces even with no tufts");
    assert!(cover.into_mesh(false).has_morph_targets(), "tufts sway in the wind");
    assert!(!lips.into_mesh(false).has_morph_targets(), "grass lips don't");
}

#[test]
fn far_chunks_keep_fewer_tufts_and_keep_still() {
    use crate::mesh::Detail;
    let mut images = Assets::<Image>::default();
    let atlas = crate::textures::build(&mut images);
    let w = world();
    let (x, z) = (world::START.x as i32, world::START.z as i32);
    let c = IVec3::new(x, w.top(x, z), z) / world::CHUNK;
    let quads = |d: Detail| {
        let m = crate::mesh::mesh_chunk(&w, &atlas, c, crate::mesh::Cover::default(), d);
        let mut l = m.layers.into_iter();
        let opaque = l.next().unwrap().into_mesh(false).count_vertices();
        let cover = l.nth(crate::mesh::COVER - 1).unwrap().into_mesh(false);
        (opaque, cover.count_vertices(), cover.has_morph_targets())
    };
    let (near, mid, far, distant) = (quads(Detail::Near), quads(Detail::Mid), quads(Detail::Far), quads(Detail::Distant));
    assert!(near.0 == distant.0, "the blocks themselves are the same at every distance");
    assert!(near.1 > mid.1 && mid.1 > far.1 && far.1 > distant.1, "{near:?} {mid:?} {far:?} {distant:?}");
    assert!(near.2 && !mid.2 && !far.2, "only near chunks sway");
    // a chunk at a boundary keeps its level until it's well past it
    let mid_of = |c: IVec3| (c * world::CHUNK).as_vec3() + Vec3::splat(world::CHUNK as f32 / 2.0);
    let at = mid_of(IVec3::new(2, 1, 2));
    let edge = at + Vec3::new(52.0, 0.0, 0.0);
    assert_eq!(Detail::of(IVec3::new(2, 1, 2), edge, Some(Detail::Near)), Detail::Near);
    assert_eq!(Detail::of(IVec3::new(2, 1, 2), edge, Some(Detail::Mid)), Detail::Mid);
    assert_eq!(Detail::of(IVec3::new(2, 1, 2), at, None), Detail::Near);
    assert_eq!(Detail::of(IVec3::new(2, 1, 2), at + Vec3::new(300.0, 0.0, 0.0), Some(Detail::Near)), Detail::Distant);
}

#[test]
fn every_block_has_its_textures() {
    let mut images = Assets::<Image>::default();
    let atlas = crate::textures::build(&mut images);
    for b in crate::blocks::BLOCKS.iter().skip(1) {
        for name in b.faces.iter().chain(b.model.iter().flat_map(|c| c.tex.iter())).filter(|n| !n.is_empty()) {
            assert!(atlas.has(name), "{} has no {name} texture", b.name);
        }
    }
}

#[test]
fn models_are_boxes_that_join_their_neighbours() {
    use crate::blocks::*;
    use crate::mesh::{Detail, COVER};
    let mut images = Assets::<Image>::default();
    let atlas = crate::textures::build(&mut images);
    let quads = |w: &Voxels, layer: usize| {
        let m = crate::mesh::mesh_chunk(w, &atlas, IVec3::ZERO, crate::mesh::Cover::default(), Detail::Near);
        m.layers.into_iter().nth(layer).unwrap().into_mesh(false).count_vertices() / 4
    };
    // two columns share a shaft: no capital and base where they meet, only
    // at the foot and the top
    let mut w = Voxels::empty(1);
    w.set(IVec3::new(4, 4, 4), COLUMN);
    w.set(IVec3::new(4, 5, 4), COLUMN);
    let m = crate::mesh::mesh_chunk(&w, &atlas, IVec3::ZERO, crate::mesh::Cover::default(), Detail::Near);
    let mesh = m.layers.into_iter().next().unwrap().into_mesh(false);
    let Some(bevy::mesh::VertexAttributeValues::Float32x3(pos)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else { panic!() };
    let wide: Vec<f32> = pos.iter().filter(|p| (p[0] - (4.0 + 1.0 / 16.0)).abs() < 1e-4).map(|p| p[1]).collect();
    assert!(!wide.is_empty() && wide.iter().all(|y| *y <= 4.0 + 3.0 / 16.0 + 1e-4 || *y >= 5.0 + 13.0 / 16.0 - 1e-4), "{wide:?}");
    // a candle's wax is solid, its flame glows on its own
    let mut w = Voxels::empty(1);
    w.set(IVec3::new(4, 4, 4), CANDLE);
    assert_eq!(quads(&w, 4), 6, "the flame is one box");
    assert!(quads(&w, 0) > 0);
    assert_eq!(quads(&w, COVER), 0);
    assert!(!is_solid(CANDLE) && is_solid(TABLE) && !is_opaque(TABLE));
    // a fence reaches for the fence beside it and not elsewhere
    let mut w = Voxels::empty(1);
    w.set(IVec3::new(4, 4, 4), FENCE);
    let post = quads(&w, 0);
    w.set(IVec3::new(5, 4, 4), FENCE);
    assert_eq!(quads(&w, 0), post * 2 + 4 * 6, "two posts, a pair of rails each toward the other");
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
    let who = |name: &str| Who { server: server.clone(), market: market.clone(), device: format!("{}-{run}", name.to_lowercase()), name: format!("{name} {run}"), young: false };
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

#[test]
fn rooms_under_a_roof_see_less_sky() {
    use crate::mesh::Detail;
    use bevy::mesh::VertexAttributeValues;
    let mut images = Assets::<Image>::default();
    let atlas = crate::textures::build(&mut images);
    let w = world();
    // the share of sky a floor's corner sees, as its chunk's mesh carries it
    let sky = |x: i32, y: i32, z: i32| -> f32 {
        let c = IVec3::new(x, y - 1, z).div_euclid(IVec3::splat(world::CHUNK));
        let m = crate::mesh::mesh_chunk(&w, &atlas, c, crate::mesh::Cover::default(), Detail::Near);
        let mesh = m.layers.into_iter().next().unwrap().into_mesh(false);
        let pos = mesh.attribute(Mesh::ATTRIBUTE_POSITION).unwrap().as_float3().unwrap();
        let Some(VertexAttributeValues::Float32x2(uv)) = mesh.attribute(Mesh::ATTRIBUTE_UV_1) else { panic!("no sky in the mesh") };
        let i = pos.iter().position(|p| *p == [x as f32, y as f32, z as f32]).expect("a floor corner there");
        let steps = crate::mesh::SKY_STEPS as f32;
        (uv[i][0] * steps - 0.5) / (steps - 1.0)
    };
    // the villa's library floor, off the skylight, and its terrace outside
    let (inside, under_glass, open) = (sky(48, 26, 202), sky(54, 26, 206), sky(39, 26, 196));
    eprintln!("sky: library {inside:.2}, under the skylight {under_glass:.2}, terrace {open:.2}");
    assert!(inside < 0.2, "the library sees little sky ({inside})");
    assert!(under_glass > inside + 0.05, "more under the skylight ({under_glass} against {inside})");
    assert!(open > 0.75, "the open terrace sees most of it ({open})");
}

#[test]
fn shores_get_foam() {
    use crate::mesh::Detail;
    let mut images = Assets::<Image>::default();
    let atlas = crate::textures::build(&mut images);
    let w = world();
    // the beach below the villa: water at x 34 against sand at x 35
    assert_eq!((w.get(34, world::SEA, 207), w.get(35, world::SEA, 207)), (crate::blocks::WATER, crate::blocks::SAND));
    let c = IVec3::new(34, world::SEA, 207).div_euclid(IVec3::splat(world::CHUNK));
    let m = crate::mesh::mesh_chunk(&w, &atlas, c, crate::mesh::Cover::default(), Detail::Near);
    let plants = m.layers.into_iter().nth(3).unwrap().into_mesh(false);
    let pos = plants.attribute(Mesh::ATTRIBUTE_POSITION).unwrap().as_float3().unwrap();
    let foam = pos.iter().filter(|p| (p[1] - (world::SEA as f32 + 0.9)).abs() < 1e-3 && p[0] == 35.0 && p[2] >= 207.0 && p[2] <= 208.0).count();
    assert!(foam >= 2, "a strip of foam on the water along the sand ({foam} corners)");
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
    let who = |name: &str| Who { server: server.clone(), market: market.clone(), device: format!("{}-{run}", name.to_lowercase()), name: format!("{name} {run}"), young: false };

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
    let beat = serde_json::json!({ "name": "Riverbend", "about": "Rodeo nights", "address": address, "players": 1, "max_players": 50, "join": "signed-in", "creations": [{ "name": "Rodeo", "code": "ABCD2345" }], "version": "test", "all_ages": true });
    let said = link.heartbeat(&beat).expect("the heartbeat");
    assert_eq!(said["listed"], true, "listed once the market reached its status page: {said}");
    assert!(MarketLink::new(&market, "not-a-token").heartbeat(&beat).is_err(), "only tokens the market made");

    jobs.servers(&who("Cy"));
    let Done::Servers(list) = done(&jobs, "the server list") else { panic!("not a list") };
    let listed = list.iter().find(|s| s.name == "Riverbend").expect("Riverbend is on the list");
    assert_eq!((listed.address.as_str(), listed.join.as_str(), listed.creations.clone()), (address.as_str(), "signed-in", vec!["Rodeo".to_string()]));
    assert!(listed.all_ages, "it says it's for all ages");

    // a board kept on Riverbend shows on the creation's page
    let handle = bo.name.to_lowercase().replace(' ', "-");
    link.boards(&serde_json::json!({ "boards": [{ "creation": "ABCD2345", "board": "longest ride", "scores": [{ "name": bo.name, "handle": handle, "value": 12.5 }, { "name": "Cy", "value": 30.0 }] }] })).expect("boards");
    let mut r = ureq::get(format!("{market}/api/c/ABCD2345/boards")).call().unwrap();
    let v: serde_json::Value = serde_json::from_str(&r.body_mut().read_to_string().unwrap()).unwrap();
    let scores = &v["boards"][0]["scores"];
    assert_eq!(v["boards"][0]["name"], "longest ride", "{v}");
    assert_eq!((scores[0]["name"].as_str(), scores[0]["server"].as_str(), scores[1]["value"].as_f64()), (Some("Cy"), Some("Riverbend"), Some(12.5)), "best first, with the server it was on: {v}");
}

fn failed(jobs: &crate::share::ShareJobs, what: &str) -> String {
    let start = Instant::now();
    loop {
        if let Some(d) = jobs.poll().into_iter().next() {
            match d {
                crate::share::Done::Failed(e) => return e,
                _ => panic!("{what} should have failed"),
            }
        }
        assert!(start.elapsed() < Duration::from_secs(60), "{what} took too long");
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// One account on several devices, with no email or password: Ada makes a
/// code on her first device and types it on her Steam Deck, which then
/// plays as her, servers included. A code works once, a device with things
/// of its own on the market can't be swallowed, a recovery code from the
/// web brings her account to a new device once, and unlinking (from the
/// device or the web) gives a device back its own account.
#[test]
fn another_device_plays_as_you_with_a_code() {
    use crate::share::{Done, ShareJobs, Who};
    home();
    let run = crate::settings::random_hex(5);
    let (market, mkt) = start_market(None, &run);
    let server = fake_game_server();
    // each device signs in to the game server as itself, with jobs of its own
    let device = |id: &str, name: &str| (Who { server: server.clone(), market: market.clone(), device: format!("{id}-{run}"), name: format!("{name} {run}"), young: false }, ShareJobs::default());
    let (ada, deck, phone, bo, tablet) = (device("ada", "Ada"), device("adadeck", "Deck"), device("adaphone", "Phone"), device("bo", "Bo"), device("adatablet", "Tablet"));
    let account = |(w, jobs): &(Who, ShareJobs)| {
        jobs.account(w);
        let Done::Account(a) = done(jobs, "who this device is") else { panic!("not an account") };
        a
    };
    let first = account(&ada);
    assert!(!first.linked);
    let deck_own = account(&deck);
    assert_ne!(deck_own.handle, first.handle, "the Deck starts out as itself");

    ada.1.link_code(&ada.0);
    let Done::LinkCode(code) = done(&ada.1, "a link code") else { panic!("not a code") };
    assert_eq!(crate::share::clean_code(&code).len(), 8, "{code}");
    deck.1.link_device(&deck.0, &code.to_lowercase());
    let Done::Linked { name, handle, recovery } = done(&deck.1, "the Deck links") else { panic!("not linked") };
    assert_eq!((name.as_str(), handle.as_str(), recovery), (ada.0.name.as_str(), first.handle.as_str(), false));
    let on_deck = account(&deck);
    assert!(on_deck.linked && on_deck.handle == first.handle, "the Deck plays as Ada: {on_deck:?}");
    assert!(potluck_share::db::account_by_handle(&mkt.db(), &deck_own.handle).unwrap().is_none(), "and its empty account is gone");
    phone.1.link_device(&phone.0, &code);
    assert!(failed(&phone.1, "the same code again").contains("run out"), "a code works once");

    // a server's pass from the Deck names Ada
    deck.1.pass(&deck.0, "ws://riverbend.example.org");
    let Done::Pass { pass: Some(pass), .. } = done(&deck.1, "the Deck's pass") else { panic!("no pass") };
    use base64::Engine;
    let body = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(pass.split('.').next().unwrap()).unwrap();
    let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body["handle"].as_str(), Some(first.handle.as_str()), "{body}");

    // Bo's device has a server of its own on the market, so it can't join Ada
    let bo_account = account(&bo);
    let bo_row = potluck_share::db::account_by_handle(&mkt.db(), &bo_account.handle).unwrap().unwrap();
    mkt.add_server(&bo_row, "Bo's").unwrap();
    ada.1.link_code(&ada.0);
    let Done::LinkCode(code) = done(&ada.1, "another link code") else { panic!("not a code") };
    bo.1.link_device(&bo.0, &code);
    assert!(failed(&bo.1, "Bo links").contains("left behind"));
    assert_eq!(account(&bo).handle, bo_account.handle, "Bo is still Bo");

    // a recovery code from the web, typed the way it was shown
    let ada_row = potluck_share::db::account_by_handle(&mkt.db(), &first.handle).unwrap().unwrap();
    let recovery_code = potluck_share::codes::pretty(&mkt.recovery_code(&ada_row).unwrap());
    assert_eq!(recovery_code.len(), 23, "{recovery_code}");
    phone.1.link_device(&phone.0, &recovery_code);
    let Done::Linked { recovery, .. } = done(&phone.1, "the phone recovers") else { panic!("not linked") };
    assert!(recovery);
    assert!(account(&phone).linked);
    tablet.1.link_device(&tablet.0, &recovery_code);
    assert!(failed(&tablet.1, "the recovery code again").contains("run out"), "a recovery code works once");
    assert_eq!(mkt.recovery_made(&ada_row).unwrap(), None, "and it's gone from her page");

    // the Deck leaves; the web unlinks the phone
    let listed = mkt.devices(&ada_row).unwrap();
    assert_eq!(listed.len(), 2, "{listed:?}");
    assert!(listed.iter().all(|d| d.label == crate::share::device_label()));
    deck.1.unlink(&deck.0);
    let Done::Unlinked = done(&deck.1, "the Deck leaves") else { panic!("not unlinked") };
    let after = account(&deck);
    assert!(!after.linked && after.handle != first.handle, "the Deck has an account of its own again: {after:?}");
    mkt.unlink(&ada_row, mkt.devices(&ada_row).unwrap()[0].id).unwrap();
    assert!(!account(&phone).linked);
    assert!(mkt.devices(&ada_row).unwrap().is_empty());
    assert_eq!(account(&ada).handle, first.handle, "the first device is always hers");
}

/// Younger players: the game tells the market, their passes say so, and a
/// listed server turns them away unless it's for all ages (a friend's world
/// never does). Their menus offer sharing by code only, suggestions not at
/// all, and only all-ages servers; a parent can turn sharing and servers
/// off, behind a PIN kept as a hash.
#[test]
fn younger_players_share_with_friends_and_play_where_its_for_all_ages() {
    use crate::hosting::Rules;
    use crate::settings::Settings;
    use crate::share::{Done, Listed, MarketLink, ShareJobs, Who};
    use crate::ui::{menu_items, Act, Focus, Kind, Screen, WorldInfo};
    home();
    let run = crate::settings::random_hex(5);
    let (market, mkt) = start_market(None, &run);
    let server = fake_game_server();
    let kid = Who { server: server.clone(), market: market.clone(), device: format!("kid-{run}"), name: format!("Kid {run}"), young: true };
    let jobs = ShareJobs::default();
    jobs.account(&kid);
    let Done::Account(a) = done(&jobs, "the kid's account") else { panic!("not an account") };
    assert!(potluck_share::db::account_by_handle(&mkt.db(), &a.handle).unwrap().unwrap().young, "the market knows");

    let address = "ws://rodeo.example.org";
    jobs.pass(&kid, address);
    let Done::Pass { pass: Some(pass), .. } = done(&jobs, "the kid's pass") else { panic!("no pass") };
    let key = MarketLink::new(&market, "").key().unwrap();
    let mut rules = Rules { address: address.into(), market_key: Some(key), listed: true, ..Default::default() };
    let why = rules.admit("Kid", Some(&pass), 0).expect_err("a listed server that isn't for all ages");
    assert!(why.contains("13 and older"), "{why}");
    rules.all_ages = true;
    assert!(rules.admit("Kid", Some(&pass), 0).is_ok(), "an all-ages server");
    rules.all_ages = false;
    rules.listed = false;
    assert!(rules.admit("Kid", Some(&pass), 0).is_ok(), "a friend's world");

    // a parent unticks Under 13: the market hears on the next call
    let older = Who { young: false, ..kid.clone() };
    jobs.account(&older);
    let Done::Account(_) = done(&jobs, "the account again") else { panic!("not an account") };
    assert!(!potluck_share::db::account_by_handle(&mkt.db(), &a.handle).unwrap().unwrap().young);

    // the menus
    let mut lib = Library::load();
    let id = lib.remix("mossback", "Kid").unwrap();
    let scripts = Scripts::default();
    let info = WorldInfo::default();
    let mut settings = Settings { young: true, age_asked: true, ..Default::default() };
    let mut focus = Focus::default();
    let acts = |focus: &Focus, settings: &Settings| -> Vec<Act> {
        menu_items(focus, &lib, &scripts, settings, &info).1.into_iter().filter_map(|i| if let Kind::Button(a) = i.kind { Some(a) } else { None }).collect()
    };
    focus.go(Screen::Share(id.clone()));
    let share = acts(&focus, &settings);
    assert!(share.contains(&Act::ShareTo(id.clone(), false)) && !share.contains(&Act::ShareTo(id.clone(), true)), "by code only: {share:?}");
    focus.go(Screen::Inside(id.clone()));
    assert!(!acts(&focus, &settings).contains(&Act::Suggest(id.clone())), "no suggestions to strangers");
    settings.young = false;
    assert!(acts(&focus, &settings).contains(&Act::Suggest(id.clone())), "older players suggest");
    settings.sharing = false;
    assert!(!acts(&focus, &settings).contains(&Act::Share(id.clone())), "a parent turned sharing off");
    settings = Settings { young: true, ..Default::default() };
    focus.servers = vec![Listed { name: "Rodeo".into(), address: "a".into(), all_ages: true, ..Default::default() }, Listed { name: "Late".into(), address: "b".into(), ..Default::default() }];
    focus.go(Screen::Servers);
    assert_eq!(acts(&focus, &settings), vec![Act::JoinAddress("a".into()), Act::Back], "only all-ages servers");
    settings.servers = false;
    assert_eq!(acts(&focus, &settings), vec![Act::Back]);

    // the PIN is kept as a hash
    settings.parent_pin = settings.pin_hash("2468");
    assert!(!settings.parent_pin.contains("2468") && settings.pin_matches("2468") && !settings.pin_matches("1357"));
    focus.go(Screen::Parents);
    focus.parents_open = false;
    assert!(acts(&focus, &settings).iter().any(|a| matches!(a, Act::UnlockParents(_))), "locked until the PIN");
}

/// The first time anything would go online, the game asks who's playing,
/// then does what the player picked; it never asks again.
#[test]
fn the_age_question_comes_once_before_going_online() {
    use bevy::ecs::system::RunSystemOnce;
    use crate::settings::Settings;
    use crate::ui::{Act, Acts, Focus, Screen};
    home();
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    crate::sim(&mut app, crate::SEED);
    app.insert_resource(Settings::default()).insert_resource(crate::player::Me::default());
    let mut run = |app: &mut App, act: Act| {
        app.world_mut().resource_mut::<Acts>().0.push(act);
        app.world_mut().run_system_once(crate::game::do_acts).unwrap();
        // what an act asks for runs on the next frame
        app.world_mut().run_system_once(crate::game::do_acts).unwrap();
    };

    // playing alone asks nothing
    run(&mut app, Act::OpenSettings);
    assert!(!matches!(app.world().resource::<Focus>().screen, Screen::Age(_)), "nothing online yet");

    // the server list waits for the answer, then opens
    run(&mut app, Act::OpenServers);
    assert!(matches!(&app.world().resource::<Focus>().screen, Screen::Age(next) if **next == Act::OpenServers), "asked first");
    run(&mut app, Act::SetAge(true));
    let settings = app.world().resource::<Settings>();
    assert!(settings.age_asked && settings.young);
    assert!(matches!(app.world().resource::<Focus>().screen, Screen::Servers), "then the servers");

    // and not again
    run(&mut app, Act::OpenDevices);
    assert!(matches!(app.world().resource::<Focus>().screen, Screen::Devices), "asked once");
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

/// `@potluck/world` 0.2: a creation with no creatures runs on timers and
/// hears who comes and goes.
#[test]
fn timers_and_world_events_run_a_creation_with_no_creatures() {
    let mut voxels = world();
    let mut herd = Herd::default();
    let mut scripts = Scripts::default();
    let code = r#"
local world = require("@potluck/world@0.2")
local ticks, once = 0, 0
world.every(0.1, function() ticks += 1 end)
world.after(0.5, function() once += 1; print("once " .. once) end)
local gone = world.after(0.3, function() print("a cancelled timer ran") end)
assert(world.cancel(gone) == true and world.cancel(gone) == false)
world.after(2, function() print("ticks " .. ticks) end)
world.on("player_joined", function(p) print("hello " .. p.name) end)
world.on("player_left", function(p) print("bye " .. p.name) end)
print(pcall(world.on, "lunch", function() end) and "lunch heard" or "no lunch")
local many = 0
while pcall(world.after, 60, function() end) do many += 1 end
print("waiting " .. many)
"#;
    scripts.players = vec![PlayerInfo { id: 7, name: "Ada".into(), pos: Vec3::new(0.0, 40.0, 0.0) }];
    scripts.start(&creation(code), &mut herd).unwrap();
    run(&mut scripts, &mut voxels, &mut herd, 2.1);
    scripts.players.clear();
    run(&mut scripts, &mut voxels, &mut herd, 0.2);
    let vm = &scripts.vms["test"];
    assert_eq!(vm.error, None, "{:?}", vm.log);
    let log = vm.log.join("\n");
    assert!(log.contains("no lunch"), "world.on knows its events: {log}");
    // three timers were waiting when it filled up
    assert!(log.contains(&format!("waiting {}", crate::script::MAX_TIMERS - 3)), "{log}");
    assert_eq!(log.matches("hello Ada").count(), 1, "{log}");
    assert_eq!(log.matches("once").count(), 1, "world.after runs once: {log}");
    assert!(!log.contains("cancelled timer ran"), "{log}");
    let ticks: u32 = log.lines().find_map(|l| l.strip_prefix("ticks ")).and_then(|n| n.parse().ok()).expect("the two second timer");
    assert!((19..=21).contains(&ticks), "world.every(0.1) ran {ticks} times in two seconds");
    assert!(log.contains("bye Ada"), "{log}");
}

/// `@potluck/store`: what a creation keeps lasts across a reload and a
/// server's restart, stays its own, and has a quota.
#[test]
fn creations_keep_things_in_the_store() {
    let mut herd = Herd::default();
    let mut scripts = Scripts::default();
    let code = r#"
local store = require("@potluck/store@0.1")
local n = (store.get("starts") or 0) + 1
store.set("starts", n)
store.set("best", { name = "Ada", laps = { 31.5, 29.25 }, at = vector.create(1, 2, 3) })
store.set("gone", true)
store.set("gone", nil)
print("starts " .. n .. " keys " .. table.concat(store.keys(), ","))
print(pcall(store.set, "big", string.rep("x", 70000)) and "too big kept" or "full")
print(pcall(store.set, "f", function() end) and "kept a function" or "no functions")
local best = store.get("best")
print(best.name .. " " .. best.laps[2] .. " " .. best.at.y .. " " .. store.used())
"#;
    let c = creation(code);
    scripts.start(&c, &mut herd).unwrap();
    let log = scripts.vms["test"].log.join("\n");
    assert!(log.contains("starts 1 keys best,starts"), "{log}");
    assert!(log.contains("full") && log.contains("no functions"), "{log}");
    assert!(log.contains("Ada 29.25 2 "), "lists and vectors come back: {log}");
    // a reload keeps it
    scripts.start(&c, &mut herd).unwrap();
    assert!(scripts.vms["test"].log.join("\n").contains("starts 2"));
    // another creation can't see it
    let mut other = c.clone();
    other.id = "other".into();
    scripts.start(&other, &mut herd).unwrap();
    assert!(scripts.vms["other"].log.join("\n").contains("starts 1"), "each creation keeps its own");
    // a server saves it with the world and reads it when it starts again
    let file = std::env::temp_dir().join(format!("potluck-store-{}.json", std::process::id()));
    scripts.shared.lock().unwrap().store.save(&file);
    let mut again = Scripts::default();
    again.shared.lock().unwrap().store = crate::script::Store::load(&file);
    let _ = std::fs::remove_file(&file);
    again.start(&c, &mut herd).unwrap();
    assert!(again.vms["test"].log.join("\n").contains("starts 3"), "{:?}", again.vms["test"].log);
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
    // at night, with Ada in survival, so the Gloomlings come out too
    scripts.set_day(0.0);
    scripts.set_pockets(7, true, &[], 0);
    run(&mut scripts, &mut voxels, &mut herd, 2.0);
    let kinds: std::collections::BTreeSet<String> = herd.bodies.values().map(|b| b.kind.clone()).collect();
    for c in base.iter().filter(|c| c.id != "gloomlings") {
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
    assert!(scripts.out.scores.iter().any(|s| s.1 == "tags" && s.3 == 7 && s.4 >= 1.0), "a tag is scored: {:?}", scripts.out.scores);
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

#[test]
fn clouds_cover_part_of_the_sky() {
    let n = 512;
    let cover: Vec<f32> = (0..n * n).map(|i| crate::render::clouds::cover((i % n) as f32 / n as f32, (i / n) as f32 / n as f32)).collect();
    let cloudy = cover.iter().filter(|&&c| c > 0.5).count() as f32 / cover.len() as f32;
    if let Ok(dir) = std::env::var("POTLUCK_DUMP_TEX") {
        let px: Vec<u8> = cover.iter().map(|c| (c * 255.0) as u8).collect();
        let mut f = format!("P5 {n} {n} 255\n").into_bytes();
        f.extend(px);
        std::fs::write(std::path::Path::new(&dir).join("clouds.pgm"), f).unwrap();
    }
    assert!((0.2..0.55).contains(&cloudy), "a third or so of the sky is cloud ({cloudy})");
    // the pattern tiles
    let edge = (crate::render::clouds::cover(0.0, 0.3) - crate::render::clouds::cover(1.0, 0.3)).abs();
    assert!(edge < 1e-3, "the texture tiles ({edge})");
}

#[test]
fn creature_parts_wear_skins_by_name() {
    use crate::textures::skins::{paint, Skin, SIZE};
    assert_eq!(Skin::of("eye_left"), Skin::Eye);
    assert_eq!(Skin::of("shell_top"), Skin::Shell);
    assert_eq!(Skin::of("fleece"), Skin::Fluff);
    assert_eq!(Skin::of("jelly"), Skin::Smooth);
    assert_eq!(Skin::of("bill"), Skin::Horn);
    assert_eq!(Skin::of("hat_tip"), Skin::Cloth);
    assert_eq!(Skin::of("leg_back_left"), Skin::Fur);
    assert_eq!(Skin::of("face"), Skin::Bare);
    for skin in Skin::ALL {
        let s = paint(skin);
        let px = s.color.data.as_ref().unwrap();
        assert_eq!(px.len(), SIZE * SIZE * 4);
        if let Ok(dir) = std::env::var("POTLUCK_DUMP_TEX") {
            let mut f = format!("P5 {SIZE} {SIZE} 255\n").into_bytes();
            f.extend(px.chunks(4).map(|c| c[0]));
            std::fs::write(std::path::Path::new(&dir).join(format!("skin_{skin:?}.pgm")), f).unwrap();
        }
    }
    // a part's box: skins tile at one scale on every face, and it's darker underneath
    let m = crate::creatures::part_mesh(Vec3::new(1.0, 0.5, 2.0), Skin::Fur, true);
    let uv = m.attribute(Mesh::ATTRIBUTE_UV_0).unwrap();
    let bevy::mesh::VertexAttributeValues::Float32x2(uv) = uv else { panic!() };
    let widest = uv.iter().map(|u| u[0].max(u[1])).fold(0.0f32, f32::max);
    assert!((widest - 2.0 / Skin::Fur.tile()).abs() < 1e-4, "the long side carries four tiles ({widest})");
    let Some(bevy::mesh::VertexAttributeValues::Float32x4(col)) = m.attribute(Mesh::ATTRIBUTE_COLOR) else { panic!("no shading") };
    let pos = m.attribute(Mesh::ATTRIBUTE_POSITION).unwrap().as_float3().unwrap();
    let (top, bottom) = pos.iter().zip(col).fold((0.0f32, 1.0f32), |(t, b), (p, c)| if p[1] > 0.0 { (t.max(c[0]), b) } else { (t, b.min(c[0])) });
    assert!(top > bottom + 0.2, "lighter on top ({top}) than underneath ({bottom})");
    assert!(crate::creatures::part_mesh(Vec3::ONE, Skin::Fur, false).attribute(Mesh::ATTRIBUTE_COLOR).is_none(), "no colours for ray tracing");
}

#[test]
fn the_sky_seen_is_less_under_a_roof() {
    let w = world();
    // the villa's library, under its roof, against its open terrace
    let (inside, outside) = (crate::mesh::sky_seen(&w, Vec3::new(48.5, 27.6, 202.5)), crate::mesh::sky_seen(&w, Vec3::new(39.5, 27.6, 196.5)));
    assert!(inside < 0.25 && outside > 0.75, "library {inside}, terrace {outside}");
}

#[test]
fn people_wear_clothes_a_face_and_hair() {
    use crate::textures::skins::Skin;
    for hue in [0.0, 67.0, 134.0, 200.0, 335.0] {
        let parts = crate::creatures::person_parts(hue);
        let skins: Vec<Skin> = parts.iter().map(|p| Skin::of(&p.name)).collect();
        for want in [Skin::Cloth, Skin::Bare, Skin::Fur, Skin::Eye] {
            assert!(skins.contains(&want), "a person wears {want:?}");
        }
        // legs, arms and hands swing
        assert_eq!(parts.iter().filter(|p| p.leg).count(), 6);
        // stands on the ground, the eyes at eye height and on the face's front
        let low = parts.iter().map(|p| p.at.y - p.size.y / 2.0).fold(9.0f32, f32::min);
        assert!(low.abs() < 1e-3, "feet on the ground ({low})");
        let eye = parts.iter().find(|p| p.name == "eye").unwrap();
        assert!((eye.at.y - crate::player::EYE).abs() < 0.15);
        let face = parts.iter().find(|p| p.name == "face").unwrap();
        assert!(eye.at.z < face.at.z - face.size.z / 2.0 + 0.01, "eyes on the front, which is -z");
    }
}

/// Jams from the game: the Green's arches show what the market offers (an
/// entry in this week's jam first, then what's trending, and for younger
/// players only what's checked for all ages) until players bring their own;
/// Interact reaches the arch in front of you; and a creation shared to
/// Everyone goes in this week's jam from Share.
#[test]
fn the_greens_arches_and_this_weeks_jam() {
    use bevy::ecs::system::RunSystemOnce;
    use crate::creations::Mark;
    use crate::green::{self, Green, Shows};
    use crate::share::{Done, ShareJobs, Who};
    use crate::settings::Settings;
    use crate::ui::{menu_items, Act, Acts, Focus, Kind, Screen, WorldInfo};
    home();
    let run = crate::settings::random_hex(5);
    let (market, mkt) = start_market(None, &run);
    let who = Who { server: fake_game_server(), market, device: format!("ada-{run}"), name: "Ada".into(), young: false };
    let jobs = ShareJobs::default();
    jobs.account(&who);
    let Done::Account(_) = done(&jobs, "Ada's account") else { panic!("not an account") };

    // on the market: three of Ada's creations, the raft in this week's jam
    let codes: Vec<String> = (0..3).map(|_| potluck_share::codes::new_code()).collect();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
    {
        let db = mkt.db();
        for (i, name) in ["Raft", "Balloon", "Kite"].iter().enumerate() {
            db.execute(
                "INSERT INTO creations (code, owner, slug, name, kind, visibility, listing, plays, created, updated) VALUES (?1, ?2, ?3, ?4, 'creature', 'everyone', 'listed', ?5, ?6, ?6)",
                (&codes[i], format!("dev-ada-{run}"), name.to_lowercase(), name, 10 - i as i64, now),
            )
            .unwrap();
        }
        db.execute("INSERT INTO jams (slug, theme, starts, ends, made_by, created) VALUES ('things-that-float', 'Things that float', ?1, ?2, 'potluck', ?1)", (now - 60, now + 7 * 86400)).unwrap();
        db.execute("INSERT INTO jam_entries (jam, creation, entered) SELECT j.id, c.id, ?1 FROM jams j, creations c WHERE c.code = ?2", (now, &codes[0])).unwrap();
    }
    jobs.green(&who);
    let Done::Green { picks, jam } = done(&jobs, "the Green") else { panic!("not the Green") };
    let jam = jam.expect("the jam running now");
    assert_eq!(jam.theme, "Things that float");
    assert_eq!(picks.iter().map(|p| p.code.clone()).collect::<Vec<_>>(), codes, "the entry, then trending: {picks:?}");
    assert!(picks[0].why.contains("this week's jam") && picks[1].why.contains("Trending"), "{picks:?}");
    jobs.green(&Who { young: true, ..who.clone() });
    let Done::Green { picks: young, .. } = done(&jobs, "the Green for a younger player") else { panic!("not the Green") };
    assert!(young.is_empty(), "nothing here is checked for all ages: {young:?}");

    // the arches: the market's picks, until a player brings something
    let mut lib = Library::load();
    lib.running = lib.list.values().filter(|c| c.origin == Origin::Base).map(|c| c.id.clone()).collect();
    let mut green = Green { picks, ..Default::default() };
    let arches = green.arches(&lib);
    assert!(matches!(&arches[0], Some(Shows::Pick(p)) if p.name == "Raft") && arches[3].is_none(), "{arches:?}");
    let mut balloon = mossback();
    balloon.id = "balloon".into();
    balloon.manifest.name = "Balloon".into();
    balloon.origin = Origin::Code(codes[1].clone());
    balloon.code = Some(codes[1].clone());
    lib.list.insert("balloon".into(), balloon);
    lib.running.push("balloon".into());
    let arches = green.arches(&lib);
    assert_eq!(arches[0], Some(Shows::Here("balloon".into())), "what players brought comes first");
    let names: Vec<String> = arches[1..].iter().flatten().map(|s| green::describe(s, &lib).0.lines().next().unwrap().to_string()).collect();
    assert_eq!(names, ["Raft", "Kite"], "and the balloon isn't shown twice");

    // aiming: the arch in front of you, within reach, and not through a wall
    let front = green::arch_at(1) + Vec3::new(0.0, 0.0, 3.0);
    assert_eq!(green::aimed(front, Vec3::NEG_Z, f32::MAX), Some(1));
    assert_eq!(green::aimed(front, Vec3::Z, f32::MAX), None, "looking away");
    assert_eq!(green::aimed(front + Vec3::Z * 10.0, Vec3::NEG_Z, f32::MAX), None, "too far");
    assert_eq!(green::aimed(front, Vec3::NEG_Z, 1.0), None, "a block in the way");
    assert_eq!(green::aimed(front + Vec3::X * 5.0, Vec3::NEG_Z, f32::MAX), None, "between arches");

    // Interact at an arch showing something in this world looks inside it
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    crate::sim(&mut app, crate::SEED);
    app.insert_resource(Settings::default()).insert_resource(crate::player::Me::default());
    app.world_mut().resource_mut::<Library>().list.insert("balloon".into(), lib.list["balloon"].clone());
    app.world_mut().resource_mut::<Library>().running.push("balloon".into());
    app.world_mut().resource_mut::<Acts>().0.push(Act::Arch(0));
    app.world_mut().run_system_once(crate::game::do_acts).unwrap();
    assert!(app.world().resource::<Focus>().screen == Screen::Inside("balloon".into()), "looks inside the balloon");

    // Share offers the jam once it's on the market for Everyone
    let id = lib.remix("mossback", "Ada").unwrap();
    let scripts = Scripts::default();
    let info = WorldInfo::default();
    let settings = Settings { age_asked: true, ..Default::default() };
    let mut focus = Focus::default();
    focus.jam = Some(jam.clone());
    focus.go(Screen::Share(id.clone()));
    let acts = |focus: &Focus, lib: &Library, settings: &Settings| -> Vec<Act> {
        menu_items(focus, lib, &scripts, settings, &info).1.into_iter().filter_map(|i| if let Kind::Button(a) = i.kind { Some(a) } else { None }).collect()
    };
    assert!(!acts(&focus, &lib, &settings).contains(&Act::EnterJam(id.clone())), "not shared yet");
    lib.marks.insert(id.clone(), Mark { code: codes[2].clone(), version: 1, everyone: true });
    assert!(acts(&focus, &lib, &settings).contains(&Act::EnterJam(id.clone())));
    assert!(!acts(&focus, &lib, &Settings { young: true, ..settings.clone() }).contains(&Act::EnterJam(id.clone())), "younger players share by code only");
    focus.go(Screen::Pause);
    assert!(acts(&focus, &lib, &settings).contains(&Act::Market("/jams/things-that-float".into(), "Things that float".into())));

    // entering: the kite goes in; someone else's creation doesn't
    jobs.enter_jam(&who, &jam, &codes[2]);
    let Done::Entered { theme } = done(&jobs, "entering the kite") else { panic!("not entered") };
    assert_eq!(theme, "Things that float");
    let n: i64 = mkt.db().query_row("SELECT COUNT(*) FROM jam_entries", (), |r| r.get(0)).unwrap();
    assert_eq!(n, 2);
    // Bo, on his own device
    let bo = Who { device: format!("bo-{run}"), name: "Bo".into(), ..who.clone() };
    let bo_jobs = ShareJobs::default();
    bo_jobs.enter_jam(&bo, &jam, &codes[1]);
    assert!(failed(&bo_jobs, "Bo entering Ada's balloon").contains("only its maker"));
    green.picks.clear();
    assert!(green.arches(&lib)[1].is_none(), "with the market out of reach, only what players brought");
}

/// `@potluck/input`: a creation's actions are in its potluck.toml; while
/// they're live (riding its creature, or it has the player's controls) its
/// buttons do its actions instead of the game's, players can move them, and
/// the host only takes presses for what's live for that player.
#[test]
fn creations_have_buttons_of_their_own() {
    use bevy::ecs::system::RunSystemOnce;
    use crate::creations::ActionDef;
    use crate::game::{Action, Actions};
    use crate::input::{claim, Controls};
    use crate::net::{decode, frame_msg, Event, Msg};
    use crate::settings::{Binding, Settings};
    use crate::transport::Wire;
    use crate::ui::{menu_items, Act, Focus, Kind, Screen, WorldInfo};
    home();
    let mut voxels = world();
    let mut herd = Herd::default();
    let mut scripts = Scripts::default();
    let code = r#"
local world = require("@potluck/world@0.2")
local input = require("@potluck/input@0.1")
local pats = 0
input.on("pat", function(p)
	pats += 1
	print("pat from " .. p.name .. " " .. pats)
	input.focus(p)
end)
world.every(0.1, function()
	for _, p in world.players() do
		if input.held("spin", p) then print("spinning " .. tostring(input.focused(p))) end
	end
end)
print(pcall(input.on, "fly", function() end) and "fly ok" or "no fly")
"#;
    let mut c = creation(code);
    c.manifest.actions.insert("pat".into(), ActionDef { name: "Pat".into(), pad: "X".into(), keys: "F".into() });
    c.manifest.actions.insert("spin".into(), ActionDef { name: "Spin".into(), pad: "RB".into(), keys: "R".into() });
    scripts.players = vec![PlayerInfo { id: 7, name: "Ada".into(), pos: Vec3::new(0.0, 40.0, 0.0) }];
    scripts.start(&c, &mut herd).unwrap();
    run(&mut scripts, &mut voxels, &mut herd, 0.2);
    scripts.press(7, "test", "pat", true);
    scripts.press(7, "test", "pat", true);
    scripts.press(7, "test", "fly", true);
    run(&mut scripts, &mut voxels, &mut herd, 0.2);
    assert_eq!(scripts.focus_changes(), vec![(7, Some("test".to_string()))], "input.focus took Ada's controls");
    assert!(scripts.focus_changes().is_empty(), "told once");
    scripts.press(7, "test", "pat", false);
    scripts.press(7, "test", "pat", true);
    scripts.press(7, "test", "spin", true);
    run(&mut scripts, &mut voxels, &mut herd, 0.2);
    let vm = &scripts.vms["test"];
    assert_eq!(vm.error, None, "{:?}", vm.log);
    let log = vm.log.join("\n");
    assert!(log.contains("no fly"), "only actions it declared: {log}");
    assert!(log.contains("pat from Ada 1") && log.contains("pat from Ada 2") && !log.contains("pat from Ada 3"), "a press, not a hold: {log}");
    assert!(log.contains("spinning true"), "held while held: {log}");
    // Ada goes, and nothing of hers is held
    scripts.players.clear();
    run(&mut scripts, &mut voxels, &mut herd, 0.2);
    assert_eq!(scripts.focus_of(7), None);
    assert!(scripts.shared.lock().unwrap().input.held.is_empty());
    // a creation that stops gives controls back
    scripts.shared.lock().unwrap().input.focus.insert(9, "test".into());
    scripts.stop("test", &mut herd);
    assert_eq!(scripts.focus_changes(), vec![(9, None)]);

    // on the device: its buttons do its actions, not the game's
    let mut lib = Library::load();
    lib.list.insert("test".into(), c.clone());
    let mut settings = Settings::default();
    let pressing = |held: &str| {
        let mut c = Controls::default();
        (c.interact, c.share, c.hotbar_step) = (true, true, 1);
        c.held = vec![held.to_string()];
        c
    };
    let mut ctl = pressing("pad:X");
    claim(&mut ctl, Some("test".into()), &lib, &settings);
    assert_eq!(ctl.actions, vec!["pat".to_string()]);
    assert!(!ctl.interact && ctl.share, "X pats instead of getting off; sharing stays");
    settings.bindings.insert("test/pat".into(), Binding { pad: "LB".into(), keys: "G".into() });
    let mut ctl = pressing("pad:X");
    claim(&mut ctl, Some("test".into()), &lib, &settings);
    assert!(ctl.actions.is_empty() && ctl.interact, "moved to LB and G, so X and F are the game's again");
    let mut ctl = pressing("pad:LB");
    claim(&mut ctl, Some("test".into()), &lib, &settings);
    assert_eq!((ctl.actions.clone(), ctl.hotbar_step), (vec!["pat".to_string()], 0));
    let mut ctl = pressing("pad:X");
    claim(&mut ctl, None, &lib, &settings);
    assert!(ctl.actions.is_empty(), "nothing's live");

    // what goes between devices
    let press = Action::Press { creation: "test".into(), action: "pat".into(), down: true };
    for m in [Msg::Act(press.clone()), Msg::Focus(Some("test".into()))] {
        let bytes = frame_msg(&m);
        let Some(Event::Msg(_, back)) = decode(Wire::Data(1, bytes)) else { panic!("didn't decode") };
        assert_eq!(serde_json::to_string(&back).unwrap(), serde_json::to_string(&m).unwrap());
    }

    // the host takes a press only while it's live for that player
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    crate::sim(&mut app, crate::SEED);
    app.insert_resource(Settings::default()).insert_resource(crate::player::Me::default());
    app.world_mut().resource_mut::<Library>().list.insert("test".into(), c.clone());
    app.world_mut().resource_scope(|w, mut s: Mut<Scripts>| {
        let mut herd = w.resource_mut::<Herd>();
        s.start(&c, &mut herd).unwrap();
    });
    let me = app.world().resource::<crate::net::Net>().my_id;
    let held = |app: &App| app.world().resource::<Scripts>().shared.lock().unwrap().input.held.contains(&(me, "test".to_string(), "pat".to_string()));
    app.world_mut().resource_mut::<Actions>().0.push((me, press.clone()));
    app.world_mut().run_system_once(crate::game::host_step).unwrap();
    assert!(!held(&app), "not riding it, and it doesn't have my controls");
    {
        let s = app.world().resource::<Scripts>();
        let mut sh = s.shared.lock().unwrap();
        sh.input.focus.insert(me, "test".into());
        sh.input.changed.push(me);
    }
    app.world_mut().resource_mut::<Actions>().0.push((me, press.clone()));
    app.world_mut().run_system_once(crate::game::host_step).unwrap();
    assert!(held(&app), "it has my controls");
    assert_eq!(app.world().resource::<crate::player::Me>().focus.as_deref(), Some("test"), "and the host's own player hears so");

    // moving a button: the screen lists it, and the next press takes it
    let scripts = Scripts::default();
    let info = WorldInfo::default();
    let mut focus = Focus::default();
    focus.go(Screen::Controls);
    let buttons: Vec<Act> = menu_items(&focus, &lib, &scripts, &Settings::default(), &info).1.into_iter().filter_map(|i| if let Kind::Button(a) = i.kind { Some(a) } else { None }).collect();
    assert!(buttons.contains(&Act::Rebind("test".into(), "spin".into())), "{buttons:?}");
    app.world_mut().resource_mut::<Library>().list.insert("test".into(), c.clone());
    app.world_mut().resource_mut::<Focus>().screen = Screen::Controls;
    app.world_mut().resource_mut::<Focus>().rebinding = Some(("test".into(), "spin".into()));
    let mut ctl = Controls::default();
    ctl.just = vec!["pad:Y".into()];
    app.insert_resource(ctl);
    app.world_mut().run_system_once(crate::ui::menu_input).unwrap();
    let s = app.world().resource::<Settings>();
    assert_eq!(s.bindings.get("test/spin"), Some(&Binding { pad: "Y".into(), keys: "R".into() }), "the pad moved, the key stayed");
    assert!(app.world().resource::<Focus>().rebinding.is_none());
}

#[test]
fn creations_show_lines_and_ask_questions() {
    use bevy::ecs::system::RunSystemOnce;
    use crate::game::{Action, Actions};
    use crate::input::Controls;
    use crate::net::{decode, frame_msg, Event, Msg};
    use crate::script::{Panel, Piece, UiChange};
    use crate::settings::Settings;
    use crate::transport::Wire;
    use crate::ui::{menu_items, Act, Acts, Focus, Kind, Screen, WorldInfo};
    home();
    let mut voxels = world();
    let mut herd = Herd::default();
    let mut scripts = Scripts::default();
    let code = r#"
local world = require("@potluck/world@0.2")
local ui = require("@potluck/ui@0.1")
ui.show(nil, "round", "Round 1", { at = "left" })
world.on("player_joined", function(p)
	ui.show(p, "you", "Hello   " .. p.name .. "\nthere")
	ui.ask(p, { title = "Play?", text = "Want a round?", choices = { "Yes", "No" } }, function(who, choice)
		print("answer " .. who.name .. " " .. tostring(choice))
		if choice == 1 then ui.hide(who, "you") end
		if choice == nil then
			ui.ask(who, { text = "Sure?" }, function(_, c) print("again " .. tostring(c)) end)
		end
	end)
end)
world.after(1, function()
	for _, p in world.players() do
		ui.ask(p, { text = "One" }, function(who, c) print("one " .. who.name .. " " .. tostring(c)) end)
		ui.ask(p, { text = "Two" }, function(who, c) print("two " .. who.name .. " " .. tostring(c)) end)
	end
end)
print(pcall(ui.show, nil, "x", "y", { at = "middle" }) and "middle ok" or "no middle")
print(pcall(ui.ask, 1, { choices = { "a", "b", "c", "d", "e" } }, function() end) and "five ok" or "no five")
for i = 1, 7 do ui.show(nil, "n" .. i, "line " .. i) end
print(pcall(ui.show, nil, "one too many", "line") and "nine ok" or "no nine")
ui.show(nil, "n1", "line 1 again")
for i = 1, 7 do ui.hide(nil, "n" .. i) end
"#;
    let ada = PlayerInfo { id: 7, name: "Ada".into(), pos: Vec3::new(0.0, 40.0, 0.0) };
    let bo = PlayerInfo { id: 8, name: "Bo".into(), pos: Vec3::new(2.0, 40.0, 0.0) };
    scripts.players = vec![ada.clone()];
    scripts.start(&creation(code), &mut herd).unwrap();
    run(&mut scripts, &mut voxels, &mut herd, 0.2);
    let log = |s: &Scripts| s.vms["test"].log.join("\n");
    assert_eq!(scripts.vms["test"].error, None, "{}", log(&scripts));
    assert!(log(&scripts).contains("no middle") && log(&scripts).contains("no five") && log(&scripts).contains("no nine"), "{}", log(&scripts));
    let piece = |text: &str, at: &str| Some(Piece { text: text.into(), at: at.into() });
    let shown = |ch: &[(u32, UiChange)], pid: u32, id: &str| {
        ch.iter().find_map(|(p, c)| match c {
            UiChange::Piece { id: i, piece, .. } if *p == pid && i == id => Some(piece.clone()),
            _ => None,
        })
    };
    let panel = |ch: &[(u32, UiChange)], pid: u32| ch.iter().find_map(|(p, c)| if let (true, UiChange::Panel(x)) = (*p == pid, c) { Some(x.clone()) } else { None });
    let ch = scripts.ui_changes(&[7]);
    assert_eq!(shown(&ch, 7, "round"), Some(piece("Round 1", "left")), "everyone's line: {ch:?}");
    assert_eq!(shown(&ch, 7, "you"), Some(piece("Hello Ada there", "right")), "one line, spaces tidied");
    assert_eq!(shown(&ch, 7, "n1"), Some(None), "shown and hidden again");
    let ada_ask = panel(&ch, 7).flatten().expect("Ada is asked");
    assert_eq!((ada_ask.title.as_str(), ada_ask.text.as_str(), ada_ask.choices.clone()), ("Play?", "Want a round?", vec!["Yes".to_string(), "No".to_string()]));
    assert!(scripts.ui_changes(&[7]).is_empty(), "told once");

    // Bo arrives, and sees everyone's line and his own
    scripts.players = vec![ada.clone(), bo.clone()];
    run(&mut scripts, &mut voxels, &mut herd, 0.2);
    let ch = scripts.ui_changes(&[7, 8]);
    assert_eq!(shown(&ch, 8, "round"), Some(piece("Round 1", "left")), "{ch:?}");
    assert_eq!(shown(&ch, 8, "you"), Some(piece("Hello Bo there", "right")));
    assert!(shown(&ch, 7, "you").is_none(), "Ada's line didn't change");
    let bo_ask = panel(&ch, 8).flatten().expect("Bo is asked");

    // Ada says yes; Bo closes it, and isn't asked again for a while
    scripts.answer(7, ada_ask.ask, Some(0));
    scripts.answer(8, bo_ask.ask, None);
    scripts.answer(8, bo_ask.ask, Some(0));
    run(&mut scripts, &mut voxels, &mut herd, 0.2);
    let l = log(&scripts);
    assert!(l.contains("answer Ada 1") && l.contains("answer Bo nil"), "{l}");
    assert_eq!(l.matches("answer Bo").count(), 1, "one answer to a question");
    let ch = scripts.ui_changes(&[7, 8]);
    assert_eq!(shown(&ch, 7, "you"), Some(None), "Ada's line hidden");
    assert_eq!(panel(&ch, 7), Some(None), "her panel's gone");
    assert_eq!(panel(&ch, 8), Some(None), "Bo's too");
    run(&mut scripts, &mut voxels, &mut herd, 0.2);
    assert!(log(&scripts).contains("again nil"), "asked again too soon: nil at once");
    assert!(panel(&scripts.ui_changes(&[7, 8]), 8).is_none(), "and nothing on his screen");

    // a second question from the same creation takes the first one's place
    run(&mut scripts, &mut voxels, &mut herd, 0.6);
    let l = log(&scripts);
    assert!(l.contains("one Ada nil") && !l.contains("two Ada"), "{l}");
    assert!(l.contains("one Bo nil") && l.contains("two Bo nil"), "Bo is still left be: {l}");
    let ch = scripts.ui_changes(&[7, 8]);
    assert_eq!(panel(&ch, 7).flatten().map(|p| p.text), Some("Two".to_string()));

    // Ada goes; then the creation stops, and everyone's lines go with it
    scripts.players = vec![bo.clone()];
    run(&mut scripts, &mut voxels, &mut herd, 0.2);
    assert!(scripts.shared.lock().unwrap().ui.panels.get(&7).is_none(), "nothing kept for Ada");
    scripts.stop("test", &mut herd);
    let ch = scripts.ui_changes(&[8]);
    assert_eq!(shown(&ch, 8, "round"), Some(None), "{ch:?}");
    assert_eq!(shown(&ch, 8, "you"), Some(None));

    // what goes between devices
    let p = Panel { creation: "test".into(), ask: 3, title: "Play?".into(), text: "Want a round?".into(), choices: vec!["Yes".into()] };
    for m in [Msg::Panel(Some(p.clone())), Msg::Piece { creation: "test".into(), id: "you".into(), piece: piece("Hi", "bottom") }, Msg::Act(Action::Answer { ask: 3, choice: Some(0) })] {
        let Some(Event::Msg(_, back)) = decode(Wire::Data(1, frame_msg(&m))) else { panic!("didn't decode") };
        assert_eq!(serde_json::to_string(&back).unwrap(), serde_json::to_string(&m).unwrap());
    }

    // on the host's own screen: it opens while playing, says whose it is, and the answer goes back
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    crate::sim(&mut app, crate::SEED);
    app.insert_resource(Settings::default()).insert_resource(crate::player::Me::default()).insert_resource(Controls::default());
    let c = creation(code);
    app.world_mut().resource_mut::<Library>().list.insert("test".into(), c.clone());
    app.world_mut().resource_scope(|w, mut s: Mut<Scripts>| {
        let mut herd = w.resource_mut::<Herd>();
        s.start(&c, &mut herd).unwrap();
    });
    let tick = |app: &mut App| {
        app.world_mut().resource_scope(|w, mut s: Mut<Scripts>| {
            w.resource_scope(|w, mut v: Mut<Voxels>| {
                let mut h = w.resource_mut::<Herd>();
                s.tick(&mut v, &mut h, 0.2);
            })
        })
    };
    app.world_mut().run_system_once(crate::game::host_step).unwrap();
    tick(&mut app);
    app.world_mut().run_system_once(crate::game::host_step).unwrap();
    {
        let me = app.world().resource::<crate::player::Me>();
        assert_eq!(me.panel.as_ref().map(|p| p.title.as_str()), Some("Play?"));
        assert!(me.pieces.values().any(|p| p.text == "Round 1"), "{:?}", me.pieces);
    }
    app.world_mut().resource_mut::<Focus>().go(Screen::Pause);
    app.world_mut().run_system_once(crate::game::interact).unwrap();
    assert_eq!(app.world().resource::<Focus>().screen, Screen::Pause, "it waits while a menu's open");
    app.world_mut().resource_mut::<Focus>().go(Screen::Playing);
    app.world_mut().run_system_once(crate::game::interact).unwrap();
    assert_eq!(app.world().resource::<Focus>().screen, Screen::Panel);
    let (title, items) = {
        let w = app.world();
        menu_items(w.resource::<Focus>(), w.resource::<Library>(), w.resource::<Scripts>(), w.resource::<Settings>(), &WorldInfo::default())
    };
    assert_eq!(title, "Play?");
    assert_eq!(items[0].label, "From Test by Tests", "always says whose question it is");
    let acts: Vec<Act> = items.iter().filter_map(|i| if let Kind::Button(a) = &i.kind { Some(a.clone()) } else { None }).collect();
    assert_eq!(acts, vec![Act::Answer(Some(0)), Act::Answer(Some(1)), Act::Answer(None)], "its choices, and Close");
    app.world_mut().resource_mut::<Acts>().0.push(Act::Answer(Some(1)));
    app.world_mut().run_system_once(crate::game::do_acts).unwrap();
    assert_eq!(app.world().resource::<Focus>().screen, Screen::Playing);
    assert!(matches!(app.world().resource::<Actions>().0.as_slice(), [(_, Action::Answer { choice: Some(1), .. })]));
    app.world_mut().run_system_once(crate::game::host_step).unwrap();
    tick(&mut app);
    assert!(app.world().resource::<Scripts>().vms["test"].log.iter().any(|l| l.starts_with("answer ") && l.ends_with(" 2")), "{:?}", app.world().resource::<Scripts>().vms["test"].log);
}

#[test]
fn sounds_are_made_the_same_every_time_and_creations_play_them() {
    use crate::net::{decode, frame_msg, Event, Msg};
    use crate::sound::{material, synth, Material, Play, SOUNDS};
    use crate::transport::Wire;
    home();
    // every sound is there, short, never clipping, and the same each time
    for (name, _) in SOUNDS {
        let v = synth(name);
        assert!(!v.is_empty() && v.len() < crate::sound::RATE as usize * 2, "{name}: {} samples", v.len());
        let peak = v.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        let rms = (v.iter().map(|s| s * s).sum::<f32>() / v.len() as f32).sqrt();
        assert!(peak > 0.2 && peak <= 0.8001 && rms <= 0.2501 && v.iter().all(|s| s.is_finite()), "{name} peaks at {peak}, rms {rms}");
        assert_eq!(v, synth(name), "{name} is made the same every time");
    }
    // what blocks sound like, from their names
    let by = |n: &str| material(crate::blocks::by_name(n).unwrap());
    assert_eq!((by("Grass"), by("Stone"), by("Sandstone"), by("Oak log"), by("Oak leaves"), by("Tall grass"), by("Glass"), by("Water")), (Material::Soft, Material::Hard, Material::Hard, Material::Wood, Material::Plant, Material::Plant, Material::Glass, Material::Water));

    // a creation plays them, for those near enough or for one player
    let mut voxels = world();
    let mut herd = Herd::default();
    let mut scripts = Scripts::default();
    let code = r#"
local world = require("@potluck/world@0.2")
local sound = require("@potluck/sound@0.1")
world.on("player_joined", function(p)
	sound.play("chime", { at = vector.create(0, 40, 0), caption = "A   bell" })
	sound.play_for(p, "ding", { volume = 3, pitch = 9 })
	sound.play("fanfare")
end)
print(pcall(sound.play, "trumpet") and "trumpet ok" or "no trumpet")
print(#sound.names())
world.after(0.5, function()
	local played = 0
	for _ = 1, 30 do
		if sound.play("pop") then played += 1 end
	end
	print("played " .. played)
end)
"#;
    let ada = PlayerInfo { id: 7, name: "Ada".into(), pos: Vec3::new(1.0, 40.0, 0.0) };
    let bo = PlayerInfo { id: 8, name: "Bo".into(), pos: Vec3::new(200.0, 40.0, 0.0) };
    scripts.players = vec![ada, bo];
    scripts.start(&creation(code), &mut herd).unwrap();
    run(&mut scripts, &mut voxels, &mut herd, 0.2);
    let vm = &scripts.vms["test"];
    assert_eq!(vm.error, None, "{:?}", vm.log);
    let log = vm.log.join("\n");
    assert!(log.contains("no trumpet") && log.contains(&SOUNDS.len().to_string()), "{log}");
    let heard = scripts.sound_changes();
    let who = |name: &str| -> Vec<u32> { heard.iter().filter(|(_, p)| p.name == name).map(|(id, _)| *id).collect() };
    assert_eq!(who("chime"), vec![7, 7], "only Ada is near the bell, and she hears both creations' joins: {heard:?}");
    let ding = heard.iter().find(|(_, p)| p.name == "ding").map(|(_, p)| p.clone()).unwrap();
    assert_eq!((ding.volume, ding.pitch), (1.0, 2.0), "kept in range");
    assert_eq!(heard.iter().find(|(_, p)| p.name == "chime").unwrap().1.caption.as_deref(), Some("A bell"));
    assert_eq!(who("fanfare"), vec![7, 8, 7, 8], "nowhere in particular: everyone");
    assert!(scripts.sound_changes().is_empty(), "played once");
    // too many at once: the rest are skipped, not an error
    run(&mut scripts, &mut voxels, &mut herd, 0.6);
    let log = scripts.vms["test"].log.join("\n");
    let played: usize = log.lines().find_map(|l| l.strip_prefix("played ")).and_then(|n| n.parse().ok()).unwrap_or(0);
    assert!((5..=10).contains(&played), "at most a second's worth: {log}");

    // what goes between devices
    let m = Msg::Sound(Play { name: "quack".into(), at: Some([1.0, 2.0, 3.0]), volume: 0.5, pitch: 1.2, caption: None });
    let Some(Event::Msg(_, back)) = decode(Wire::Data(1, frame_msg(&m))) else { panic!("didn't decode") };
    assert_eq!(serde_json::to_string(&back).unwrap(), serde_json::to_string(&m).unwrap());
}

/// Writes every sound as a WAV to listen to, and all of them in a row:
/// `POTLUCK_SOUNDS=DIR cargo test --workspace -- --ignored write_the_sounds`.
#[test]
#[ignore]
fn write_the_sounds() {
    let dir = std::path::PathBuf::from(std::env::var("POTLUCK_SOUNDS").expect("POTLUCK_SOUNDS=DIR"));
    std::fs::create_dir_all(&dir).unwrap();
    let wav = |samples: &[f32]| {
        let mut b = Vec::with_capacity(44 + samples.len() * 2);
        let rate = crate::sound::RATE;
        let data = samples.len() as u32 * 2;
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + data).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&rate.to_le_bytes());
        b.extend_from_slice(&(rate * 2).to_le_bytes());
        b.extend_from_slice(&2u16.to_le_bytes());
        b.extend_from_slice(&16u16.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&data.to_le_bytes());
        for s in samples {
            b.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
        }
        b
    };
    let mut all = vec![];
    for (name, _) in crate::sound::SOUNDS {
        let v = crate::sound::synth(name);
        std::fs::write(dir.join(format!("{name}.wav")), wav(&v)).unwrap();
        all.extend_from_slice(&v);
        all.extend(std::iter::repeat_n(0.0, crate::sound::RATE as usize / 3));
    }
    std::fs::write(dir.join("all.wav"), wav(&all)).unwrap();
}

#[test]
fn clouds_keep_their_mesh_to_recolour() {
    // `clouds::drift` recolours the disc as the sun moves, long after it has
    // gone to the GPU, so its data has to stay in the main world too
    let disc = crate::render::clouds::disc();
    assert!(disc.asset_usage.contains(bevy::asset::RenderAssetUsages::MAIN_WORLD));
    assert!(disc.attribute(Mesh::ATTRIBUTE_COLOR).is_some());
}

#[test]
fn lows_sky_meets_the_fog_and_burns_toward_a_low_sun() {
    use crate::render::{dome, fog_colour, sun_now, Clock};
    // recoloured as the sun moves, like the clouds
    assert!(dome::mesh().asset_usage.contains(bevy::asset::RenderAssetUsages::MAIN_WORLD));
    let (sun, day, low) = sun_now(&Clock { time: 0.735, running: false });
    let flat = Vec3::new(sun.x, 0.0, sun.z).normalize();
    // at the horizon away from the sun it's the fog the far hills fade into
    let away = dome::colour(-flat, sun, day, low);
    assert!((away - fog_colour(day, low)).length() < 0.02, "{away} against the fog's {}", fog_colour(day, low));
    // just over the horizon toward the sun it's gold, overhead it's blue
    let toward = dome::colour((flat + Vec3::Y * 0.08).normalize(), sun, day, low);
    // under the horizon it's the fog's colour all round, as the far sea is,
    // so where the sea stops at the dome there's no line
    assert!((dome::colour((flat - Vec3::Y * 0.03).normalize(), sun, day, low) - fog_colour(day, low)).length() < 0.02);
    assert!(toward.x > away.x + 0.15 && toward.x > toward.z + 0.3, "sunset {toward}");
    let up = dome::colour(Vec3::Y, sun, day, low);
    assert!(up.z > up.x + 0.2, "overhead {up}");
    // and at night it's dark everywhere
    let (sun, day, low) = sun_now(&Clock { time: 0.95, running: false });
    for d in [Vec3::Y, Vec3::X, -Vec3::X, Vec3::Z] {
        assert!(dome::colour(d, sun, day, low).max_element() < 0.12);
    }
}

#[test]
fn a_creation_talks_to_its_client_half_on_each_players_game() {
    use bevy::ecs::system::RunSystemOnce;
    use crate::game::{Action, Actions};
    use crate::halves::Halves;
    use crate::net::{decode, frame_msg, Event, Msg};
    use crate::script::{Note, Piece};
    use crate::settings::Settings;
    use crate::ui::WorldInfo;
    home();
    let mut voxels = world();
    let mut herd = Herd::default();
    let mut scripts = Scripts::default();
    let main = r#"
local net = require("@potluck/net@0.1")
net.on("hello", function(p, v)
	print("hello from " .. p.name .. " " .. tostring(v.n))
	net.send(p, "hi", { n = v.n + 1, at = vector.create(1, 2, 3), list = { "a", "b" } })
end)
net.on("spam", function(p) print("spam " .. p.name) end)
print(pcall(net.send, nil, "", 1) and "empty ok" or "no empty topic")
print(pcall(net.send, nil, "big", string.rep("x", 5000)) and "big ok" or "no big")
print(pcall(net.send, nil, "fn", function() end) and "fn ok" or "no fn")
print(pcall(net.me) and "me ok" or "no me")
local sent = 0
for i = 1, 40 do
	if net.send(nil, "all", i) then sent += 1 end
end
print("sent " .. sent)
"#;
    let client = r#"
local net = require("@potluck/net@0.1")
local ui = require("@potluck/ui@0.1")
local sound = require("@potluck/sound@0.1")
local world = require("@potluck/world@0.2")
local me = net.me()
print("I am " .. me.name)
print(pcall(world.set_block, vector.create(0, 0, 0), "stone") and "set ok" or "no set_block")
print(pcall(require, "@potluck/store@0.1") and "store ok" or "no store")
print(pcall(ui.show, 99, "x", "y") and "others ok" or "not for others")
print(pcall(ui.ask, me, { text = "?" }, function() end) and "ask ok" or "no ask")
net.on("hi", function(v)
	ui.show(nil, "hi", "hi " .. v.n .. " " .. v.list[2] .. " " .. v.at.y, { at = "bottom" })
	sound.play("ding", { at = v.at })
end)
local got = 0
net.on("all", function() got += 1 end)
net.on("boom", function() error("oops") end)
world.every(0.5, function() ui.show(me, "got", "got " .. got .. " at " .. string.format("%.2f", world.time())) end)
net.send("hello", { n = 1 })
for i = 1, 15 do net.send("spam") end
"#;
    let mut c = creation(main);
    c.files.insert("client.luau".into(), client.into());
    c.manifest.client = Some("client.luau".into());
    let ada = PlayerInfo { id: 7, name: "Ada".into(), pos: Vec3::new(0.0, 40.0, 0.0) };
    let bo = PlayerInfo { id: 8, name: "Bo".into(), pos: Vec3::new(2.0, 40.0, 0.0) };
    scripts.players = vec![ada.clone(), bo.clone()];
    scripts.start(&c, &mut herd).unwrap();
    let log = |s: &Scripts| s.vms["test"].log.join("\n");
    for want in ["no empty topic", "no big", "no fn", "no me", "sent 30"] {
        assert!(log(&scripts).contains(want), "{want}: {}", log(&scripts));
    }

    // each player's game runs its own half, which says hello
    let mut lib = Library::default();
    lib.list.insert("test".into(), c.clone());
    lib.running.push("test".into());
    lib.world = 1;
    let mut halves = [Halves::default(), Halves::default()];
    for (h, p) in halves.iter_mut().zip([&ada, &bo]) {
        h.sync(Some(&lib), p);
        assert_eq!(h.list["test"].error, None);
    }
    let out = halves[0].take();
    let l = halves[0].list["test"].log.join("\n");
    for want in ["I am Ada", "no set_block", "no store", "not for others", "no ask"] {
        assert!(l.contains(want), "{want}: {l}");
    }
    assert_eq!(out.notes.len(), 10, "ten a second from a half");
    assert_eq!(out.notes[0], Note { creation: "test".into(), topic: "hello".into(), value: serde_json::json!({ "n": 1 }) });
    for n in out.notes {
        scripts.hear(7, n);
    }
    // the host holds each player to the same: an eleventh is dropped
    scripts.hear(7, Note { creation: "test".into(), topic: "spam".into(), value: serde_json::Value::Null });
    for n in halves[1].take().notes.into_iter().take(1) {
        scripts.hear(8, n);
    }
    scripts.hear(8, Note { creation: "nobody".into(), topic: "hello".into(), value: serde_json::Value::Null });
    run(&mut scripts, &mut voxels, &mut herd, 0.2);
    let l = log(&scripts);
    assert!(l.contains("hello from Ada 1") && l.contains("hello from Bo 1"), "{l}");
    assert_eq!(l.matches("spam Ada").count(), 9, "{l}");

    // what the creation sent goes to whoever it was for
    let notes = scripts.net_changes(&[7, 8]);
    let for_ada: Vec<Note> = notes.iter().filter(|(p, _)| *p == 7).map(|(_, n)| n.clone()).collect();
    assert_eq!(for_ada.iter().filter(|n| n.topic == "all").count(), 30);
    assert_eq!(for_ada.iter().filter(|n| n.topic == "hi").count(), 1);
    assert_eq!(notes.iter().filter(|(p, _)| *p == 8).count(), 31);
    assert!(scripts.net_changes(&[7, 8]).is_empty(), "sent once");
    halves[0].frame(0.1, &ada, for_ada);
    halves[0].frame(0.45, &ada, vec![]);
    let out = halves[0].take();
    let piece = |text: &str, at: &str| Some(Piece { text: text.into(), at: at.into() });
    assert!(out.pieces.contains(&("test".into(), "hi".into(), piece("hi 2 b 2", "bottom"))), "{:?}", out.pieces);
    assert!(out.pieces.contains(&("test".into(), "got".into(), piece("got 30 at 0.55", "right"))), "its own clock: {:?}", out.pieces);
    assert_eq!(out.sounds.len(), 1);
    assert_eq!((out.sounds[0].name.as_str(), out.sounds[0].at), ("ding", Some([1.0, 2.0, 3.0])));

    // a new version starts it afresh, and takes its lines off the screen
    let mut c2 = c.clone();
    c2.files.insert("client.luau".into(), format!("{client}\nprint(\"again\")"));
    lib.list.insert("test".into(), c2.clone());
    halves[0].sync(Some(&lib), &ada);
    let out = halves[0].take();
    assert!(out.pieces.contains(&("test".into(), "hi".into(), None)) && out.pieces.contains(&("test".into(), "got".into(), None)), "{:?}", out.pieces);
    assert!(halves[0].list["test"].log.join("\n").contains("again"));
    // so does a new world
    lib.world = 2;
    halves[0].sync(Some(&lib), &ada);
    assert_eq!(halves[0].take().notes.len(), 10, "a new half says hello again");

    // a half that fails stops and says why; one that never stops is stopped
    halves[0].frame(0.1, &ada, vec![Note { creation: "test".into(), topic: "boom".into(), value: serde_json::Value::Null }]);
    assert!(halves[0].list["test"].error.as_deref().unwrap_or("").contains("oops"), "{:?}", halves[0].list["test"].error);
    let mut c3 = c.clone();
    c3.files.insert("client.luau".into(), "while true do end".into());
    lib.list.insert("test".into(), c3);
    halves[0].sync(Some(&lib), &ada);
    assert!(halves[0].list["test"].error.as_deref().unwrap_or("").contains("too long"));
    // and leaving the world stops them all
    halves[1].sync(None, &bo);
    assert!(halves[1].list.is_empty());

    // what goes between devices
    let n = Note { creation: "test".into(), topic: "hi".into(), value: serde_json::json!({ "n": 2, "at": { "__vector": [1.0, 2.0, 3.0] } }) };
    for m in [Msg::Net(n.clone()), Msg::Act(Action::Net(n.clone()))] {
        let Some(Event::Msg(_, back)) = decode(Wire::Data(1, frame_msg(&m))) else { panic!("didn't decode") };
        assert_eq!(serde_json::to_string(&back).unwrap(), serde_json::to_string(&m).unwrap());
    }

    // on the host's own game: the half's hello reaches the creation, and its answer the screen
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    crate::sim(&mut app, crate::SEED);
    app.insert_resource(Settings { name: "Cy".into(), ..default() }).insert_resource(crate::player::Me::default());
    app.world_mut().resource_mut::<WorldInfo>().in_world = true;
    {
        let mut lib = app.world_mut().resource_mut::<Library>();
        lib.list.insert("test".into(), c.clone());
        lib.running = vec!["test".into()];
    }
    app.world_mut().resource_scope(|w, mut s: Mut<Scripts>| {
        let mut herd = w.resource_mut::<Herd>();
        s.start(&c, &mut herd).unwrap();
    });
    app.world_mut().run_system_once(crate::halves::run).unwrap();
    assert!(app.world().resource::<Actions>().0.iter().any(|(_, a)| matches!(a, Action::Net(n) if n.topic == "hello")));
    app.world_mut().run_system_once(crate::game::host_step).unwrap();
    app.world_mut().resource_scope(|w, mut s: Mut<Scripts>| {
        w.resource_scope(|w, mut v: Mut<Voxels>| {
            let mut h = w.resource_mut::<Herd>();
            s.tick(&mut v, &mut h, 0.2);
        })
    });
    assert!(app.world().resource::<Scripts>().vms["test"].log.join("\n").contains("hello from Cy 1"));
    app.world_mut().run_system_once(crate::game::host_step).unwrap();
    app.world_mut().run_system_once(crate::halves::run).unwrap();
    let me = app.world().resource::<crate::player::Me>();
    assert_eq!(me.pieces.get(&("test".to_string(), "hi".to_string())).map(|p| p.text.as_str()), Some("hi 2 b 2"), "{:?}", me.pieces);
}

#[test]
fn the_rodeo_rams_clock_runs_on_the_riders_game() {
    use crate::halves::Halves;
    home();
    let lib = Library::load();
    let ram = lib.list["rodeo-ram"].clone();
    assert_eq!(ram.manifest.client.as_deref(), Some("client.luau"));
    let mut mine = Library::default();
    mine.list.insert(ram.id.clone(), ram.clone());
    mine.running.push(ram.id.clone());
    let ada = PlayerInfo { id: 7, name: "Ada".into(), pos: Vec3::ZERO };
    let mut h = Halves::default();
    h.sync(Some(&mine), &ada);
    assert_eq!(h.list["rodeo-ram"].error, None);
    let note = |topic: &str, value: serde_json::Value| crate::script::Note { creation: "rodeo-ram".into(), topic: topic.into(), value };
    h.frame(0.05, &ada, vec![note("ride", serde_json::json!({ "best": 1.0 }))]);
    for _ in 0..12 {
        h.frame(0.1, &ada, vec![]);
    }
    let out = h.take();
    let last = out.pieces.iter().rev().find(|p| p.1 == "ride").and_then(|p| p.2.clone()).expect("a clock");
    assert!(last.text.starts_with("Riding 1.") && last.text.contains("past the record"), "{}", last.text);
    assert_eq!(out.sounds.iter().filter(|s| s.name == "sparkle").count(), 1, "once, as it passes");
    h.frame(0.1, &ada, vec![note("done", serde_json::json!({ "time": 1.3, "record": true }))]);
    let out = h.take();
    assert_eq!(out.pieces.last().and_then(|p| p.2.clone()).map(|p| p.text), Some("You rode 1.3 s, the longest yet!".into()));
    for _ in 0..41 {
        h.frame(0.1, &ada, vec![]);
    }
    assert_eq!(h.take().pieces.last().map(|p| p.2.clone()), Some(None), "gone after a while");
    assert_eq!(h.list["rodeo-ram"].error, None);
}

#[test]
fn creations_give_and_take_the_blocks_survival_players_carry() {
    use crate::blocks::{by_name, PLANKS, SAND, STONE};
    use crate::game::Action;
    use crate::net::{decode, frame_msg, Event, Msg};
    use crate::player::Me;
    home();
    let log_id = by_name("Oak log").unwrap();
    let mut voxels = world();
    let mut herd = Herd::default();
    let mut scripts = Scripts::default();
    let code = r#"
local world = require("@potluck/world@0.3")
world.after(0.1, function()
	local p = world.pockets(7)
	print("ada logs " .. tostring(p["Oak log"]) .. " sand " .. tostring(p.Sand))
	print("ada has " .. world.has(7, "oak log"))
	print("take 2 " .. tostring(world.take(7, "Oak log", 2)))
	print("take 5 " .. tostring(world.take(7, "Oak log", 5)))
	print("give " .. world.give(7, "Planks", 8))
	print("bo " .. tostring(world.pockets(8)) .. " " .. tostring(world.has(8, "Stone") == math.huge) .. " " .. tostring(world.take(8, "Stone", 64)) .. " " .. world.give(8, "Glass", 3))
	print("cy " .. tostring(next(world.pockets(9)) == nil) .. " " .. world.has(9, "Stone") .. " " .. tostring(world.take(9, "Stone")) .. " " .. world.give(9, "Stone"))
	print(pcall(world.give, 7, "Bedrock") and "bedrock ok" or "no bedrock")
	print(pcall(world.give, 7, "Water") and "water ok" or "no water")
	print(pcall(world.give, 7, "Planks", 65) and "65 ok" or "no 65")
	print(pcall(world.give, 7, "Planks", 1.5) and "half ok" or "no half")
	print(pcall(world.has, 7, "Unobtainium") and "unob ok" or "no unob")
	for _, b in { "Stone", "Dirt", "Glass", "Hay", "Snow", "Gravel" } do
		world.give(7, b)
	end
	print("tenth " .. world.give(7, "Lantern") .. " more " .. world.give(7, "Stone", 3))
end)
"#;
    let ada = PlayerInfo { id: 7, name: "Ada".into(), pos: Vec3::ZERO };
    let bo = PlayerInfo { id: 8, name: "Bo".into(), pos: Vec3::ZERO };
    let cy = PlayerInfo { id: 9, name: "Cy".into(), pos: Vec3::ZERO };
    scripts.players = vec![ada.clone(), bo.clone(), cy.clone()];
    scripts.set_pockets(7, true, &[(log_id, 3), (SAND, 2)], 0);
    scripts.set_pockets(8, false, &[], 0);
    scripts.start(&creation(code), &mut herd).unwrap();
    run(&mut scripts, &mut voxels, &mut herd, 0.3);
    let l = scripts.vms["test"].log.join("\n");
    assert_eq!(scripts.vms["test"].error, None, "{l}");
    for want in [
        "ada logs 3 sand 2",
        "ada has 3",
        "take 2 true",
        "take 5 false",
        "give 8",
        "bo nil true true 3",
        "cy true 0 false 0",
        "no bedrock",
        "no water",
        "no 65",
        "no half",
        "no unob",
        "tenth 0 more 3",
    ] {
        assert!(l.contains(want), "{want}: {l}");
    }
    // the changes go to Ada alone, once
    let changes = scripts.pocket_changes();
    assert_eq!(changes.len(), 1, "{changes:?}");
    let (pid, seq, list) = changes[0].clone();
    assert_eq!((pid, seq), (7, 1));
    assert_eq!(&list[..3], &[(log_id, -2), (PLANKS, 8), (STONE, 1)]);
    assert!(scripts.pocket_changes().is_empty());
    // a report from before they arrived changes nothing
    scripts.set_pockets(7, true, &[(log_id, 3), (SAND, 2)], 0);
    assert_eq!(scripts.shared.lock().unwrap().pockets[&7].items[&log_id], 1);

    // on Ada's game: the hotbar takes them, and says so next time
    let mut me = Me::default();
    me.hotbar[0] = log_id;
    me.counts[0] = 3;
    me.hotbar[1] = SAND;
    me.counts[1] = 2;
    me.change_pockets(&list);
    me.pockets_seen = seq;
    let carried = me.carried();
    assert!(carried.contains(&(log_id, 1)) && carried.contains(&(PLANKS, 8)) && carried.contains(&(STONE, 4)) && carried.contains(&(SAND, 2)), "{carried:?}");
    assert_eq!(carried.len(), 9, "a full hotbar: {carried:?}");
    me.change_pockets(&[(SAND, -5)]);
    assert!(!me.carried().iter().any(|c| c.0 == SAND), "never fewer than none");
    scripts.set_pockets(7, true, &me.carried(), me.pockets_seen);
    assert!(!scripts.shared.lock().unwrap().pockets[&7].items.contains_key(&SAND), "an up to date report counts");

    // Ada goes, and what she carried with her
    scripts.players = vec![bo.clone(), cy.clone()];
    run(&mut scripts, &mut voxels, &mut herd, 0.2);
    assert!(!scripts.shared.lock().unwrap().pockets.contains_key(&7));

    // on the host's own game: the hotbar reports itself, and a gift lands in it
    {
        use bevy::ecs::system::RunSystemOnce;
        use crate::settings::Settings;
        use crate::ui::WorldInfo;
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        crate::sim(&mut app, crate::SEED);
        let mut me = Me::default();
        me.hotbar[0] = log_id;
        me.counts[0] = 2;
        app.insert_resource(Settings { survival: true, ..default() }).insert_resource(me);
        app.world_mut().resource_mut::<WorldInfo>().in_world = true;
        let gift = creation("local world = require(\"@potluck/world@0.3\")\nworld.after(0.1, function() for _, p in world.players() do world.give(p, \"Lantern\", world.has(p, \"Oak log\")) end end)");
        app.world_mut().resource_scope(|w, mut s: Mut<Scripts>| {
            let mut herd = w.resource_mut::<Herd>();
            s.start(&gift, &mut herd).unwrap();
        });
        app.world_mut().run_system_once(crate::player::report_pockets).unwrap();
        app.world_mut().run_system_once(crate::game::host_step).unwrap();
        app.world_mut().resource_scope(|w, mut s: Mut<Scripts>| {
            w.resource_scope(|w, mut v: Mut<Voxels>| {
                let mut h = w.resource_mut::<Herd>();
                s.tick(&mut v, &mut h, 0.2);
            })
        });
        app.world_mut().run_system_once(crate::game::host_step).unwrap();
        let me = app.world().resource::<Me>();
        assert!(me.carried().contains(&(crate::blocks::by_name("Lantern").unwrap(), 2)), "{:?}", me.carried());
        assert_eq!(me.pockets_seen, 1);
    }

    // what goes between devices
    for m in [Msg::Pockets { seq: 2, changes: vec![(PLANKS, 4), (log_id, -1)] }, Msg::Act(Action::Pockets { survival: true, items: vec![(PLANKS, 4)], seen: 2 })] {
        let Some(Event::Msg(_, back)) = decode(Wire::Data(1, frame_msg(&m))) else { panic!("didn't decode") };
        assert_eq!(serde_json::to_string(&back).unwrap(), serde_json::to_string(&m).unwrap());
    }
}

#[test]
fn villagers_trade_what_survival_players_gathered() {
    use crate::blocks::{by_name, PLANKS};
    home();
    let lib = Library::load();
    let villagers = lib.list["villagers"].clone();
    let log_id = by_name("Oak log").unwrap();
    let mut voxels = world();
    let mut herd = Herd::default();
    let mut scripts = Scripts::default();
    let ada = PlayerInfo { id: 7, name: "Ada".into(), pos: Vec3::ZERO };
    let bo = PlayerInfo { id: 8, name: "Bo".into(), pos: Vec3::ZERO };
    scripts.players = vec![ada.clone(), bo.clone()];
    scripts.set_pockets(7, true, &[(log_id, 2)], 0);
    scripts.set_pockets(8, false, &[], 0);
    scripts.start(&villagers, &mut herd).unwrap();
    run(&mut scripts, &mut voxels, &mut herd, 0.3);
    let rowan = herd.bodies.values().find(|b| b.kind == "Carpenter").map(|b| b.id).expect("Rowan's on the Green");
    assert!(herd.bodies.values().any(|b| b.kind == "Mason"), "and Ivy");
    let asked = |scripts: &mut Scripts, pid: u32| scripts.shared.lock().unwrap().ui.panels.get(&pid).and_then(|q| q.front().cloned());
    scripts.event(&mut voxels, &mut herd, rowan, "used", &ada);
    let panel = asked(&mut scripts, 7).expect("Rowan asks what to make");
    assert_eq!(panel.choices[0], "4 Planks for 1 Oak log", "{:?}", panel.choices);
    assert_eq!(panel.choices.len(), 4);
    scripts.answer(7, panel.ask, Some(0));
    run(&mut scripts, &mut voxels, &mut herd, 0.2);
    let items = scripts.shared.lock().unwrap().pockets[&7].items.clone();
    assert_eq!((items.get(&log_id).copied(), items.get(&PLANKS).copied()), (Some(1), Some(4)), "{items:?}");
    // a trade she can't pay for takes nothing
    scripts.event(&mut voxels, &mut herd, rowan, "used", &ada);
    let panel = asked(&mut scripts, 7).unwrap();
    scripts.answer(7, panel.ask, Some(3));
    run(&mut scripts, &mut voxels, &mut herd, 0.2);
    assert_eq!(scripts.shared.lock().unwrap().pockets[&7].items.get(&PLANKS).copied(), Some(4));
    assert!(herd.bodies[&rowan].says.as_ref().is_some_and(|s| s.0.contains("You'll need 6 Planks")), "{:?}", herd.bodies[&rowan].says);
    // in creative, there's nothing to trade for
    scripts.event(&mut voxels, &mut herd, rowan, "used", &bo);
    let panel = asked(&mut scripts, 8).unwrap();
    assert!(panel.text.contains("creative"), "{}", panel.text);
    assert_eq!(scripts.vms["villagers"].error, None);
}

#[test]
fn creations_know_the_time_of_day_find_lights_and_hurt_survival_players() {
    use crate::net::{decode, frame_msg, Event, Msg};
    use crate::player::{Me, MAX_HEALTH};
    home();
    let lantern = crate::blocks::by_name("Lantern").unwrap();
    let mut voxels = world();
    let mut herd = Herd::default();
    let mut scripts = Scripts::default();
    // a lantern in the open, far from the Green
    let (x, z) = (world::GREEN.x as i32 + 70, world::GREEN.z as i32 - 40);
    let y = voxels.top(x, z) + 1;
    voxels.set(IVec3::new(x, y, z), lantern);
    let code = format!(
        r#"
local world = require("@potluck/world@0.4")
print(string.format("start %.2f", world.daytime()))
world.after(0.1, function()
	print(string.format("time %.2f light %.2f", world.daytime(), world.daylight()))
	local l = world.find_light(vector.create({x}, {y} + 3, {z}), 5)
	print("found " .. tostring(l and l.x) .. " " .. tostring(l and l.y))
	print("too far " .. tostring(world.find_light(vector.create({x} + 9, {y}, {z}), 6)))
	print("hurt " .. tostring(world.hurt(7, 3)) .. " " .. tostring(world.heal(7, 1)) .. " " .. tostring(world.hurt(7, 500)))
	print("creative " .. tostring(world.hurt(8, 3)) .. " unknown " .. tostring(world.hurt(9, 3)))
	print(pcall(world.hurt, 7, -1) and "less ok" or "no less")
	print(pcall(world.heal, 7, 0/0) and "nan ok" or "no nan")
end)
"#
    );
    scripts.players = vec![PlayerInfo { id: 7, name: "Ada".into(), pos: Vec3::ZERO }, PlayerInfo { id: 8, name: "Bo".into(), pos: Vec3::ZERO }];
    scripts.set_pockets(7, true, &[], 0);
    scripts.set_pockets(8, false, &[], 0);
    scripts.start(&creation(&code), &mut herd).unwrap();
    scripts.set_day(1.0); // midnight
    run(&mut scripts, &mut voxels, &mut herd, 0.3);
    let l = scripts.vms["test"].log.join("\n");
    assert_eq!(scripts.vms["test"].error, None, "{l}");
    for want in [
        "start 0.34",
        "time 0.00 light 0.00",
        &format!("found {} {}", x as f32 + 0.5, y as f32 + 0.5),
        "too far nil",
        "hurt true true true",
        "creative false unknown false",
        "no less",
        "no nan",
    ] {
        assert!(l.contains(want), "{want}: {l}");
    }
    // one change for Ada: three, back one, and at most a whole bar
    assert_eq!(scripts.health_changes(), vec![(7, -3.0 + 1.0 - MAX_HEALTH)]);
    assert!(scripts.health_changes().is_empty());
    scripts.set_day(0.5);
    assert_eq!(crate::render::Clock { time: 0.5, running: true }.daylight(), 1.0);

    // on the host's own game it lands in their health, in survival only
    use bevy::ecs::system::RunSystemOnce;
    use crate::settings::Settings;
    for survival in [true, false] {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        crate::sim(&mut app, crate::SEED);
        app.insert_resource(Settings { survival, ..default() }).insert_resource(Me::default());
        app.init_resource::<crate::input::Controls>().init_resource::<crate::ui::Focus>().init_resource::<crate::ui::Toasts>();
        app.world_mut().resource_mut::<crate::ui::WorldInfo>().in_world = true;
        let bite = creation("local world = require(\"@potluck/world@0.4\")\nworld.after(0.1, function() for _, p in world.players() do world.hurt(p, 5) end end)");
        app.world_mut().resource_scope(|w, mut s: Mut<Scripts>| {
            let mut herd = w.resource_mut::<Herd>();
            s.start(&bite, &mut herd).unwrap();
        });
        app.world_mut().run_system_once(crate::player::report_pockets).unwrap();
        app.world_mut().run_system_once(crate::game::host_step).unwrap();
        app.world_mut().resource_scope(|w, mut s: Mut<Scripts>| {
            w.resource_scope(|w, mut v: Mut<Voxels>| {
                let mut h = w.resource_mut::<Herd>();
                s.tick(&mut v, &mut h, 0.2);
            })
        });
        app.world_mut().run_system_once(crate::game::host_step).unwrap();
        let due = app.world().resource::<Me>().health_due;
        assert_eq!(due, if survival { -5.0 } else { 0.0 }, "survival {survival}");
        app.world_mut().resource_mut::<Me>().health_due = -5.0; // as if sent anyway
        app.world_mut().run_system_once(crate::player::walk).unwrap();
        let me = app.world().resource::<Me>();
        assert_eq!((me.health, me.health_due), (if survival { MAX_HEALTH - 5.0 } else { MAX_HEALTH }, 0.0), "survival {survival}");
    }

    // what goes between devices
    let m = Msg::Health(-2.5);
    let Some(Event::Msg(_, back)) = decode(Wire::Data(1, frame_msg(&m))) else { panic!("didn't decode") };
    assert_eq!(serde_json::to_string(&back).unwrap(), serde_json::to_string(&m).unwrap());
}

#[test]
fn gloomlings_come_out_at_night_nip_and_keep_away_from_light() {
    home();
    let gloomlings = Library::load().list.remove("gloomlings").expect("the base game has the gloomlings");
    let lantern = crate::blocks::by_name("Lantern").unwrap();
    let mut voxels = world();
    // a flat, dark meadow of tall grass, well away from the Green
    let (cx, cz, ground) = (world::GREEN.x as i32 + 80, world::GREEN.z as i32 + 60, 40);
    let grass = crate::blocks::by_name("Tall grass").unwrap();
    for x in cx - 26..=cx + 26 {
        for z in cz - 26..=cz + 26 {
            for y in 1..world::WY {
                let b = match y {
                    _ if y <= ground => crate::blocks::STONE,
                    _ if y == ground + 1 => grass,
                    _ => crate::blocks::AIR,
                };
                voxels.set(IVec3::new(x, y, z), b);
            }
        }
    }
    let ada = Vec3::new(cx as f32 + 0.5, ground as f32 + 1.0, cz as f32 + 0.5);
    let mut herd = Herd::default();
    let mut scripts = Scripts::default();
    scripts.players = vec![PlayerInfo { id: 7, name: "Ada".into(), pos: ada }];
    scripts.set_pockets(7, true, &[], 0);
    scripts.start(&gloomlings, &mut herd).unwrap();
    let count = |herd: &Herd| herd.bodies.values().filter(|b| b.kind == "Gloomling").count();
    let log = |scripts: &Scripts| scripts.vms["gloomlings"].log.join("\n");

    // nothing by day
    scripts.set_day(0.5);
    run(&mut scripts, &mut voxels, &mut herd, 5.0);
    assert_eq!(count(&herd), 0);
    // after dark they come, as many as the knob says, and say so
    scripts.set_day(0.0);
    run(&mut scripts, &mut voxels, &mut herd, 9.0);
    assert_eq!(scripts.vms["gloomlings"].error, None, "{}", log(&scripts));
    let n = count(&herd);
    assert!((2..=4).contains(&n), "{n} gloomlings");
    for b in herd.bodies.values() {
        let d = Vec2::new(b.pos.x - ada.x, b.pos.z - ada.z).length();
        assert!(d < 20.0, "one turned up {d} away");
    }
    let shown = scripts.ui_changes(&[7]);
    assert!(
        shown.iter().any(|(p, c)| *p == 7 && matches!(c, crate::script::UiChange::Piece { id, piece: Some(_), .. } if id == "night")),
        "night falls on Ada's screen"
    );
    // they follow her and nip
    run(&mut scripts, &mut voxels, &mut herd, 12.0);
    let hurt: f32 = scripts.health_changes().iter().filter(|c| c.0 == 7).map(|c| c.1).sum();
    assert!(hurt <= -2.0, "nipped {hurt}");
    assert!(count(&herd) <= 4, "{} after a while", count(&herd));
    // a lantern over her head: they back off and stop
    voxels.set(IVec3::new(cx + 1, ground + 3, cz), lantern);
    run(&mut scripts, &mut voxels, &mut herd, 2.5);
    scripts.health_changes();
    run(&mut scripts, &mut voxels, &mut herd, 8.0);
    assert!(scripts.health_changes().is_empty(), "no nips in the light");
    for b in herd.bodies.values() {
        let d = Vec2::new(b.pos.x - (cx as f32 + 1.5), b.pos.z - (cz as f32 + 0.5)).length();
        assert!(d > 4.0, "one's {d} from the lantern");
    }
    // bop one and it pops
    let id = *herd.bodies.keys().next().expect("they wait at the edge of the light");
    scripts.event(&mut voxels, &mut herd, id, "hit", &scripts.players[0].clone());
    assert!(!herd.bodies.contains_key(&id));
    // in the morning they fade away
    scripts.set_day(0.4);
    run(&mut scripts, &mut voxels, &mut herd, 15.0);
    assert_eq!(count(&herd), 0);
    // and creative players have nothing to fear
    voxels.set(IVec3::new(cx + 1, ground + 3, cz), crate::blocks::AIR);
    scripts.set_pockets(7, false, &[], 0);
    scripts.set_day(0.0);
    run(&mut scripts, &mut voxels, &mut herd, 10.0);
    assert_eq!(count(&herd), 0);
    assert_eq!(scripts.vms["gloomlings"].error, None, "{}", log(&scripts));
}
