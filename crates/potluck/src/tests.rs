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

/// A player's pockets with these in, put in as their game would, as the
/// host last heard.
trait Carry {
    fn carry<T: Copy + Into<crate::items::Thing>>(&mut self, player: u32, survival: bool, list: &[(T, u32)], seen: u32);
    /// How many of a thing the host thinks a player carries.
    fn carried<T: Into<crate::items::Thing>>(&self, player: u32, thing: T) -> u32;
}

impl Carry for Scripts {
    fn carry<T: Copy + Into<crate::items::Thing>>(&mut self, player: u32, survival: bool, list: &[(T, u32)], seen: u32) {
        let mut slots = crate::items::Slots::default();
        for &(t, n) in list {
            slots.give(t.into(), n);
        }
        self.set_pockets(player, survival, &slots.filled(), seen);
    }
    fn carried<T: Into<crate::items::Thing>>(&self, player: u32, thing: T) -> u32 {
        self.shared.lock().unwrap().pockets.get(&player).map(|p| p.count(thing.into())).unwrap_or(0)
    }
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
fn a_creature_used_before_its_first_step_is_spawned_first() {
    // spawned by a timer, and patted before its first tick: `spawned` comes
    // first, so `used` finds the field it sets (the soak caught Gloomlings
    // stopping over this)
    let code = r##"
local world = require("@potluck/world")
local Thing = world.creature {
    name = "Thing", count = 0, home = "paddock",
    body = { { "box", size = { 1, 1, 1 }, at = { 0, 0.5, 0 }, color = "#ffffff" } },
}
function Thing:spawned() self.pats = 0 end
function Thing:used(player) self.pats += 1; print("pats " .. self.pats) end
world.after(0.1, function() world.spawn(Thing, world.spot("paddock")) end)
"##;
    let mut voxels = world();
    let mut herd = Herd::default();
    let mut scripts = Scripts::default();
    scripts.start(&creation(code), &mut herd).unwrap();
    for _ in 0..20 {
        scripts.tick(&mut voxels, &mut herd, 0.05);
        if !herd.bodies.is_empty() {
            break;
        }
    }
    let id = *herd.bodies.keys().next().expect("the timer spawns one");
    let ada = PlayerInfo { id: 1, name: "Ada".into(), pos: herd.bodies[&id].pos };
    scripts.players = vec![ada.clone()];
    scripts.event(&mut voxels, &mut herd, id, "used", &ada);
    run(&mut scripts, &mut voxels, &mut herd, 0.5);
    let vm = &scripts.vms["test"];
    assert_eq!(vm.error, None, "no error");
    assert!(vm.log.iter().any(|l| l.contains("pats 1")), "used once, after spawned: {:?}", vm.log);
}

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
fn worlds_are_kept_with_what_you_carry_and_where_you_stood() {
    use crate::ui::{Act, Acts, Focus, Screen, WorldInfo};
    use crate::worlds::{self, Current};
    home();
    let mut app = App::new();
    app.insert_resource(Library::load())
        .insert_resource(Scripts::default())
        .insert_resource(Herd::default())
        .insert_resource(crate::net::Net::default())
        .insert_resource(crate::ui::Toasts::default())
        .insert_resource(crate::settings::Settings::default())
        .insert_resource(crate::player::Me::default())
        .insert_resource(world())
        .insert_resource(WorldInfo::default())
        .insert_resource(Focus::default())
        .init_resource::<Acts>()
        .init_resource::<Current>()
        .add_systems(Update, worlds::acts);
    let act = |app: &mut App, a: Act| {
        app.world_mut().resource_mut::<Acts>().0.push(a);
        app.update();
        app.update();
        // what's left is `game::do_acts`'s, which this test hasn't got
        app.world_mut().resource_mut::<Acts>().0.clear();
    };
    // a new world, played straight away, from a seed of its own
    act(&mut app, Act::NewWorld(false));
    let id = app.world().resource::<Current>().id.clone().expect("it's being played");
    assert!(app.world().resource::<WorldInfo>().in_world);
    assert_eq!(app.world().resource::<Focus>().screen, Screen::Playing);
    let kept = worlds::list().into_iter().find(|k| k.id == id).expect("it's listed");
    assert!(kept.name.starts_with("World "), "{}", kept.name);
    assert_eq!(app.world().resource::<Voxels>().seed, kept.seed, "its own island");
    assert!(!app.world().resource::<Scripts>().vms.is_empty(), "the base game runs in it");
    // build, carry, move and get hurt
    let spot = IVec3::new(200, 70, 200);
    let planks = crate::blocks::by_name("Planks").unwrap();
    {
        let w = app.world_mut();
        w.resource_mut::<Voxels>().set(spot, planks);
        let mut me = w.resource_mut::<crate::player::Me>();
        me.pockets.0[2] = crate::items::Slot { thing: planks.into(), count: 17 };
        // and something in the backpack, and a tool
        me.pockets.0[30] = crate::items::Slot { thing: crate::items::STONE_AXE, count: 1 };
        me.slot = 2;
        me.pos = Vec3::new(205.5, 71.0, 190.5);
        me.yaw = 1.25;
        me.health = 13.0;
    }
    // the store is the world's own
    app.world().resource::<Scripts>().shared.lock().unwrap().store.set("test", "note", Some(serde_json::json!("kept"))).unwrap();
    act(&mut app, Act::Leave);
    assert!(app.world().resource::<Current>().id.is_none());
    // the title's island and a fresh player, as after leaving
    {
        let w = app.world_mut();
        *w.resource_mut::<Voxels>() = world();
        *w.resource_mut::<crate::player::Me>() = crate::player::Me::default();
        w.resource::<Scripts>().shared.lock().unwrap().store = Default::default();
        w.resource_mut::<WorldInfo>().in_world = false;
    }
    act(&mut app, Act::OpenWorlds(false));
    assert!(app.world().resource::<Focus>().worlds.iter().any(|k| k.id == id), "the list has it");
    {
        let w = app.world();
        let (title, items) = crate::ui::menu_items(w.resource::<Focus>(), w.resource::<Library>(), w.resource::<Scripts>(), w.resource::<crate::settings::Settings>(), w.resource::<WorldInfo>());
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(title, "Your worlds");
        assert_eq!(labels[0], "A new world");
        assert!(labels.iter().any(|l| l.starts_with(&format!("{}  \u{b7}  ", kept.name))), "{labels:?}");
        assert!(labels.contains(&"Back"));
    }
    act(&mut app, Act::PlayWorld(id.clone(), false));
    {
        let w = app.world();
        assert_eq!(w.resource::<Voxels>().at(spot), planks, "what was built is still there");
        let me = w.resource::<crate::player::Me>();
        assert_eq!((me.pockets.0[2], me.slot), (crate::items::Slot { thing: planks.into(), count: 17 }, 2), "and what was carried");
        assert_eq!(me.pockets.0[30], crate::items::Slot { thing: crate::items::STONE_AXE, count: 1 }, "in the backpack too");
        assert!(me.pos.distance(Vec3::new(205.5, 71.0, 190.5)) < 0.01 && (me.yaw - 1.25).abs() < 1e-4, "where they stood: {:?}", me.pos);
        assert_eq!(me.health, 13.0);
        assert_eq!(w.resource::<Scripts>().shared.lock().unwrap().store.get("test", "note").cloned(), Some(serde_json::json!("kept")));
    }
    // an open world can't be deleted; renamed, it keeps its folder
    act(&mut app, Act::ForgetWorld(id.clone()));
    assert!(worlds::list().iter().any(|k| k.id == id), "still there while it's open");
    act(&mut app, Act::Leave);
    app.world_mut().resource_mut::<Focus>().fields.insert("world_name".into(), "  Ada's island  ".into());
    act(&mut app, Act::RenameWorld(id.clone()));
    assert_eq!(worlds::list().iter().find(|k| k.id == id).map(|k| k.name.as_str()), Some("Ada's island"));
    act(&mut app, Act::ForgetWorld(id.clone()));
    assert!(!worlds::list().iter().any(|k| k.id == id), "deleted for good");
    // nothing outside the worlds folder can be named
    assert!(worlds::forget("../creations").is_err() && worlds::load("a/b").is_err());
}

#[test]
fn time_the_machine_gives_to_others_isnt_held_against_a_creation() {
    use crate::script::Slice;
    let ms = Duration::from_millis;
    // off the core past its slice, as when a busy machine runs something else
    let slice = Slice::new(ms(5));
    std::thread::sleep(ms(15));
    if cfg!(unix) {
        assert!(!slice.over(), "waiting isn't running");
    }
    // on it, the slice runs out
    let t = Instant::now();
    while !slice.over() {
        assert!(t.elapsed() < Duration::from_secs(5), "running uses it up");
    }
    // and however busy the machine, the wall has the last word
    let slice = Slice::new(ms(5));
    std::thread::sleep(ms(45));
    assert!(slice.over(), "eight slices of waiting is too long");
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
    to_host.send(net::frame_msg(&Msg::Hello { name: "Drew".into(), protocol: net::PROTOCOL, pass: None, young: false })).unwrap();

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
    start_market_with(forgejo, run, |_| {})
}

/// A market with some of its settings changed first.
fn start_market_with(forgejo: Option<(&str, &str)>, run: &str, edit: impl FnOnce(&mut potluck_share::config::Config) + Send + 'static) -> (String, Arc<potluck_share::market::App>) {
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
            edit(&mut cfg);
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
        to_host.send(net::frame_msg(&Msg::Hello { name: name.into(), protocol: net::PROTOCOL, pass, young: false })).unwrap();
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

/// The way under worlds shared from home, with no market: frames go both
/// ways and come out as `Wire` events, a world shared with Always connect
/// through LinuxGroove gives out no addresses and goes only by the relay,
/// and with no relay at all friends on the same network still connect
/// direct.
#[test]
fn frames_go_both_ways_peer_to_peer() {
    use crate::p2p::{dial, key_text, new_key, Relay, Sharing};
    use std::sync::mpsc::Sender;
    let (map, url, _relay) = crate::p2p::rt().block_on(iroh::test_utils::run_relay_server()).expect("a relay");
    let relay = Relay::testing(map, url);
    // what one side hears: frames, which way it went, and the end
    let hear = |rx: &Receiver<Wire>, want: &[u8], what: &str| -> (Option<Sender<Vec<u8>>>, Vec<bool>) {
        let t = Instant::now();
        let (mut joined, mut ways) = (None, vec![]);
        while t.elapsed() < Duration::from_secs(30) {
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(Wire::Joined(_, out)) => joined = Some(out),
                Ok(Wire::Link(_, l)) => ways.push(l.relayed),
                Ok(Wire::Data(_, b)) if b == want => return (joined, ways),
                Ok(Wire::Gone(_)) => panic!("{what}: gone"),
                _ => {}
            }
        }
        panic!("{what}: heard nothing");
    };

    for (through, with_relay) in [(true, true), (false, false)] {
        let what = if through { "through the relay" } else { "with no relay" };
        let relay = with_relay.then(|| relay.clone());
        let secret = new_key();
        let key = key_text(&secret);
        let (tx, rx) = channel();
        let sharing = Sharing::start(secret, relay.clone(), through, tx, Arc::new(Mutex::new(1))).unwrap();
        assert_eq!(sharing.key(), key);
        let addrs = sharing.addrs();
        assert_eq!(addrs.is_empty(), through, "{what}: addresses {addrs:?}");
        let (ctx, crx) = channel();
        let out = dial(new_key(), relay, through, &key, &addrs, ctx).unwrap();
        out.send(b"hello".to_vec()).unwrap();
        let (back, host_ways) = hear(&rx, b"hello", what);
        back.expect("the friend comes out as a player").send(b"welcome".to_vec()).unwrap();
        let (_, friend_ways) = hear(&crx, b"welcome", what);
        assert!(!host_ways.is_empty() && host_ways.iter().all(|r| *r == through), "{what}: the host's way {host_ways:?}");
        assert!(friend_ways.iter().all(|r| *r == through), "{what}: the friend's way {friend_ways:?}");
        // the friend leaves: the host hears they've gone
        drop(out);
        let t = Instant::now();
        while !matches!(rx.recv_timeout(Duration::from_millis(100)), Ok(Wire::Gone(_))) {
            assert!(t.elapsed() < Duration::from_secs(20), "{what}: the host never heard the friend go");
        }
    }
    assert!(dial(new_key(), None, true, &key_text(&new_key()), &[], channel().0).is_err(), "through the relay with no relay can't go anywhere");
}

/// A relay for the tests that asks the market who may use it, as the
/// real one does with `access.http`.
#[derive(Debug)]
struct AsksTheMarket(Arc<std::sync::OnceLock<(String, String)>>);

impl iroh_relay::server::AccessControl for AsksTheMarket {
    async fn on_connect(&self, r: &iroh_relay::server::ClientRequest) -> iroh_relay::server::Access {
        let Some((market, token)) = self.0.get().cloned() else { return iroh_relay::server::Access::Deny { reason: None } };
        let key = r.endpoint_id().to_string();
        let ok = tokio::task::spawn_blocking(move || relay_asks(&market, &token, &key)).await.unwrap_or(false);
        if ok { iroh_relay::server::Access::Allow } else { iroh_relay::server::Access::Deny { reason: None } }
    }
}

/// What the market tells the relay about a key.
fn relay_asks(market: &str, token: &str, key: &str) -> bool {
    ureq::post(format!("{market}/api/relay/allow"))
        .header("Authorization", format!("Bearer {token}"))
        .header("X-Iroh-Endpoint-Id", key)
        .send_empty()
        .ok()
        .and_then(|mut r| r.body_mut().read_to_string().ok())
        .is_some_and(|t| t == "true")
}

/// Sharing a world from home, peer to peer: Ada and Bo swap friend codes,
/// Ada shares her world to Friends and Bo comes in under the name the
/// market knows, straight to her game. Cy, who isn't a friend, is turned
/// away, as is a pass made for another key; set to Anyone with the code,
/// Cy comes in. A younger friend joins only while it's set to Friends, and
/// Always connect through LinuxGroove goes by the relay, which lets in only
/// the keys the market gave out. Invites put the world at the top of a
/// friend's list, and once Ada stops sharing nobody gets in.
#[test]
fn a_world_shared_from_home_lets_friends_in_peer_to_peer() {
    use crate::net::{self, Event, Msg, Net};
    use crate::share::{Done, ShareJobs, Who, WorldWay};
    use crate::ui::Focus;
    home();
    let run = crate::settings::random_hex(5);
    let token = format!("relay-{run}");
    let asks = Arc::new(std::sync::OnceLock::new());
    let (map, relay_url, _relay) = crate::p2p::rt().block_on(iroh::test_utils::run_relay_server_with_access(true, Arc::new(AsksTheMarket(asks.clone())))).expect("a relay");
    let relay = crate::p2p::Relay::testing(map, relay_url);
    let (relay_text, token2) = (relay.url(), token.clone());
    let (market, _mkt) = start_market_with(None, &run, move |cfg| {
        cfg.relay = relay_text;
        cfg.relay_token = token2;
    });
    asks.set((market.clone(), token.clone())).unwrap();
    let server = fake_game_server();
    let who = |name: &str, young: bool| Who { server: server.clone(), market: market.clone(), device: format!("{}-{run}", name.to_lowercase()), name: format!("{name} {run}"), young };
    let (ada, bo, cy, dee) = (who("Ada", false), who("Bo", false), who("Cy", false), who("Dee", true));
    // each player's game signs in on its own
    let jobs: std::collections::HashMap<String, ShareJobs> = [&ada, &bo, &cy, &dee].iter().map(|w| (w.device.clone(), ShareJobs::default())).collect();
    let j = |w: &Who| &jobs[&w.device];
    let page = |w: &Who| {
        j(w).friends(w);
        match done(j(w), "a friends page") {
            Done::Friends { page, .. } => page,
            _ => panic!("not a friends page"),
        }
    };
    let said = |w: &Who, what: &str| match done(j(w), what) {
        Done::Friends { said, page } => (said, page),
        _ => panic!("{what}: not a friends page"),
    };

    // Ada's friend code, typed by Bo the way people type them
    let code = page(&ada).code;
    assert_eq!(code.len(), 8, "{code}");
    j(&bo).add_friend(&bo, &format!("{}-{}", &code[..4], &code[4..]).to_lowercase());
    let (s, bo_page) = said(&bo, "Bo asks");
    assert!(s.starts_with("Sent"), "{s}");
    assert_eq!(bo_page.asked.len(), 1, "Bo sees his request waiting");
    let asking = page(&ada).asking;
    assert_eq!(asking.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(), vec![bo.name.as_str()], "Ada sees Bo asking");
    j(&ada).answer_friend(&ada, asking[0].id, true);
    let (s, ada_page) = said(&ada, "Ada says yes");
    assert!(s.contains("friends now"), "{s}");
    let bo_handle = ada_page.friends.iter().find(|f| f.name == bo.name).expect("Bo is Ada's friend").handle.clone();
    assert!(page(&bo).friends.iter().any(|f| f.name == ada.name), "and Ada is Bo's");

    // Cy asks, Ada blocks him, and her code looks like nobody's to him after
    j(&cy).add_friend(&cy, &code);
    said(&cy, "Cy asks");
    let cy_handle = page(&ada).asking.iter().find(|a| a.name == cy.name).expect("Cy asking").handle.clone();
    j(&ada).unfriend(&ada, &cy_handle, true);
    let (s, ada_page) = said(&ada, "Ada blocks Cy");
    assert!(s.starts_with("Blocked") && ada_page.blocked.len() == 1 && ada_page.asking.is_empty(), "{s} {ada_page:?}");
    j(&cy).add_friend(&cy, &code);
    let (s, _) = said(&cy, "Cy asks again");
    assert!(s.contains("isn't anyone's friend code"), "a block looks like a wrong code: {s}");
    j(&ada).unblock(&ada, &cy_handle);
    said(&ada, "Ada unblocks Cy");

    // Dee, who is under 13, becomes Ada's friend too
    j(&dee).add_friend(&dee, &code);
    said(&dee, "Dee asks");
    let dee_asks = page(&ada).asking.into_iter().find(|a| a.name == dee.name).expect("Dee asking");
    assert!(dee_asks.young, "Ada's game knows Dee is younger");
    j(&ada).answer_friend(&ada, dee_asks.id, true);
    said(&ada, "Ada says yes to Dee");

    // Ada shares her world, to Friends
    let mut host = App::new();
    host.add_plugins(MinimalPlugins);
    crate::sim(&mut host, crate::SEED);
    host.insert_resource(crate::settings::Settings { name: ada.name.clone(), server: server.clone(), market: market.clone(), device: ada.device.clone(), ..Default::default() });
    host.update();
    let secret = crate::p2p::new_key();
    let ada_key = crate::p2p::key_text(&secret);
    let beat = |host: &App, key: &str| {
        let w = host.world();
        crate::game::world_beat(w.resource::<Net>(), w.resource::<crate::settings::Settings>(), w.resource::<Voxels>(), key, false)
    };
    host.world().resource::<ShareJobs>().share_world(&ada, beat(&host, &ada_key), Some(secret));
    let t = Instant::now();
    let world_code = loop {
        host.update();
        if let Some(s) = &host.world().resource::<Focus>().shared {
            break s.code.clone();
        }
        assert!(t.elapsed() < Duration::from_secs(30), "Ada's world was never shared: {}", host.world().resource::<Focus>().status);
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(world_code.len(), 6, "{world_code}");
    // the game tells the market where it listens once it knows
    let t = Instant::now();
    while host.world().resource::<Focus>().shared.as_ref().is_some_and(|s| s.sent_addrs.is_empty()) {
        host.update();
        assert!(t.elapsed() < Duration::from_secs(20), "Ada's game never said where it listens");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(host.world().resource::<Net>().shared().map(|(c, k, _)| (c.to_string(), k.to_string())), Some((world_code.clone(), ada_key.clone())));
    assert_eq!(host.world().resource::<Focus>().shared.as_ref().map(|s| s.who.as_str()), Some("friends"));
    assert!(relay_asks(&market, &token, &ada_key), "the relay may carry Ada's world");
    assert!(!relay_asks(&market, &token, &crate::p2p::key_text(&crate::p2p::new_key())), "and nobody else");
    assert!(!relay_asks(&market, "not-the-token", &ada_key), "when it's really the relay asking");
    let mut r = ureq::get(format!("{market}/w/{}", world_code.to_lowercase())).call().expect("the world's page");
    let html = r.body_mut().read_to_string().unwrap();
    assert!(html.contains(&crate::ui::pretty_world_code(&world_code)), "the QR code's page shows the code");

    // a friend's game dials Ada's: the name it's welcomed under, whether it
    // went by the relay (as both games see it) and its connection, or why not
    let knock = |host: &mut App, w: WorldWay, through: bool, hello: &str, young: bool, key: Option<iroh::SecretKey>| -> Result<(String, bool, std::sync::mpsc::Sender<Vec<u8>>), String> {
        let (tx, rx) = channel();
        let out = crate::p2p::dial(key.unwrap_or(w.secret), crate::p2p::Relay::parse(&w.relay), through, &w.key, &w.addrs, tx).unwrap();
        out.send(net::frame_msg(&Msg::Hello { name: hello.into(), protocol: net::PROTOCOL, pass: Some(w.pass), young })).unwrap();
        let t = Instant::now();
        let (mut name, mut relayed) = (None, vec![]);
        while t.elapsed() < Duration::from_secs(40) {
            host.update();
            std::thread::sleep(Duration::from_millis(10));
            for e in rx.try_iter() {
                match net::decode(e) {
                    Some(Event::Link(_, l)) => relayed.push(l.relayed),
                    Some(Event::Msg(_, Msg::Refused(why))) => return Err(why),
                    Some(Event::Msg(_, Msg::Roster(all))) if name.is_none() => name = all.last().cloned(),
                    Some(Event::Gone(_)) => return Err(format!("gone (heard {name:?})")),
                    _ => {}
                }
            }
            // through the relay: every look at the way says so; direct: it
            // gets there once the two games find each other
            let Some(n) = &name else { continue };
            let host_sees = host.world().resource::<Net>().remote_players().iter().find(|p| &p.name == n).and_then(|p| p.link.as_ref().map(|l| l.relayed));
            if through && !relayed.is_empty() && t.elapsed() > Duration::from_secs(3) && host_sees == Some(true) {
                return Ok((n.clone(), relayed.iter().all(|r| *r), out));
            }
            if !through && relayed.last() == Some(&false) && host_sees == Some(false) {
                return Ok((n.clone(), false, out));
            }
        }
        panic!("{hello} heard nothing back (welcomed as {name:?}, ways {relayed:?})");
    };
    let way = |w: &Who, code: &str| {
        j(w).join_world(w, code);
        match done(j(w), "the way in") {
            Done::WorldWay(w) => w,
            _ => panic!("not a way in"),
        }
    };

    let mut w = way(&bo, &world_code.to_lowercase());
    let t = Instant::now();
    while w.addrs.is_empty() {
        host.update();
        assert!(t.elapsed() < Duration::from_secs(20), "the market never heard where Ada's game listens");
        std::thread::sleep(Duration::from_millis(100));
        w = way(&bo, &world_code);
    }
    assert_eq!((w.code.as_str(), w.key.as_str(), w.name.clone()), (world_code.as_str(), ada_key.as_str(), format!("{}'s world", ada.name)));
    let came = knock(&mut host, w, false, "bo the impostor", false, None);
    assert_eq!(came.as_ref().map(|(n, r, _)| (n.as_str(), *r)), Ok((bo.name.as_str(), false)), "Bo comes in under the name the market knows, straight to Ada's game");
    let bo_there = host.world().resource::<Net>().remote_players().iter().map(|p| p.handle.clone()).collect::<Vec<_>>();
    assert_eq!(bo_there, vec![Some(bo_handle.clone())], "Ada's game knows who Bo is");
    drop(came);

    // a pass made for another game's key doesn't open it
    let w = way(&bo, &world_code);
    let why = knock(&mut host, w, false, "Bo", false, Some(crate::p2p::new_key())).expect_err("a pass for another key");
    assert!(why.contains("another world"), "{why}");

    // Cy isn't Ada's friend: the market says so, and won't give him a way in
    j(&cy).join_world(&cy, &world_code);
    let why = failed(j(&cy), "Cy joining a friends' world");
    assert!(why.contains("friends"), "{why}");

    // Anyone with the code: Cy comes in, and Dee can't while it's that way
    host.world_mut().resource_mut::<crate::settings::Settings>().world_who = "code".into();
    host.world().resource::<ShareJobs>().share_world(&ada, beat(&host, &ada_key), None);
    let t = Instant::now();
    while host.world().resource::<Focus>().shared.as_ref().is_some_and(|s| s.who != "code") {
        host.update();
        assert!(t.elapsed() < Duration::from_secs(30), "who may come never changed");
        std::thread::sleep(Duration::from_millis(20));
    }
    let w = way(&cy, &world_code);
    assert_eq!(knock(&mut host, w, false, "Cy", false, None).map(|(n, _, _)| n), Ok(cy.name.clone()), "anyone with the code comes in");
    j(&dee).join_world(&dee, &world_code);
    let why = failed(j(&dee), "Dee joining a world open to anyone");
    assert!(why.contains("younger players"), "{why}");

    // back to Friends: Dee comes in, always through LinuxGroove
    host.world_mut().resource_mut::<crate::settings::Settings>().world_who = "friends".into();
    host.world().resource::<ShareJobs>().share_world(&ada, beat(&host, &ada_key), None);
    let t = Instant::now();
    while host.world().resource::<Focus>().shared.as_ref().is_some_and(|s| s.who != "friends") {
        host.update();
        assert!(t.elapsed() < Duration::from_secs(30), "who may come never changed back");
        std::thread::sleep(Duration::from_millis(20));
    }
    let w = way(&dee, &world_code);
    assert!(relay_asks(&market, &token, &crate::p2p::key_text(&w.secret)), "the relay may carry Dee's connection");
    assert_eq!(knock(&mut host, w, true, "Dee", true, None).map(|(n, r, _)| (n, r)), Ok((dee.name.clone(), true)), "Dee comes in by the relay, and only by it");

    // an invite puts Ada's world first on Bo's list
    j(&ada).invite(&ada, &world_code, &bo_handle);
    let Done::Said(s) = done(j(&ada), "Ada invites Bo") else { panic!("not said") };
    assert!(s.starts_with("Invited"), "{s}");
    j(&bo).worlds(&bo);
    let Done::Worlds(list) = done(j(&bo), "Bo's friends' worlds") else { panic!("not worlds") };
    assert_eq!(list.first().map(|o| (o.code.as_str(), o.invited)), Some((world_code.as_str(), true)), "{list:?}");
    j(&cy).worlds(&cy);
    let Done::Worlds(list) = done(j(&cy), "Cy's friends' worlds") else { panic!("not worlds") };
    assert!(list.is_empty(), "Cy isn't Ada's friend, so her world isn't on his list: {list:?}");

    // Bo's own game joins by the code, the way Join a friend's world does
    let mut guest = App::new();
    guest.add_plugins(MinimalPlugins);
    crate::sim(&mut guest, crate::SEED);
    guest.init_resource::<crate::player::Me>().init_resource::<crate::mesh::ChunkEntities>().add_systems(Update, net::client_net);
    guest.insert_resource(crate::settings::Settings { name: "Bo".into(), server: server.clone(), market: market.clone(), device: bo.device.clone(), ..Default::default() });
    guest.update();
    guest.world().resource::<ShareJobs>().join_world(&bo, &world_code);
    let t = Instant::now();
    while guest.world().resource::<Net>().relayed() != Some(false) || !host.world().resource::<Net>().remote_players().iter().any(|p| p.name == bo.name) {
        host.update();
        guest.update();
        assert!(t.elapsed() < Duration::from_secs(30), "Bo's game never got in: {}", guest.world().resource::<Focus>().status);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(guest.world().resource::<crate::ui::WorldInfo>().line.ends_with("direct"), "Bo's game says which way it went: {}", guest.world().resource::<crate::ui::WorldInfo>().line);
    assert!(host.world().resource::<Focus>().shared.as_ref().is_some_and(|s| s.players.iter().any(|(n, relayed)| n == &bo.name && !relayed)), "Share this world lists Bo, direct");
    drop(guest);

    // Ada stops sharing: her game closes it on the market
    host.world_mut().resource_mut::<Net>().stop_sharing();
    let t = Instant::now();
    while host.world().resource::<Focus>().shared.is_some() {
        host.update();
        assert!(t.elapsed() < Duration::from_secs(10), "sharing never stopped");
        std::thread::sleep(Duration::from_millis(20));
    }
    let t = Instant::now();
    loop {
        host.update();
        j(&bo).join_world(&bo, &world_code);
        match jobs_wait(j(&bo)) {
            Done::Failed(why) => {
                assert!(why.contains("isn't open"), "{why}");
                break;
            }
            _ => assert!(t.elapsed() < Duration::from_secs(10), "the market still offers a way into a world that stopped"),
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// The next thing a job finished with, failures included.
fn jobs_wait(jobs: &crate::share::ShareJobs) -> crate::share::Done {
    let start = Instant::now();
    loop {
        if let Some(d) = jobs.poll().into_iter().next() {
            return d;
        }
        assert!(start.elapsed() < Duration::from_secs(60), "a job took too long");
        std::thread::sleep(Duration::from_millis(50));
    }
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

/// A younger player's new friends wait for a parent: with no PIN the game
/// says to ask for one, with one it asks for it, and only the right PIN
/// sends the request. Their world is always for their friends, Always
/// connect through LinuxGroove turns on with the age answer, and a parent
/// can turn sharing and joining worlds off.
#[test]
fn a_younger_players_friends_and_worlds_wait_for_a_parent() {
    use bevy::ecs::system::RunSystemOnce;
    use crate::settings::Settings;
    use crate::ui::{menu_items, Act, Acts, Focus, Kind, Screen, SharedWorld, WorldInfo};
    home();
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    crate::sim(&mut app, crate::SEED);
    let offline = Settings { market: "http://127.0.0.1:9".into(), server: "http://127.0.0.1:9".into(), ..Default::default() };
    app.insert_resource(offline.clone()).insert_resource(crate::player::Me::default());
    let run = |app: &mut App, act: Act| {
        app.world_mut().resource_mut::<Acts>().0.push(act);
        app.world_mut().run_system_once(crate::game::do_acts).unwrap();
        app.world_mut().run_system_once(crate::game::do_acts).unwrap();
    };
    run(&mut app, Act::SetAge(true));
    assert!(app.world().resource::<Settings>().through_relay, "a younger player's friends never see their address");

    run(&mut app, Act::AddFriend("N8GT-3WFA".into()));
    assert!(app.world().resource::<Focus>().status.contains("parent"), "{}", app.world().resource::<Focus>().status);
    app.world_mut().resource_mut::<Settings>().parent_pin = offline.pin_hash("2468");
    run(&mut app, Act::AddFriend("N8GT-3WFA".into()));
    assert!(matches!(&app.world().resource::<Focus>().screen, Screen::ParentOk(a) if **a == Act::AddFriend("N8GT-3WFA".into())), "the PIN first");
    run(&mut app, Act::ParentOk("1357".into()));
    assert!(matches!(app.world().resource::<Focus>().screen, Screen::ParentOk(_)), "not with the wrong PIN");
    run(&mut app, Act::ParentOk("2468".into()));
    let focus = app.world().resource::<Focus>();
    assert_eq!((&focus.screen, focus.status.as_str()), (&Screen::Friends, "Sending..."), "the right PIN sends it");
    app.world_mut().resource_mut::<Settings>().friends_ok = false;
    run(&mut app, Act::AnswerFriend(7, true));
    assert_eq!(app.world().resource::<Focus>().screen, Screen::Friends, "a parent can let them make friends on their own");

    // Share this world: friends only, and a parent can turn it off
    let (lib, scripts, info) = (app.world().resource::<Library>(), app.world().resource::<Scripts>(), WorldInfo::default());
    let mut settings = app.world().resource::<Settings>().clone();
    let mut focus = Focus::default();
    focus.go(Screen::ShareWorld);
    let items = |focus: &Focus, settings: &Settings| menu_items(focus, lib, scripts, settings, &info).1;
    let shown = items(&focus, &settings);
    assert!(!shown.iter().any(|i| matches!(&i.kind, Kind::Choice { act: Act::WorldWho, .. })), "younger players don't choose who joins");
    assert!(shown.iter().any(|i| i.kind == Kind::Button(Act::ShareWorld)));
    focus.shared = Some(SharedWorld { code: "K7FQ2M".into(), url: "https://market.example/w/K7FQ2M".into(), who: "friends".into(), name: "Kid's world".into(), players: vec![("Ada".into(), true)], ..Default::default() });
    let shown = items(&focus, &settings);
    assert!(shown.iter().any(|i| i.label == "K7F-Q2M" && i.kind == Kind::Big), "the code, big");
    assert!(shown.iter().any(|i| i.kind == Kind::Qr), "and its QR code");
    assert!(shown.iter().any(|i| i.label.contains("Ada") && i.label.contains("through LinuxGroove")), "who's here, and which way");
    settings.share_worlds = false;
    assert!(!items(&focus, &settings).iter().any(|i| i.kind == Kind::Button(Act::StopSharing) || i.kind == Kind::Button(Act::ShareWorld)), "a parent turned sharing off");
    settings.join_worlds = false;
    focus.go(Screen::Join);
    assert!(!items(&focus, &settings).iter().any(|i| matches!(i.kind, Kind::Button(Act::JoinCode(_)))), "or joining friends' worlds");
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
    // the Old Greens build their three ruins once; what's counted below is what changes after
    let built = |scripts: &Scripts| {
        let shared = scripts.shared.lock().unwrap();
        // the store keeps a Luau list as {"#1": .., "#2": ..}
        let sites: Vec<serde_json::Value> = shared.store.get("old-greens", "sites").and_then(|s| s.as_object()).map(|o| o.values().cloned().collect()).unwrap_or_default();
        sites.len() == 3 && sites.iter().all(|s| s["built"] == serde_json::Value::Bool(true))
    };
    for _ in 0..30 {
        if built(&scripts) {
            break;
        }
        run(&mut scripts, &mut voxels, &mut herd, 1.0);
    }
    assert!(built(&scripts), "the Old Greens build their ruins");
    voxels.changes.clear();
    let kinds: std::collections::BTreeSet<String> = herd.bodies.values().map(|b| b.kind.clone()).collect();
    // everything but the wild creatures that only turn up near survival players (Gloomlings
    // at night, and the woods' and caves' ones), mods with no bodies, balloons (which only
    // come from a basket put down) and the Old Greens (which only build)
    let only_near = ["gloomlings", "crawlers", "boars", "imps", "sentinels", "balloon", "old-greens"];
    for c in base.iter().filter(|c| !only_near.contains(&c.id.as_str()) && c.manifest.kind != "mod") {
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
    balloon.id = "airship".into();
    balloon.manifest.name = "Balloon".into();
    balloon.origin = Origin::Code(codes[1].clone());
    balloon.code = Some(codes[1].clone());
    lib.list.insert("airship".into(), balloon);
    lib.running.push("airship".into());
    let arches = green.arches(&lib);
    assert_eq!(arches[0], Some(Shows::Here("airship".into())), "what players brought comes first");
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
    app.world_mut().resource_mut::<Library>().list.insert("airship".into(), lib.list["airship"].clone());
    app.world_mut().resource_mut::<Library>().running.push("airship".into());
    app.world_mut().resource_mut::<Acts>().0.push(Act::Arch(0));
    app.world_mut().run_system_once(crate::game::do_acts).unwrap();
    assert!(app.world().resource::<Focus>().screen == Screen::Inside("airship".into()), "looks inside the balloon");

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
print(pcall(ui.ask, 1, { choices = { "a", "b", "c", "d", "e", "f", "g", "h", "i" } }, function() end) and "choice nine ok" or "no choice nine")
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
    assert!(log(&scripts).contains("no middle") && log(&scripts).contains("no choice nine") && log(&scripts).contains("no nine"), "{}", log(&scripts));
    let piece = |text: &str, at: &str| Some(Piece { text: text.into(), at: at.into(), meter: None });
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

/// A Blockbench model is a body (world 0.11): its cubes are parts, sized 16
/// pixels to a block, coloured from its texture, with head and leg from its
/// groups' names; what can't be a part is said in the log.
#[test]
fn a_blockbench_model_is_a_body() {
    use crate::model::{read, MAX_PARTS};
    home();
    // a red pixel picture, 2 by 2, for a texture
    let png = {
        let mut img = image::RgbaImage::new(2, 2);
        for p in img.pixels_mut() {
            *p = image::Rgba([200, 30, 30, 255]);
        }
        img.put_pixel(1, 1, image::Rgba([10, 10, 200, 255]));
        let mut bytes = std::io::Cursor::new(vec![]);
        img.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
        use base64::Engine;
        base64::engine::general_purpose::STANDARD.encode(bytes.into_inner())
    };
    let model = format!(
        r#"{{
        "resolution": {{"width": 16, "height": 16}},
        "elements": [
            {{"name": "body", "from": [-4, 2, -4], "to": [4, 10, 4], "uuid": "b", "color": 3,
              "faces": {{"north": {{"uv": [0, 0, 16, 16], "texture": 0}}, "up": {{"uv": [0, 0, 8, 8], "texture": 0}}}}}},
            {{"name": "snout", "from": [-1, 6, -8], "to": [1, 8, -4], "uuid": "s", "rotation": [0, 22.5, 0], "inflate": 0.5, "color": 1}},
            {{"name": "paw", "from": [-3, 0, -3], "to": [-1, 2, -1], "uuid": "p"}},
            {{"name": "tail", "type": "mesh", "uuid": "t"}},
            {{"name": "ghost", "from": [0, 0, 0], "to": [1, 1, 1], "uuid": "g"}}
        ],
        "outliner": [
            "b",
            {{"uuid": "h", "children": ["s"]}},
            {{"name": "Front legs", "children": ["p"]}},
            {{"name": "hidden", "visibility": false, "children": ["g"]}}
        ],
        "groups": [{{"uuid": "h", "name": "Head"}}],
        "textures": [{{"name": "skin", "uv_width": 16, "uv_height": 16, "source": "data:image/png;base64,{png}"}}]
    }}"#
    );
    let (parts, notes) = read("m.bbmodel", &model, 1.0).unwrap();
    let names: Vec<&str> = parts.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["body", "snout", "paw"], "cubes only, and nothing hidden");
    let (body, snout, paw) = (&parts[0], &parts[1], &parts[2]);
    assert_eq!((body.size, body.at), (Vec3::splat(0.5), Vec3::new(0.0, 0.375, 0.0)), "16 pixels to a block, at its middle");
    assert_eq!(body.color, [200, 30, 30], "the most common colour on its faces");
    assert!(snout.head && !snout.leg && paw.leg && !paw.head && !body.head, "head and legs from the groups' names, new and old style");
    assert_eq!(snout.size, Vec3::new(3.0, 3.0, 5.0) / 16.0, "inflate grows it on every side");
    assert_eq!((snout.color, paw.color), ([0xf2, 0xd1, 0x5b], [0x89, 0xc6, 0xe8]), "no texture: its marker colour");
    assert!(notes.iter().any(|n| n.contains("1 turned cubes are drawn upright")) && notes.iter().any(|n| n.contains("1 meshes are left out")), "{notes:?}");
    // the body's picture: its faces' rectangles on the texture, 0 to 1 across it
    let (tex, faces) = body.picture.expect("the body has a picture");
    assert_eq!((tex, faces[3], faces[4], faces[0]), (0, Some([0.0, 0.0, 1.0, 1.0]), Some([0.0, 0.0, 0.5, 0.5]), None), "north and up, nothing east");
    assert!(snout.picture.is_none() && paw.picture.is_none(), "no faces, no picture");
    // drawn: only the faces it has, each showing its rectangle as seen from outside
    let mesh = crate::creatures::painted_mesh(Vec3::splat(2.0), &[None, None, None, Some([0.25, 0.5, 0.75, 1.0]), None, None], true);
    let pos = mesh.attribute(Mesh::ATTRIBUTE_POSITION).and_then(|a| a.as_float3()).unwrap();
    let Some(bevy::mesh::VertexAttributeValues::Float32x2(uv)) = mesh.attribute(Mesh::ATTRIBUTE_UV_0) else { panic!("no UVs") };
    assert_eq!(pos.len(), 4, "one face");
    // seen from the north (looking south), its top left is east and up
    let top_left = pos.iter().position(|p| p[0] > 0.0 && p[1] > 0.0).unwrap();
    let bottom_right = pos.iter().position(|p| p[0] < 0.0 && p[1] < 0.0).unwrap();
    assert_eq!((uv[top_left], uv[bottom_right]), ([0.25, 0.5], [0.75, 1.0]));
    let (big, _) = read("m.bbmodel", &model, 2.0).unwrap();
    assert_eq!(big[0].size, Vec3::ONE, "scaled");
    // mistakes, in plain words
    assert!(read("m.bbmodel", "{ nope", 1.0).unwrap_err().starts_with("m.bbmodel isn't a Blockbench model"));
    assert!(read("m.bbmodel", "{}", 1.0).unwrap_err().contains("has no cubes"));
    assert!(read("m.bbmodel", r#"{"elements": [{"type": "mesh"}]}"#, 1.0).unwrap_err().contains("no cubes in it to make a body"));
    let many: Vec<String> = (0..70).map(|i| format!(r#"{{"name": "c{i}", "from": [0, 0, 0], "to": [1, 1, 1]}}"#)).collect();
    let (parts, notes) = read("m.bbmodel", &format!(r#"{{"elements": [{}]}}"#, many.join(",")), 1.0).unwrap();
    assert!(parts.len() == MAX_PARTS && notes.iter().any(|n| n.contains("6 are left out")), "{notes:?}");

    // in a creation: body = "file" is short for world.model, and code can change the parts first
    let mut herd = Herd::default();
    let mut scripts = Scripts::default();
    let mut c = creation(
        r##"
local world = require("@potluck/world@0.11")
local body = world.model("models/m.bbmodel")
body[1].color = "#00ff00"
world.creature { name = "Changed", count = 1, home = "paddock", body = body }
world.creature { name = "Plain", count = 1, home = "paddock", body = "models/m.bbmodel" }
for _, bad in { "models/none.bbmodel", "main.luau" } do
    local ok, err = pcall(world.model, bad)
    print(err)
end
"##,
    );
    c.files.insert("models/m.bbmodel".into(), model.clone());
    scripts.start(&c, &mut herd).unwrap();
    let mut voxels = world();
    run(&mut scripts, &mut voxels, &mut herd, 0.2);
    let of = |kind: &str| herd.bodies.values().find(|b| b.kind == kind).unwrap_or_else(|| panic!("a {kind}"));
    let (changed, plain) = (of("Changed"), of("Plain"));
    assert_eq!(plain.parts.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["body", "snout", "paw"]);
    assert!(plain.parts[1].head && plain.parts[2].leg && (plain.parts[0].color[0] - 200.0 / 255.0).abs() < 0.01, "{:?}", plain.parts);
    assert!(changed.parts[0].color[1] > 0.99 && changed.parts[0].color[0] < 0.01, "the code's colour: {:?}", changed.parts[0].color);
    // the picture goes with the part, with how far the code moved its colour
    let pic = plain.parts[0].paint.as_ref().expect("a model's part keeps its picture");
    assert_eq!((pic.file.as_str(), pic.texture, pic.stamp, pic.faces[3]), ("models/m.bbmodel", 0, crate::model::stamp(&model), Some([0.0, 0.0, 1.0, 1.0])));
    assert!(pic.tint.iter().all(|t| (t - 1.0).abs() < 0.01), "as the model has it: {:?}", pic.tint);
    let tint = changed.parts[0].paint.as_ref().unwrap().tint;
    assert!(tint[0] < 0.01 && tint[1] == 4.0 && tint[2] < 0.01, "red to green: {tint:?}");
    assert!(plain.parts[1].paint.is_none());
    // and between games, as part of a body's look
    let json = serde_json::to_string(&plain.parts).unwrap();
    assert_eq!(serde_json::from_str::<Vec<creatures::Part>>(&json).unwrap(), plain.parts);
    assert!(!serde_json::to_string(&plain.parts[1]).unwrap().contains("paint"), "nothing extra for a part with no picture");
    let log = scripts.vms["test"].log.join("\n");
    assert!(log.contains("drawn upright"), "notes go to the log: {log}");
    assert!(log.contains("there's no file called models/none.bbmodel") && log.contains("reads Blockbench models (.bbmodel), not main.luau"), "{log}");
}

/// sound 0.2: a creation's own sounds, made from numbers in its
/// potluck.toml, the same on every game; played by name before the game's,
/// by behaviours too; and mistakes in them say what's wrong.
#[test]
fn a_creation_makes_sounds_of_its_own() {
    use crate::sound::{make_own, note, own_recipe, recipe, Own, OWN_LONGEST, RATE, SOUNDS};
    home();
    let sounds = r#"
[sounds.moo]
wave = "saw"
pitch = [220, 160]
length = 0.9
attack = 0.08
wobble = [5, 12]
pulse = [7, 0.3]
lowpass = 1600

[[sounds.moo.layers]]
wave = "noise"
pitch = 900
length = 0.3
at = 0.05
gain = 0.3

[sounds.tada]
wave = "square"
notes = ["C5", "E5", "G5", "-", "C6"]
each = 0.12

[sounds.quack]
caption = "A duck of its own"
wave = "triangle"
pitch = 300
length = 9
"#;
    let with = |toml: &str, code: &str| {
        let mut c = creation(code);
        let text = format!("{}\n[needs]\nsound = \"0.2\"\n{toml}", c.files["potluck.toml"]);
        c.manifest = toml::from_str(&text).unwrap_or_else(|e| panic!("{e}\n{text}"));
        c.files.insert("potluck.toml".into(), text);
        c
    };
    // notes by name
    assert_eq!(note("A4"), Some(440.0));
    assert!((note("C5").unwrap() - 523.25).abs() < 0.01 && (note("F#4").unwrap() - 369.99).abs() < 0.01 && (note("Bb3").unwrap() - 233.08).abs() < 0.01);
    assert_eq!((note("-"), note("H2"), note("C12")), (Some(0.0), None, None));

    // they're made the same every time, as loud as the game's, never too long
    let c = with(sounds, "");
    for (name, v) in &c.manifest.sounds {
        let r = recipe(name, v).unwrap();
        let key = format!("test/{name}");
        let s = make_own(&r, &key);
        let peak = s.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        assert!(!s.is_empty() && s.len() <= (OWN_LONGEST * RATE as f32) as usize && peak > 0.2 && peak <= 0.8001 && s.iter().all(|x| x.is_finite()), "{name}: {} samples, peak {peak}", s.len());
        assert_eq!(s, make_own(&r, &key), "{name} is the same every time");
    }
    let tada = recipe("tada", &c.manifest.sounds["tada"]).unwrap();
    assert_eq!((tada.caption.as_str(), tada.layer.notes.len()), ("Tada", 5), "its caption is its name when it gives none");
    assert!((make_own(&tada, "test/tada").len() as f32 / RATE as f32 - 0.6).abs() < 0.01, "five notes of 0.12 s");
    assert_eq!(recipe("quack", &c.manifest.sounds["quack"]).unwrap().layer.length, OWN_LONGEST, "too long is brought down to the most");
    // they're kept through a remix, which writes potluck.toml again
    let again: Manifest = toml::from_str(&toml::to_string_pretty(&c.manifest).unwrap()).unwrap();
    assert_eq!(again.sounds, c.manifest.sounds);

    // played by name, its own first, as "creation/name" with its caption
    let code = r##"
local world = require("@potluck/world@0.10")
local sound = require("@potluck/sound@0.2")
local Cow = world.creature {
	name = "Cow", count = 1, home = "paddock",
	behaviours = { wander = false, chatter = { "Moo." }, sound = "moo" },
	body = { { "body", size = { 1, 1, 1.4 }, at = { 0, 0.6, 0 }, color = "#ffffff" } },
}
world.on("player_joined", function(p)
	sound.play("moo", { at = vector.create(0, 40, 0) })
	sound.play_for(p, "tada")
	sound.play("quack")
	sound.play("chime", { caption = "Ding dong" })
end)
local names = sound.names()
print(names[1] .. " " .. names[2] .. " " .. names[3] .. " " .. #names)
print(pcall(sound.play, "trumpet"))
"##;
    let mut voxels = world();
    let mut herd = Herd::default();
    let mut scripts = Scripts::default();
    scripts.players = vec![PlayerInfo { id: 7, name: "Ada".into(), pos: Vec3::new(1.0, 40.0, 0.0) }];
    scripts.start(&with(sounds, code), &mut herd).unwrap();
    run(&mut scripts, &mut voxels, &mut herd, 0.2);
    let vm = &scripts.vms["test"];
    assert_eq!(vm.error, None, "{:?}", vm.log);
    let log = vm.log.join("\n");
    assert!(log.contains(&format!("moo quack tada {}", 3 + SOUNDS.len())), "its own first: {log}");
    assert!(log.contains("false") && log.contains("[sounds.trumpet]"), "an unknown sound says where to make one: {log}");
    let heard: Vec<(String, Option<String>)> = scripts.sound_changes().into_iter().map(|(_, p)| (p.name, p.caption)).collect();
    let says = |n: &str, c: Option<&str>| (n.to_string(), c.map(String::from));
    for want in [says("test/moo", Some("Moo")), says("test/tada", Some("Tada")), says("test/quack", Some("A duck of its own")), says("chime", Some("Ding dong"))] {
        assert!(heard.contains(&want), "{want:?} in {heard:?}");
    }
    // behaviours make them too, heard by whoever's near the cow
    let cow = herd.bodies.values().find(|b| b.creation == "test").unwrap();
    let (cow, by) = (cow.id, cow.pos);
    scripts.players[0].pos = by + Vec3::new(1.5, 0.0, 0.0);
    let ada = scripts.players[0].clone();
    scripts.event(&mut voxels, &mut herd, cow, "used", &ada);
    assert!(scripts.sound_changes().iter().any(|(_, p)| p.name == "test/moo" && p.caption.as_deref() == Some("Moo")), "the cow moos when it's patted");

    // every game makes them from the library, as they are now
    let mut lib = Library::default();
    let mut c = with(sounds, code);
    lib.list.insert("test".into(), c.clone());
    let moo = own_recipe(&lib, "test/moo").expect("the cow's moo");
    assert_eq!(moo.layers.len(), 1);
    assert!(own_recipe(&lib, "test/nope").is_none() && own_recipe(&lib, "chime").is_none() && own_recipe(&lib, "other/moo").is_none());
    c.manifest.sounds.insert("moo".into(), serde_json::json!({ "wave": "sine", "pitch": 300 }));
    lib.list.insert("test".into(), c);
    assert_ne!(own_recipe(&lib, "test/moo").unwrap(), moo, "a changed recipe is made again");
    assert!(Own::of("test", &BTreeMap::new()).unwrap().resolve("moo").is_none());

    // mistakes say what's wrong, and it doesn't start
    let too_many: String = (0..17).map(|i| format!("[sounds.s{i}]\npitch = 300\n")).collect();
    for (bad, says) in [
        ("[sounds.moo]\npiitch = 200\n", "there's no \"piitch\""),
        ("[sounds.moo]\nwave = \"hum\"\n", "wave is one of sine, triangle, saw, square, noise"),
        ("[sounds.moo]\npitch = \"high\"\n", "pitch is a number of Hz, or [from, to]"),
        ("[sounds.moo]\nnotes = [\"H2\"]\n", "notes are names like"),
        ("[sounds.Moo]\npitch = 200\n", "lowercase letters"),
        ("[sounds.moo]\n[[sounds.moo.layers]]\ncaption = \"no\"\n", "sounds.moo.layers[1]: there's no \"caption\""),
        (too_many.as_str(), "16 sounds of its own at most"),
    ] {
        let mut herd = Herd::default();
        let mut scripts = Scripts::default();
        let e = scripts.start(&with(bad, BOX), &mut herd).err().unwrap_or_default();
        assert!(e.contains(says), "{bad}: {e}");
    }
    let mut herd = Herd::default();
    let e = Scripts::default().start(&with("", &BOX.replace("count = 1,", "count = 1, behaviours = { sound = \"moo\" },")), &mut herd).err().unwrap_or_default();
    assert!(e.contains("no sound called \"moo\""), "{e}");
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
    // and the base creations' own
    home();
    for c in Library::load().list.values().filter(|c| c.origin == Origin::Base) {
        for (name, v) in &c.manifest.sounds {
            let key = format!("{}/{name}", c.id);
            let s = crate::sound::make_own(&crate::sound::recipe(name, v).unwrap(), &key);
            std::fs::write(dir.join(format!("{}-{name}.wav", c.id)), wav(&s)).unwrap();
            all.extend_from_slice(&s);
            all.extend(std::iter::repeat_n(0.0, crate::sound::RATE as usize / 3));
        }
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
        h.sync(Some(&lib), p, None);
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
    halves[0].frame(0.1, &ada, for_ada, None);
    halves[0].frame(0.45, &ada, vec![], None);
    let out = halves[0].take();
    let piece = |text: &str, at: &str| Some(Piece { text: text.into(), at: at.into(), meter: None });
    assert!(out.pieces.contains(&("test".into(), "hi".into(), piece("hi 2 b 2", "bottom"))), "{:?}", out.pieces);
    assert!(out.pieces.contains(&("test".into(), "got".into(), piece("got 30 at 0.55", "right"))), "its own clock: {:?}", out.pieces);
    assert_eq!(out.sounds.len(), 1);
    assert_eq!((out.sounds[0].name.as_str(), out.sounds[0].at), ("ding", Some([1.0, 2.0, 3.0])));

    // a new version starts it afresh, and takes its lines off the screen
    let mut c2 = c.clone();
    c2.files.insert("client.luau".into(), format!("{client}\nprint(\"again\")"));
    lib.list.insert("test".into(), c2.clone());
    halves[0].sync(Some(&lib), &ada, None);
    let out = halves[0].take();
    assert!(out.pieces.contains(&("test".into(), "hi".into(), None)) && out.pieces.contains(&("test".into(), "got".into(), None)), "{:?}", out.pieces);
    assert!(halves[0].list["test"].log.join("\n").contains("again"));
    // so does a new world
    lib.world = 2;
    halves[0].sync(Some(&lib), &ada, None);
    assert_eq!(halves[0].take().notes.len(), 10, "a new half says hello again");

    // a half that fails stops and says why; one that never stops is stopped
    halves[0].frame(0.1, &ada, vec![Note { creation: "test".into(), topic: "boom".into(), value: serde_json::Value::Null }], None);
    assert!(halves[0].list["test"].error.as_deref().unwrap_or("").contains("oops"), "{:?}", halves[0].list["test"].error);
    let mut c3 = c.clone();
    c3.files.insert("client.luau".into(), "while true do end".into());
    lib.list.insert("test".into(), c3);
    halves[0].sync(Some(&lib), &ada, None);
    assert!(halves[0].list["test"].error.as_deref().unwrap_or("").contains("too long"));
    // and leaving the world stops them all
    halves[1].sync(None, &bo, None);
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
    h.sync(Some(&mine), &ada, None);
    assert_eq!(h.list["rodeo-ram"].error, None);
    let note = |topic: &str, value: serde_json::Value| crate::script::Note { creation: "rodeo-ram".into(), topic: topic.into(), value };
    h.frame(0.05, &ada, vec![note("ride", serde_json::json!({ "best": 1.0 }))], None);
    for _ in 0..12 {
        h.frame(0.1, &ada, vec![], None);
    }
    let out = h.take();
    let last = out.pieces.iter().rev().find(|p| p.1 == "ride").and_then(|p| p.2.clone()).expect("a clock");
    assert!(last.text.starts_with("Riding 1.") && last.text.contains("past the record"), "{}", last.text);
    assert_eq!(out.sounds.iter().filter(|s| s.name == "sparkle").count(), 1, "once, as it passes");
    h.frame(0.1, &ada, vec![note("done", serde_json::json!({ "time": 1.3, "record": true }))], None);
    let out = h.take();
    assert_eq!(out.pieces.last().and_then(|p| p.2.clone()).map(|p| p.text), Some("You rode 1.3 s, the longest yet!".into()));
    for _ in 0..41 {
        h.frame(0.1, &ada, vec![], None);
    }
    assert_eq!(h.take().pieces.last().map(|p| p.2.clone()), Some(None), "gone after a while");
    assert_eq!(h.list["rodeo-ram"].error, None);
}

#[test]
fn the_balloon_shows_its_rider_how_high_they_are() {
    use crate::halves::{Halves, Seen};
    home();
    let lib = Library::load();
    let balloon = lib.list["balloon"].clone();
    assert_eq!(balloon.manifest.client.as_deref(), Some("client.luau"));
    let mut mine = Library::default();
    mine.list.insert(balloon.id.clone(), balloon.clone());
    mine.running.push(balloon.id.clone());
    let mut voxels = world();
    let (x, z) = (20, 20);
    let ground = voxels.top(x, z);
    voxels.set(IVec3::new(x, ground, z), crate::blocks::SAND);
    let at = |up: f32| PlayerInfo { id: 7, name: "Ada".into(), pos: Vec3::new(x as f32 + 0.5, ground as f32 + 1.0 + up, z as f32 + 0.5) };
    let seen = |p: &PlayerInfo| Seen { voxels: &voxels, players: vec![p.clone()], look: Vec3::NEG_Z, aim: None };
    let mut h = Halves::default();
    h.sync(Some(&mine), &at(0.0), Some(&seen(&at(0.0))));
    assert_eq!(h.list["balloon"].error, None, "{:?}", h.list["balloon"].log);
    let out = h.take();
    assert!(out.notes.iter().any(|n| n.topic == "hello"), "it asks whether it's aboard: {:?}", out.notes);
    let note = |on: bool| crate::script::Note { creation: "balloon".into(), topic: "aboard".into(), value: serde_json::json!(on) };
    let mut line = String::new();
    for (i, up) in [0.0, 3.0, 8.0, 12.2].into_iter().enumerate() {
        let notes = if i == 0 { vec![note(true)] } else { vec![] };
        h.frame(0.2, &at(up), notes, Some(&seen(&at(up))));
        if let Some(l) = h.take().pieces.iter().rev().find(|p| p.1 == "height").and_then(|p| p.2.clone()) {
            line = l.text;
        }
    }
    assert_eq!(line, "12 m up, rising · over sand");
    h.frame(0.2, &at(4.0), vec![note(false)], Some(&seen(&at(4.0))));
    assert_eq!(h.take().pieces.iter().rev().find(|p| p.1 == "height").map(|p| p.2.clone()), Some(None), "gone once they're out");
    assert_eq!(h.list["balloon"].error, None);
}

#[test]
fn creations_give_and_take_what_survival_players_carry() {
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
	-- a pickaxe comes one to a slot, and they fill the rest
	local picks = 0
	for _ = 1, 40 do
		picks += world.give(7, "Wooden pickaxe")
	end
	print("picks " .. picks .. " then " .. world.give(7, "Stone", 64) .. " stone and " .. world.give(7, "Cobblestone") .. " cobble")
end)
"#;
    let ada = PlayerInfo { id: 7, name: "Ada".into(), pos: Vec3::ZERO };
    let bo = PlayerInfo { id: 8, name: "Bo".into(), pos: Vec3::ZERO };
    let cy = PlayerInfo { id: 9, name: "Cy".into(), pos: Vec3::ZERO };
    scripts.players = vec![ada.clone(), bo.clone(), cy.clone()];
    scripts.carry(7, true, &[(log_id, 3), (SAND, 2)], 0);
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
        "tenth 1 more 3",
        "picks 26 then 64 stone and 0 cobble",
    ] {
        assert!(l.contains(want), "{want}: {l}");
    }
    // the changes go to Ada alone, once
    let changes = scripts.pocket_changes();
    assert_eq!(changes.len(), 1, "{changes:?}");
    let (pid, seq, list) = changes[0].clone();
    assert_eq!((pid, seq), (7, 1));
    let t = |b: crate::blocks::BlockId| crate::items::of_block(b);
    assert_eq!(&list[..3], &[(t(log_id), -2), (t(PLANKS), 8), (t(STONE), 1)]);
    assert!(scripts.pocket_changes().is_empty());
    // a report from before they arrived changes nothing
    scripts.carry(7, true, &[(log_id, 3), (SAND, 2)], 0);
    assert_eq!(scripts.shared.lock().unwrap().pockets[&7].count(t(log_id)), 1);

    // on Ada's game: the pockets take them, as the host worked out, and say so next time
    let mut me = Me::default();
    me.pockets.0[0] = crate::items::Slot { thing: t(log_id), count: 3 };
    me.pockets.0[1] = crate::items::Slot { thing: t(SAND), count: 2 };
    me.change_pockets(&list);
    me.pockets_seen = seq;
    let carried = me.carried();
    let pick = crate::items::WOODEN_PICKAXE;
    assert!(carried.contains(&(t(log_id), 1)) && carried.contains(&(t(PLANKS), 8)) && carried.contains(&(t(STONE), 68)) && carried.contains(&(t(SAND), 2)) && carried.contains(&(pick, 26)), "{carried:?}");
    assert_eq!(me.pockets.filled(), scripts.shared.lock().unwrap().pockets[&7].slots.filled(), "slot for slot as the host has them");
    assert_eq!((me.pockets.room_for(pick), me.pockets.room_for(t(crate::blocks::COBBLE)), me.pockets.room_for(t(STONE))), (0, 0, 31), "full, but for more stone");
    me.change_pockets(&[(t(SAND), -5)]);
    assert!(!me.carried().iter().any(|c| c.0 == t(SAND)), "never fewer than none");
    scripts.set_pockets(7, true, &me.pockets.filled(), me.pockets_seen);
    assert_eq!(scripts.shared.lock().unwrap().pockets[&7].count(t(SAND)), 0, "an up to date report counts");

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
        me.pockets.0[0] = crate::items::Slot { thing: t(log_id), count: 2 };
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
        assert!(me.carried().contains(&(t(crate::blocks::LANTERN), 2)), "{:?}", me.carried());
        assert_eq!(me.pockets_seen, 1);
    }

    // what goes between devices
    for m in [Msg::Pockets { seq: 2, changes: vec![(t(PLANKS), 4), (pick, -1)] }, Msg::Act(Action::Pockets { survival: true, slots: vec![(0, t(PLANKS), 4), (35, pick, 1)], seen: 2 })] {
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
    scripts.carry(7, true, &[(log_id, 2)], 0);
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
    assert_eq!((scripts.carried(7, log_id), scripts.carried(7, PLANKS)), (1, 4));
    // a trade she can't pay for takes nothing
    scripts.event(&mut voxels, &mut herd, rowan, "used", &ada);
    let panel = asked(&mut scripts, 7).unwrap();
    scripts.answer(7, panel.ask, Some(3));
    run(&mut scripts, &mut voxels, &mut herd, 0.2);
    assert_eq!(Some(scripts.carried(7, PLANKS)).filter(|n| *n > 0), Some(4));
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
    assert_eq!(scripts.health_changes(), vec![(7, -3.0 + 1.0 - MAX_HEALTH, 0.0)]);
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
    // bop one twice and it pops, the first bop knocking it back
    let ids: Vec<_> = herd.bodies.keys().copied().collect();
    let id = ids[0];
    let ada = scripts.players[0].clone();
    scripts.event(&mut voxels, &mut herd, id, "hit", &ada);
    assert!(herd.bodies.contains_key(&id), "one bop by hand only knocks it back");
    scripts.event(&mut voxels, &mut herd, id, "hit", &ada);
    assert!(!herd.bodies.contains_key(&id));
    // a sword pops one at once (world.held)
    if let Some(&other) = ids.get(1) {
        scripts.set_held(7, crate::items::by_name("Wooden sword").unwrap());
        scripts.event(&mut voxels, &mut herd, other, "hit", &ada);
        assert!(!herd.bodies.contains_key(&other), "a sword pops it");
        scripts.set_held(7, 0);
    }
    // and a shot as hard as its power (world 0.19): an arrow at 2 pops one
    if let Some(&third) = ids.get(2) {
        let fly = crate::shots::Flying { id: 1, look: 0, pos: Vec3::ZERO, vel: Vec3::X, drop: 0.0, age: 0.0, stuck: false };
        let arrow = crate::script::Shot { fly, creation: "ranged".into(), shooter: 7, by: None, power: 2.0 };
        scripts.event_with(&mut voxels, &mut herd, third, "hit", &ada, Some(&arrow));
        assert!(!herd.bodies.contains_key(&third), "an arrow pops it");
    }
    // in the morning they fade away
    scripts.set_day(0.4);
    run(&mut scripts, &mut voxels, &mut herd, 15.0);
    assert_eq!(count(&herd), 0);
    // and creative players have nothing to fear
    voxels.set(IVec3::new(cx + 1, ground + 3, cz), crate::blocks::AIR);
    // (her game has had the gloom dust the popped ones left)
    scripts.pocket_changes();
    scripts.set_pockets(7, false, &[], 1000);
    scripts.set_day(0.0);
    run(&mut scripts, &mut voxels, &mut herd, 10.0);
    assert_eq!(count(&herd), 0);
    assert_eq!(scripts.vms["gloomlings"].error, None, "{}", log(&scripts));
}

const SHOOTS: &str = r##"
local world = require("@potluck/world@0.19")
local Target = world.creature {
	name = "Target", count = 1, speed = 0, home = { 60, 41, 60 },
	body = { { "box", size = { 1, 2, 1 }, at = { 0, 1, 0 }, color = "#ffffff" } },
}
function Target:hit(player, shot)
	print(string.format("hit by %s %s %.0f", player.name, shot and shot.look or "hand", shot and shot.power or 0))
end
world.aims("Bow")
world.on("item_used", function(player, item, aim)
	local id = world.shoot(player, { aim = aim, look = "arrow", speed = 30, drop = 0, power = 2 }, function(at, block, creature)
		print(string.format("landed %s %s %.0f", block or "-", if creature then "creature" else "-", at.y))
	end)
	print("shot " .. tostring(id))
end)
"##;

/// world 0.19: a shot flies from the player the way they aim, hits the
/// first creature or block in its way, and the creature hears `hit` with
/// the shot; the creation hears where it landed, and every game is told.
#[test]
fn shots_fly_hit_what_is_in_their_way_and_land() {
    use crate::script::ShotNews;
    home();
    let mut voxels = world();
    // a stone floor at 40 with air over it, and a wall to the south
    for x in 30..=90 {
        for z in 30..=90 {
            for y in 1..world::WY {
                let b = if y <= 40 || (z == 70 && y <= 50) { crate::blocks::STONE } else { crate::blocks::AIR };
                voxels.set(IVec3::new(x, y, z), b);
            }
        }
    }
    let mut herd = Herd::default();
    let mut scripts = Scripts::default();
    let ada = Vec3::new(50.5, 41.0, 60.5);
    scripts.players = vec![PlayerInfo { id: 7, name: "Ada".into(), pos: ada }];
    scripts.set_pockets(7, true, &[], 0);
    scripts.start(&creation(SHOOTS), &mut herd).unwrap();
    run(&mut scripts, &mut voxels, &mut herd, 2.0);
    let log = |scripts: &Scripts| scripts.vms["test"].log.join("\n");
    assert_eq!(scripts.vms["test"].error, None, "{}", log(&scripts));
    let target = herd.bodies.values().find(|b| b.kind == "Target").expect("the target").clone();
    // at the target: it's hit
    scripts.shot_news.clear();
    scripts.use_item(7, target.pos + Vec3::Y - (ada + Vec3::Y * 1.5));
    run(&mut scripts, &mut voxels, &mut herd, 1.0);
    let l = log(&scripts);
    assert!(l.contains("shot 1") && l.contains("hit by Ada arrow 2") && l.contains("landed - creature"), "{l}");
    assert!(matches!(scripts.shot_news.as_slice(), [ShotNews::Flew(f), ShotNews::Gone(1)] if f.id == 1 && f.look == 0), "{:?}", scripts.shot_news);
    // at the wall: it lands on the stone
    scripts.use_item(7, Vec3::Z);
    run(&mut scripts, &mut voxels, &mut herd, 1.0);
    assert!(log(&scripts).contains("landed Stone -"), "{}", log(&scripts));
    // straight up: it flies its time and comes down on nothing
    scripts.use_item(7, Vec3::Y);
    run(&mut scripts, &mut voxels, &mut herd, crate::shots::LIFE + 1.0);
    assert!(log(&scripts).contains("landed - -"), "{}", log(&scripts));
    assert!(scripts.shots.is_empty());
    // a bow in hand is always aimed; a sword isn't
    scripts.set_held(7, crate::items::BOW);
    assert!(scripts.aims(7));
    scripts.set_held(7, crate::items::by_name("Wooden sword").unwrap());
    assert!(!scripts.aims(7));
    assert_eq!(scripts.vms["test"].error, None, "{}", log(&scripts));
    // the things' numbers are where their names say
    assert_eq!(crate::items::by_name("Bow"), Some(crate::items::BOW));
    assert_eq!(crate::items::by_name("Spear"), Some(crate::items::SPEAR));
    assert_eq!(crate::items::by_name("Shield"), Some(crate::items::SHIELD));
    assert_eq!(crate::items::by_name("Wool cap"), Some(crate::items::WOOL_CAP));
    assert_eq!(crate::items::by_name("Shears"), Some(crate::items::SHEARS));
    assert_eq!(crate::items::by_name("Cooked fish"), Some(crate::items::COOKED_FISH));
}

const THROWS: &str = r##"
local world = require("@potluck/world@0.21")
local Imp = world.creature {
	name = "Imp", count = 1, speed = 0, home = { 60, 41, 50 }, behaviours = { wander = false },
	body = { { "box", size = { 0.6, 1, 0.6 }, at = { 0, 0.5, 0 }, color = "#ff6020" } },
}
local Roamer = world.creature {
	name = "Roamer", count = 0, home = "here",
	body = { { "box", size = { 0.6, 1, 0.6 }, at = { 0, 0.5, 0 }, color = "#20ff60" } },
}
print("danger " .. world.danger())
world.on("player_fell", function(player, at)
	print(string.format("fell %s %.0f %.0f", player.name, at.x, at.z))
end)
world.after(0.5, function()
	print("roamer " .. tostring(world.spawn(Roamer, vector.create(80.5, 41, 80.5))))
	for _, id in world.creatures(Imp) do
		print("imp " .. id)
	end
end)
function Imp:tick(dt)
	if self.thrown then return end
	local p = world.nearest_player(self.position, 30)
	if p then
		self.thrown = true
		local id = self:throw({ at = p.position + vector.create(0, 1, 0), look = "ember", speed = 20, drop = 0 }, function(at, block, creature, player)
			print(string.format("ember %s %s %s", block or "-", if creature then "creature" else "-", if player then player.name else "-"))
		end)
		print("threw " .. tostring(id))
	end
end
"##;

/// world 0.21: a creature throws at a player and hits them (not itself),
/// the world's danger reads as its name, a player hurt too badly is heard
/// as `player_fell`, and `home = "here"` keeps a creature where it spawned.
#[test]
fn creatures_throw_at_players_and_danger_and_falling_are_heard() {
    use crate::script::Danger;
    home();
    let mut voxels = world();
    for x in 30..=90 {
        for z in 30..=90 {
            for y in 1..world::WY {
                voxels.set(IVec3::new(x, y, z), if y <= 40 { crate::blocks::STONE } else { crate::blocks::AIR });
            }
        }
    }
    let mut herd = Herd::default();
    let mut scripts = Scripts::default();
    scripts.set_danger(Danger::Hard);
    let ada = Vec3::new(60.5, 41.0, 60.5);
    scripts.players = vec![PlayerInfo { id: 7, name: "Ada".into(), pos: ada }];
    scripts.set_pockets(7, true, &[], 0);
    scripts.start(&creation(THROWS), &mut herd).unwrap();
    run(&mut scripts, &mut voxels, &mut herd, 2.0);
    let log = |scripts: &Scripts| scripts.vms["test"].log.join("\n");
    assert_eq!(scripts.vms["test"].error, None, "{}", log(&scripts));
    let l = log(&scripts);
    assert!(l.contains("danger hard"), "{l}");
    assert!(l.contains("threw 1") && l.contains("ember - - Ada"), "the ember hits Ada, not the imp: {l}");
    scripts.fell(7, Vec3::new(61.0, 41.0, 59.0));
    run(&mut scripts, &mut voxels, &mut herd, 0.3);
    assert!(log(&scripts).contains("fell Ada 61 59"), "{}", log(&scripts));
    // a roamer's home is where it was spawned, far from the Green
    let roamer = herd.bodies.values().find(|b| b.kind == "Roamer").expect("a roamer");
    assert!(roamer.home.distance(Vec3::new(80.5, 41.0, 80.5)) < 0.01, "{:?}", roamer.home);
    // names and numbers
    for d in Danger::ALL {
        assert_eq!(Danger::parse(d.as_str()), Some(d));
    }
    assert_eq!(Danger::default(), Danger::Gentle);
    assert_eq!(Danger::Hard.next(), Danger::Peaceful);
    assert_eq!(crate::items::by_name("Silk"), Some(crate::items::SILK));
    assert_eq!(crate::items::by_name("Gloom dust"), Some(crate::items::GLOOM_DUST));
    assert_eq!(crate::items::by_name("Leather cap"), Some(crate::items::LEATHER_CAP));
    assert_eq!(crate::items::by_name("Leather boots"), Some(crate::items::LEATHER_BOOTS));
}

/// world 0.18: armor's share comes off a hurt as it's given, unless the
/// hurt says it isn't guarded, and shares multiply.
#[test]
fn guarded_hurts_lose_armors_share() {
    home();
    let mut voxels = world();
    let mut herd = Herd::default();
    let mut scripts = Scripts::default();
    scripts.players = vec![PlayerInfo { id: 7, name: "Ada".into(), pos: Vec3::ZERO }];
    scripts.set_pockets(7, true, &[], 0);
    let code = r#"
local world = require("@potluck/world@0.18")
world.guard(7, 0.5)
world.after(0.1, function() world.hurt(7, 4) world.hurt(7, 4, { guarded = false }) end)
"#;
    scripts.start(&creation(code), &mut herd).unwrap();
    run(&mut scripts, &mut voxels, &mut herd, 0.3);
    assert_eq!(scripts.vms["test"].error, None, "{:?}", scripts.vms["test"].log);
    let hurt: f32 = scripts.health_changes().iter().filter(|c| c.0 == 7).map(|c| c.1).sum();
    assert_eq!(hurt, -6.0, "half of 4, and all of 4");
}

/// world 0.24: what a player is seen wearing, by slot, for every game to
/// draw.
#[test]
fn players_are_seen_wearing_what_creations_say() {
    home();
    let mut voxels = world();
    let mut herd = Herd::default();
    let mut scripts = Scripts::default();
    scripts.players = vec![PlayerInfo { id: 7, name: "Ada".into(), pos: Vec3::ZERO }];
    scripts.set_pockets(7, true, &[], 0);
    let code = r#"
local world = require("@potluck/world@0.24")
assert(world.wear(7, "head", "Iron helmet"))
assert(world.wear(7, "arm", "Shield"))
assert(not world.wear(9, "head", "Iron helmet"), "nobody's there")
assert(not pcall(function() world.wear(7, "tail", "Shield") end))
assert(not pcall(function() world.wear(7, "head", "Hat of wonder") end))
world.after(0.2, function() world.wear(7, "arm", nil) end)
"#;
    scripts.start(&creation(code), &mut herd).unwrap();
    assert_eq!(scripts.vms["test"].error, None, "{:?}", scripts.vms["test"].log);
    let helmet = crate::items::by_name("Iron helmet").unwrap();
    assert_eq!(scripts.worn(7), [helmet, 0, 0, 0, crate::items::by_name("Shield").unwrap()]);
    assert_eq!(scripts.worn(8), [0; 5]);
    run(&mut scripts, &mut voxels, &mut herd, 0.4);
    assert_eq!(scripts.worn(7), [helmet, 0, 0, 0, 0], "the shield's off");
    // and gone when they leave
    scripts.players.clear();
    run(&mut scripts, &mut voxels, &mut herd, 0.2);
    assert_eq!(scripts.worn(7), [0; 5]);
}

/// world 0.22: XP is kept by name, given only in survival, a little at a
/// time from each creation, and spent; the levels it makes; haste and might
/// multiply and go when their creation stops, and charms stay.
#[test]
fn xp_rewards_haste_and_charms() {
    use crate::script::{xp_for, xp_level};
    assert_eq!((xp_for(0), xp_for(1), xp_for(2), xp_for(10)), (0, 10, 30, 550));
    assert_eq!(xp_level(0), (0, 0.0));
    assert_eq!(xp_level(9).0, 0);
    assert_eq!(xp_level(10), (1, 0.0));
    assert_eq!(xp_level(20), (1, 0.5));
    assert_eq!((xp_level(549).0, xp_level(550).0), (9, 10));
    for xp in (0..200_000).step_by(37) {
        let (l, f) = xp_level(xp);
        assert!(xp_for(l) <= xp && xp < xp_for(l + 1) && (0.0..1.0).contains(&f), "{xp}: {l} {f}");
        assert_eq!(xp_level(xp_for(l)).0, l);
    }
    assert_eq!(xp_level(u32::MAX).0, xp_level(u32::MAX - 1).0, "no overflow at the top");
    home();
    let mut voxels = world();
    let mut herd = Herd::default();
    let mut scripts = Scripts::default();
    scripts.players = vec![PlayerInfo { id: 7, name: "Ada".into(), pos: Vec3::ZERO }, PlayerInfo { id: 8, name: "Bo".into(), pos: Vec3::ZERO }];
    scripts.set_pockets(7, true, &[], 0);
    scripts.set_pockets(8, false, &[], 0);
    let code = r#"
local world = require("@potluck/world@0.22")
world.after(0.1, function()
	print("first " .. world.reward(7, 100) .. " " .. world.reward(7, 50) .. " bo " .. world.reward(8, 10))
	print(pcall(world.reward, 7, 101) and "too many ok" or "too many no")
	world.haste(7, 2)
	world.might(7, 1.5)
	print(pcall(world.haste, 7, 5) and "fast ok" or "fast no")
	world.charm(7, "Iron pickaxe", { "Swift", "Sturdy" })
	print(pcall(world.charm, 7, "Nothing at all", { "Swift" }) and "nothing ok" or "nothing no")
end)
world.after(0.6, function()
	print("soon " .. world.reward(7, 50))
end)
world.after(40.6, function()
	local xp, level = world.xp(7)
	print("later " .. world.reward(7, 50) .. " had " .. xp .. " level " .. level)
	print("spend " .. tostring(world.spend_xp(7, 5000)) .. " " .. tostring(world.spend_xp(7, 20)) .. " " .. world.xp(7))
	print("might " .. world.might(7) .. " haste " .. world.haste(7) .. " charms " .. table.concat(world.charms(7, "Iron pickaxe"), ","))
end)
"#;
    scripts.start(&creation(code), &mut herd).unwrap();
    run(&mut scripts, &mut voxels, &mut herd, 0.3);
    let log = scripts.vms["test"].log.clone();
    assert!(log.iter().any(|l| l == "first 100 20 bo 0"), "120 at once, none in creative: {log:?}");
    assert!(log.iter().any(|l| l == "too many no") && log.iter().any(|l| l == "fast no") && log.iter().any(|l| l == "nothing no"), "{log:?}");
    assert_eq!(scripts.haste_changes(), vec![(7, 2.0)]);
    assert!(scripts.xp_changes().contains(&(7, 120)));
    run(&mut scripts, &mut voxels, &mut herd, 41.0);
    assert_eq!(scripts.vms["test"].error, None, "{:?}", scripts.vms["test"].log);
    let log = scripts.vms["test"].log.clone();
    let soon: u32 = log.iter().find_map(|l| l.strip_prefix("soon ")).and_then(|n| n.parse().ok()).expect("soon");
    assert!((1..=2).contains(&soon), "4 a second after the first 120: {soon}");
    let had = 120 + soon;
    let level = xp_level(had).0;
    assert!(log.iter().any(|l| *l == format!("later 50 had {had} level {level}")), "and 120 again after a long while: {log:?}");
    assert!(log.iter().any(|l| *l == format!("spend false true {}", had + 50 - 20)), "{log:?}");
    assert!(log.iter().any(|l| l == "might 1.5 haste 2 charms Swift,Sturdy"), "{log:?}");
    assert_eq!(scripts.shared.lock().unwrap().store.xp.get("Ada"), Some(&(had + 30)), "kept by name");
    // stopped, its haste and might go, and the charms and XP stay
    scripts.stop("test", &mut herd);
    assert_eq!(scripts.haste_changes(), vec![(7, 1.0)]);
    let sh = scripts.shared.lock().unwrap();
    assert!(sh.might.is_empty());
    assert_eq!(sh.store.charms["Ada"]["Iron pickaxe"], ["Swift", "Sturdy"]);
    assert_eq!(sh.store.xp["Ada"], had + 30);
    // and the store keeps them when it's saved
    let json = serde_json::to_string(&sh.store).unwrap();
    let back: crate::script::Store = serde_json::from_str(&json).unwrap();
    assert_eq!((back.xp["Ada"], back.charms["Ada"]["Iron pickaxe"].len()), (had + 30, 2));
}

const HEARS_BLOCKS: &str = r#"
local world = require("@potluck/world@0.5")
for _, e in { "block_placed", "block_broken", "block_used" } do
	world.on(e, function(player, at, block)
		print(string.format("%s %s %s %.1f %.1f %.1f", e, player.name, block, at.x, at.y, at.z))
	end)
end
"#;

/// world 0.5: creations hear players place, break and use blocks, from the
/// host's own player, from Interact (within reach) and from a friend over
/// the network.
#[test]
fn creations_hear_players_place_break_and_use_blocks() {
    use crate::blocks::{by_name, AIR, PLANKS, STONE, WATER};
    use crate::game::{Action, Actions};
    use crate::net::{self, decode, frame_msg, Event, Msg};
    use crate::player::Me;
    use bevy::ecs::system::RunSystemOnce;
    home();
    let table = by_name("Table").unwrap();
    let mut voxels = world();
    let mut herd = Herd::default();
    let mut scripts = Scripts::default();
    scripts.players = vec![PlayerInfo { id: 7, name: "Ada".into(), pos: Vec3::ZERO }];
    scripts.start(&creation(HEARS_BLOCKS), &mut herd).unwrap();
    let at = IVec3::new(10, 50, 12);
    scripts.player_block(7, at, AIR, PLANKS);
    scripts.player_block(7, at, PLANKS, AIR);
    scripts.player_block(7, at, STONE, WATER);
    scripts.use_block(7, at, table);
    // not events: nothing changed, water running in or out, using air, someone who isn't here
    scripts.player_block(7, at, PLANKS, PLANKS);
    scripts.player_block(7, at, WATER, AIR);
    scripts.player_block(7, at, AIR, WATER);
    scripts.use_block(7, at, AIR);
    scripts.player_block(9, at, AIR, PLANKS);
    run(&mut scripts, &mut voxels, &mut herd, 0.3);
    let log = scripts.vms["test"].log.clone();
    assert_eq!(scripts.vms["test"].error, None, "{log:?}");
    let heard: Vec<&String> = log.iter().filter(|l| l.contains("block_")).collect();
    let want = ["block_placed Ada Planks 10.5 50.5 12.5", "block_broken Ada Planks 10.5 50.5 12.5", "block_broken Ada Stone 10.5 50.5 12.5", "block_used Ada Table 10.5 50.5 12.5"];
    assert_eq!(heard.len(), want.len(), "{heard:?}");
    for (h, w) in heard.iter().zip(want) {
        assert!(h.contains(w), "{w}: {heard:?}");
    }

    // a flood of them waits its turn, 64 a tick, and a few hundred at most wait
    let mut scripts = Scripts::default();
    scripts.players = vec![PlayerInfo { id: 7, name: "Ada".into(), pos: Vec3::ZERO }];
    let count = "local world = require(\"@potluck/world@0.5\")\nlocal n = 0\nworld.on(\"block_placed\", function() n += 1 if n % 64 == 0 then print(\"heard \" .. n) end end)";
    scripts.start(&creation(count), &mut herd).unwrap();
    for i in 0..600 {
        scripts.player_block(7, at + IVec3::X * i, AIR, PLANKS);
    }
    scripts.tick(&mut voxels, &mut herd, 0.1);
    let log = scripts.vms["test"].log.join("\n");
    assert!(log.contains("heard 64") && !log.contains("heard 128"), "{log}");
    run(&mut scripts, &mut voxels, &mut herd, 1.5);
    let log = scripts.vms["test"].log.join("\n");
    assert!(log.contains("heard 512") && !log.contains("heard 576"), "{log}");

    // the host's own player: blocks they built, and Interact on a block near them (not far off)
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    crate::sim(&mut app, crate::SEED);
    app.insert_resource(crate::settings::Settings { name: "Ken".into(), ..default() }).insert_resource(Me::default());
    app.world_mut().resource_scope(|w, mut s: Mut<Scripts>| {
        let mut herd = w.resource_mut::<Herd>();
        s.start(&creation(HEARS_BLOCKS), &mut herd).unwrap();
    });
    let me = app.world().resource::<Me>().pos.floor().as_ivec3();
    let (near, far) = (me + IVec3::new(2, 0, 0), me + IVec3::new(40, 0, 0));
    app.world_mut().resource_mut::<Voxels>().set(near, table);
    app.world_mut().resource_mut::<Voxels>().set(far, table);
    let my_id = app.world().resource::<net::Net>().my_id;
    app.world_mut().resource_mut::<Me>().built.push((me + IVec3::Y * 3, AIR, PLANKS));
    app.world_mut().resource_mut::<Actions>().0.extend([(my_id, Action::UseBlock(near.to_array())), (my_id, Action::UseBlock(far.to_array()))]);
    app.world_mut().run_system_once(crate::game::host_step).unwrap();
    assert!(app.world().resource::<Me>().built.is_empty(), "the host has heard them");
    app.world_mut().resource_scope(|w, mut s: Mut<Scripts>| {
        w.resource_scope(|w, mut v: Mut<Voxels>| {
            let mut h = w.resource_mut::<Herd>();
            s.tick(&mut v, &mut h, 0.2);
        })
    });
    let log = app.world().resource::<Scripts>().vms["test"].log.join("\n");
    assert!(log.contains("block_placed Ken Planks"), "{log}");
    assert_eq!(log.matches("block_used Ken Table").count(), 1, "only the one in reach: {log}");
    assert!(log.contains(&format!("block_used Ken Table {:.1}", near.x as f32 + 0.5)), "{log}");

    // what goes between devices
    let m = Msg::Act(Action::UseBlock([1, -2, 3]));
    let Some(Event::Msg(_, back)) = decode(transport::Wire::Data(1, frame_msg(&m))) else { panic!("didn't decode") };
    assert_eq!(serde_json::to_string(&back).unwrap(), serde_json::to_string(&m).unwrap());

    // a friend building in a hosted world
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    crate::sim(&mut app, crate::SEED);
    app.insert_resource(crate::settings::Settings { name: "Ken's world".into(), ..Default::default() });
    app.update();
    app.world_mut().resource_scope(|w, mut s: Mut<Scripts>| {
        let mut herd = w.resource_mut::<Herd>();
        s.start(&creation(HEARS_BLOCKS), &mut herd).unwrap();
    });
    let port = free_port();
    app.world_mut().resource_mut::<net::Net>().host_with(net::HostOptions { name: "Ken's world".into(), port, tls: None, beacon: false, rules: Default::default() }).unwrap();
    let (tx, rx) = channel();
    let to_host = transport::connect(&format!("127.0.0.1:{port}"), port, tx).unwrap();
    to_host.send(net::frame_msg(&Msg::Hello { name: "Drew".into(), protocol: net::PROTOCOL, pass: None, young: false })).unwrap();
    let spot = IVec3::new(world::GREEN.x as i32 + 3, 100, world::GREEN.z as i32 + 3);
    let (mut welcomed, mut heard) = (false, false);
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(20) && !heard {
        app.update();
        std::thread::sleep(Duration::from_millis(10));
        for w in rx.try_iter() {
            if let Some(Event::Msg(_, Msg::Welcome { .. })) = net::decode(w) {
                welcomed = true;
                to_host.send(net::frame_msg(&Msg::Blocks(vec![(spot.x, spot.y, spot.z, PLANKS)]))).unwrap();
            }
        }
        heard = app.world().resource::<Scripts>().vms["test"].log.iter().any(|l| l.contains("block_placed Drew Planks"));
    }
    assert!(welcomed, "Drew's in");
    assert!(heard, "{:?}", app.world().resource::<Scripts>().vms["test"].log);
}

/// The Workbench: Interact on any table to make things from what you carry.
#[test]
fn a_table_is_a_workbench_for_survival_players() {
    use crate::blocks::{by_name, PLANKS, STONE};
    home();
    let mut lib = Library::load().list;
    let bench = lib.remove("workbench").expect("the base game has the workbench");
    // the circuit parts are there when the Circuits creation's blocks are
    let circuits = lib.remove("circuits").expect("the base game has circuits");
    crate::made::register(&circuits.id, &circuits.manifest.blocks).unwrap();
    // and the anvil when the Tools creation's is
    let tools = lib.remove("tools").expect("the base game has tools");
    crate::made::register(&tools.id, &tools.manifest.blocks).unwrap();
    // and the charm table when the Charms creation's is
    let charms = lib.remove("charms").expect("the base game has charms");
    crate::made::register(&charms.id, &charms.manifest.blocks).unwrap();
    // and rails, carts, dispensers and chutes when Rails' and Storage's are
    for id in ["rails", "storage"] {
        let c = lib.remove(id).expect("a base creation");
        crate::made::register(&c.id, &c.manifest.blocks).unwrap();
    }
    let b = |n: &str| by_name(n).unwrap();
    let (table, oak, glass, cobble) = (b("Table"), b("Oak log"), b("Glass"), b("Cobblestone"));
    let mut voxels = world();
    let mut herd = Herd::default();
    let mut scripts = Scripts::default();
    scripts.players = vec![PlayerInfo { id: 7, name: "Ada".into(), pos: Vec3::ZERO }, PlayerInfo { id: 8, name: "Bo".into(), pos: Vec3::ZERO }];
    scripts.carry(7, true, &[(oak, 2), (glass, 1), (cobble, 2)], 0);
    scripts.set_pockets(8, false, &[], 0);
    scripts.start(&bench, &mut herd).unwrap();
    let at = IVec3::new(5, 60, 5);
    let asked = |scripts: &Scripts, pid: u32| scripts.shared.lock().unwrap().ui.panels.get(&pid).and_then(|q| q.front().cloned());
    let mut answer = |scripts: &mut Scripts, voxels: &mut Voxels, herd: &mut Herd, choice: Option<u32>| {
        let panel = asked(scripts, 7).expect("a question for Ada");
        scripts.answer(7, panel.ask, choice);
        run(scripts, voxels, herd, 0.2);
        asked(scripts, 7)
    };
    // anything but a table is just a block
    scripts.use_block(7, at, STONE);
    run(&mut scripts, &mut voxels, &mut herd, 0.2);
    assert!(asked(&scripts, 7).is_none());
    scripts.use_block(7, at, table);
    run(&mut scripts, &mut voxels, &mut herd, 0.2);
    let panel = asked(&scripts, 7).expect("the table asks what to make");
    assert_eq!(panel.choices, ["Building", "Furniture", "Lights", "Garden and farm", "Tools", "Weapons and armor", "Travel", "Circuits"]);
    let panel = answer(&mut scripts, &mut voxels, &mut herd, Some(0)).unwrap();
    assert_eq!(panel.choices[0], "4 Planks for 1 Oak log", "{:?}", panel.choices);
    // it asks again with how it went, so making several is quick
    let panel = answer(&mut scripts, &mut voxels, &mut herd, Some(0)).expect("asks again");
    assert!(panel.text.starts_with("Made 4 Planks."), "{}", panel.text);
    assert_eq!((scripts.carried(7, oak), scripts.carried(7, PLANKS)), (1, 4));
    // closing it and using the table again opens it at once
    assert!(answer(&mut scripts, &mut voxels, &mut herd, None).is_none());
    scripts.use_block(7, at, table);
    run(&mut scripts, &mut voxels, &mut herd, 0.2);
    assert!(asked(&scripts, 7).is_some(), "using the table again asks again");
    // a choice too long to fit says what it takes in the question
    let panel = answer(&mut scripts, &mut voxels, &mut herd, Some(2)).unwrap();
    assert!(panel.choices[0].contains("Torch"), "torches come first: {:?}", panel.choices);
    assert_eq!(panel.choices[2], "1 Lantern", "{:?}", panel.choices);
    assert!(panel.text.contains("1 Lantern takes 1 Glass and 2 Cobblestone."), "{}", panel.text);
    let panel = answer(&mut scripts, &mut voxels, &mut herd, Some(2)).unwrap();
    assert!(panel.text.starts_with("Made 1 Lantern."), "{}", panel.text);
    assert_eq!((scripts.carried(7, b("Lantern")), scripts.carried(7, glass), scripts.carried(7, cobble)), (1, 0, 0));
    // one she can't pay for takes nothing
    let panel = answer(&mut scripts, &mut voxels, &mut herd, Some(3)).unwrap();
    assert!(panel.text.starts_with("You'll need 1 Oak log and 4 Cobblestone for that."), "{}", panel.text);
    assert_eq!(scripts.carried(7, oak), 1);
    answer(&mut scripts, &mut voxels, &mut herd, None);
    // in creative there's nothing to make
    scripts.use_block(8, at, table);
    run(&mut scripts, &mut voxels, &mut herd, 0.2);
    let panel = asked(&scripts, 8).unwrap();
    assert!(panel.text.contains("creative"), "{}", panel.text);

    // every recipe makes what it says, with enough to hand
    // (the kiln and paper lanterns are there when Brickworks is)
    let inputs = [
        vec!["Oak log", "Birch log", "Sand", "Stone", "Cobblestone", "Copper ingot", "Iron ingot", "Silk"],
        vec!["Planks", "Iron ingot", "Gold ingot", "Crystal"],
        vec!["Coal", "Planks", "Hay", "Glass", "Cobblestone", "Oak log", "Candle", "Cream wool"],
        vec!["Planks", "Dirt", "Wheat", "Copper ingot", "Hay"],
        vec!["Planks", "Cobblestone", "Flint", "Iron ingot", "Copper ingot", "Cream wool", "Silk"],
        vec!["Planks", "Cobblestone", "Copper ingot", "Iron ingot", "Cream wool", "Flint", "Feather", "Silk", "Bristles", "Leather"],
        vec!["Cream wool", "Planks", "Lantern", "Iron ingot", "Gold ingot", "Copper wire"],
        vec!["Copper ingot", "Cobblestone", "Planks", "Stone", "Glass", "Crystal", "Copper wire", "Torch", "Coal", "Iron ingot", "Crate"],
    ];
    for (kind, want) in inputs.iter().enumerate() {
        scripts.pocket_changes();
        let carry: Vec<_> = want.iter().map(|n| (crate::items::by_name(n).unwrap(), 64)).collect();
        scripts.carry(7, true, &carry, 1000);
        scripts.use_block(7, at, table);
        run(&mut scripts, &mut voxels, &mut herd, 0.2);
        let mut panel = answer(&mut scripts, &mut voxels, &mut herd, Some(kind as u32)).unwrap();
        // a long list comes in pages, with More last, back round to the first
        loop {
            assert!(panel.choices.iter().all(|c| c.chars().count() <= 30), "{:?}", panel.choices);
            assert!(panel.choices.len() <= 8 && panel.title.chars().count() <= 40, "{}: {:?}", panel.title, panel.choices);
            let mut next = None;
            for i in 0..panel.choices.len() {
                let after = answer(&mut scripts, &mut voxels, &mut herd, Some(i as u32)).unwrap_or_else(|| panic!("{} {i}: {:?} {:?}", panel.title, scripts.vms["workbench"].error, scripts.vms["workbench"].log));
                if panel.choices[i] == "More" {
                    next = Some(after);
                    break;
                }
                assert!(after.text.starts_with("Made"), "{} {i}: {}", panel.title, after.text);
            }
            match next {
                Some(p) if !p.title.contains("(1 of") => panel = p,
                _ => break,
            }
        }
        answer(&mut scripts, &mut voxels, &mut herd, None);
    }
    assert_eq!(scripts.vms["workbench"].error, None, "{:?}", scripts.vms["workbench"].log);
}

/// Farming: dig farmland, plant, wait, harvest more than you sowed.
#[test]
fn crops_planted_on_farmland_grow_and_give_more_back() {
    use crate::blocks::{by_name, AIR, WATER};
    home();
    let mut farming = Library::load().list.remove("farming").expect("the base game has farming");
    farming.knobs.insert("minutes".into(), KnobValue::Number(1.0));
    let b = |n: &str| by_name(n).unwrap();
    let (grass, soil, wheat, carrots) = (b("Grass"), b("Farmland"), b("Wheat"), b("Carrots"));
    let seeds = crate::items::by_name("Seeds").unwrap();
    let mut voxels = world();
    let mut herd = Herd::default();
    let mut scripts = Scripts::default();
    scripts.players = vec![PlayerInfo { id: 7, name: "Ada".into(), pos: Vec3::ZERO }, PlayerInfo { id: 8, name: "Bo".into(), pos: Vec3::ZERO }];
    scripts.carry(7, true, &[(wheat, 3), (carrots, 1)], 0);
    scripts.set_pockets(8, false, &[], 0);
    scripts.set_day(0.5);
    scripts.start(&farming, &mut herd).unwrap();
    // its crops grow as its own shoots
    let (shoot, fern) = (b("Wheat shoots"), b("Carrot shoots"));
    // a strip of grass up in the air, dry but for water by its east end
    let at = |i: i32| IVec3::new(40 + i * 3, 110, 40);
    for i in 0..5 {
        voxels.set(at(i), grass);
    }
    voxels.set(at(4) + IVec3::X * 2, WATER);
    let up = IVec3::Y;
    let carried = |scripts: &Scripts, block| scripts.carried(7, block);
    let plant = |scripts: &mut Scripts, voxels: &mut Voxels, i: i32, crop| {
        voxels.set(at(i) + up, crop);
        scripts.player_block(7, at(i) + up, AIR, crop);
    };
    let harvest = |scripts: &mut Scripts, voxels: &mut Voxels, i: i32| {
        let was = voxels.at(at(i) + up);
        voxels.set(at(i) + up, AIR);
        scripts.player_block(7, at(i) + up, was, AIR);
    };

    // Interact digs farmland, in survival only
    for i in 0..5 {
        scripts.use_block(7, at(i), grass);
    }
    scripts.use_block(8, at(0) + IVec3::Z, grass);
    run(&mut scripts, &mut voxels, &mut herd, 0.2);
    assert!((0..5).all(|i| voxels.at(at(i)) == soil), "farmland");
    // planting turns a crop into its shoots; wheat on grass is just wheat
    plant(&mut scripts, &mut voxels, 0, wheat);
    plant(&mut scripts, &mut voxels, 1, wheat);
    plant(&mut scripts, &mut voxels, 4, carrots);
    voxels.set(at(0) + IVec3::Z * 2, grass);
    voxels.set(at(0) + IVec3::Z * 2 + up, wheat);
    scripts.player_block(7, at(0) + IVec3::Z * 2 + up, AIR, wheat);
    run(&mut scripts, &mut voxels, &mut herd, 0.5);
    assert_eq!([0, 1, 4].map(|i| voxels.at(at(i) + up)), [shoot, shoot, fern]);
    assert_eq!(voxels.at(at(0) + IVec3::Z * 2 + up), wheat);
    // a shoot broken too soon gives the seed back
    harvest(&mut scripts, &mut voxels, 1);
    run(&mut scripts, &mut voxels, &mut herd, 0.5);
    assert_eq!((carried(&scripts, wheat), scripts.carried(7, seeds)), (3, 1));
    // near water they're ready in two thirds of the time
    run(&mut scripts, &mut voxels, &mut herd, 44.0);
    assert_eq!([voxels.at(at(0) + up), voxels.at(at(4) + up)], [shoot, carrots], "the carrots by the water are ripe first");
    run(&mut scripts, &mut voxels, &mut herd, 20.0);
    assert_eq!(voxels.at(at(0) + up), wheat, "a minute by day");
    // a ripe crop gives two more than the one the game gives, and wheat
    // seeds to sow again
    harvest(&mut scripts, &mut voxels, 0);
    harvest(&mut scripts, &mut voxels, 4);
    run(&mut scripts, &mut voxels, &mut herd, 0.5);
    assert_eq!((carried(&scripts, wheat), carried(&scripts, carrots), scripts.carried(7, seeds)), (5, 3, 2));
    // at night, half as fast; and kept with the world, so it grows on after a restart
    scripts.set_day(0.0);
    plant(&mut scripts, &mut voxels, 2, wheat);
    run(&mut scripts, &mut voxels, &mut herd, 65.0);
    assert_eq!(voxels.at(at(2) + up), shoot, "not ripe after a minute at night");
    let kept = scripts.shared.lock().unwrap().store.clone();
    assert!(kept.data["farming"]["crops"].as_object().is_some_and(|c| c.len() == 1), "{:?}", kept.data);
    let mut again = Scripts::default();
    again.players = scripts.players.clone();
    again.shared.lock().unwrap().store = kept;
    again.set_day(0.5);
    again.start(&farming, &mut herd).unwrap();
    run(&mut again, &mut voxels, &mut herd, 35.0);
    assert_eq!(voxels.at(at(2) + up), wheat, "it picked up where it left off");
    // digging up the farmland takes the crop on it out of the list
    plant(&mut again, &mut voxels, 3, wheat);
    run(&mut again, &mut voxels, &mut herd, 0.5);
    voxels.set(at(3), AIR);
    voxels.set(at(3) + up, AIR);
    again.player_block(7, at(3), soil, AIR);
    run(&mut again, &mut voxels, &mut herd, 6.0);
    let crops = again.shared.lock().unwrap().store.data["farming"]["crops"].clone();
    assert!(crops.as_object().is_some_and(|c| c.keys().all(|k| k != &format!("{},{},{}", at(3).x, at(3).y + 1, at(3).z))), "{crops}");
    assert_eq!(again.vms["farming"].error, None, "{:?}", again.vms["farming"].log);
    assert_eq!(scripts.vms["farming"].error, None, "{:?}", scripts.vms["farming"].log);
}

/// world 0.6 and ui 0.2: creations set how fast players heal by
/// themselves, and show meters beside the health bar.
#[test]
fn creations_slow_healing_and_show_meters() {
    use crate::net::{decode, frame_msg, Event, Msg};
    use crate::player::{Me, MAX_HEALTH};
    use crate::script::{Piece, UiChange};
    use bevy::ecs::system::RunSystemOnce;
    home();
    let mut voxels = world();
    let mut herd = Herd::default();
    let mut scripts = Scripts::default();
    scripts.players = vec![PlayerInfo { id: 7, name: "Ada".into(), pos: Vec3::ZERO }, PlayerInfo { id: 8, name: "Bo".into(), pos: Vec3::ZERO }];
    scripts.set_pockets(7, true, &[], 0);
    scripts.set_pockets(8, false, &[], 0);
    let code = |rate: f32| {
        format!(
            r##"
local world = require("@potluck/world@0.6")
local ui = require("@potluck/ui@0.2")
print("ada " .. tostring(world.healing(7, {rate})) .. " bo " .. tostring(world.healing(8, 1)) .. " stranger " .. tostring(world.healing(9, 0)))
print(pcall(world.healing, 7, 5) and "fast ok" or "no fast")
print(pcall(world.healing, 7, -1) and "less ok" or "no less")
ui.meter(7, "food", 0.75, {{ color = "#20a040" }})
ui.meter(nil, "air", 3)
print(pcall(ui.meter, 7, "x", 0/0) and "nan ok" or "no nan")
print(pcall(ui.meter, 7, "x", 0.5, {{ color = "green" }}) and "name ok" or "no name")
print(pcall(ui.meter, 7, "", 0.5) and "blank ok" or "no blank")
"##
        )
    };
    let mut a = creation(&code(0.0));
    scripts.start(&a, &mut herd).unwrap();
    a.id = "other".into();
    a.files.insert("main.luau".into(), code(2.5));
    scripts.start(&a, &mut herd).unwrap();
    run(&mut scripts, &mut voxels, &mut herd, 0.2);
    let l = scripts.vms["test"].log.join("\n");
    assert_eq!(scripts.vms["test"].error, None, "{l}");
    for want in ["ada true bo true stranger false", "no fast", "no less", "no nan", "no name", "no blank"] {
        assert!(l.contains(want), "{want}: {l}");
    }
    // two creations' rates multiply: none at all wins
    let mut changes = scripts.healing_changes();
    changes.sort_by_key(|c| c.0);
    assert_eq!(changes, vec![(7, 0.0)]);
    assert!(scripts.healing_changes().is_empty(), "told once");
    // meters: a colour of its own or the usual amber, filled 0 to 1
    let ch = scripts.ui_changes(&[7, 8]);
    let meter = |pid: u32, id: &str| ch.iter().find_map(|(p, c)| match c {
        UiChange::Piece { id: i, piece, creation } if *p == pid && i == id && creation == "test" => piece.clone(),
        _ => None,
    });
    assert_eq!(meter(7, "food"), Some(Piece { text: String::new(), at: "meter".into(), meter: Some((0.75, [32, 160, 64])) }));
    assert_eq!(meter(8, "air").and_then(|p| p.meter), Some((1.0, [224, 160, 64])));
    // stopping one takes its rate away
    scripts.stop("test", &mut herd);
    assert_eq!(scripts.healing_changes(), vec![(7, 2.5)]);
    scripts.stop("other", &mut herd);
    assert_eq!(scripts.healing_changes(), vec![(7, 1.0)]);

    // the host's own player heals at the rate creations ask for, in survival
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    crate::sim(&mut app, crate::SEED);
    app.insert_resource(crate::settings::Settings { survival: true, ..default() }).insert_resource(Me::default());
    app.init_resource::<crate::input::Controls>().init_resource::<crate::ui::Focus>().init_resource::<crate::ui::Toasts>();
    app.world_mut().resource_mut::<crate::ui::WorldInfo>().in_world = true;
    let starve = creation("local world = require(\"@potluck/world@0.6\")\nworld.every(0.1, function() for _, p in world.players() do world.healing(p, 0) end end)");
    app.world_mut().resource_scope(|w, mut s: Mut<Scripts>| {
        let mut herd = w.resource_mut::<Herd>();
        s.start(&starve, &mut herd).unwrap();
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
    assert_eq!(app.world().resource::<Me>().healing, 0.0);
    app.world_mut().resource_mut::<Me>().health = 10.0;
    for _ in 0..3 {
        app.update();
        std::thread::sleep(Duration::from_millis(50));
    }
    let me = app.world().resource::<Me>();
    assert!(me.health <= 10.0 && me.health > 0.0, "no healing by themselves: {}", me.health);
    assert!(MAX_HEALTH > 10.0);

    // what goes between devices
    for m in [Msg::Healing(0.5), Msg::Piece { creation: "c".into(), id: "food".into(), piece: Some(Piece { text: String::new(), at: "meter".into(), meter: Some((0.5, [1, 2, 3])) }) }] {
        let Some(Event::Msg(_, back)) = decode(transport::Wire::Data(1, frame_msg(&m))) else { panic!("didn't decode") };
        assert_eq!(serde_json::to_string(&back).unwrap(), serde_json::to_string(&m).unwrap());
    }
}

/// Hunger: the food bar goes down, the food you carry is eaten when you're
/// hungry, an empty belly stops healing and a full one speeds it up.
#[test]
fn hunger_eats_what_you_carry_and_an_empty_belly_stops_healing() {
    use crate::blocks::{by_name, GLASS, STONE};
    home();
    let mut hunger = Library::load().list.remove("hunger").expect("the base game has hunger");
    hunger.knobs.insert("minutes".into(), KnobValue::Number(5.0));
    let (carrots, wheat) = (by_name("Carrots").unwrap(), by_name("Wheat").unwrap());
    let mut voxels = world();
    let mut herd = Herd::default();
    let mut scripts = Scripts::default();
    scripts.players = vec![PlayerInfo { id: 7, name: "Ada".into(), pos: Vec3::ZERO }, PlayerInfo { id: 8, name: "Bo".into(), pos: Vec3::ZERO }];
    scripts.carry(7, true, &[(carrots, 1), (wheat, 1)], 0);
    scripts.set_pockets(8, false, &[], 0);
    scripts.start(&hunger, &mut herd).unwrap();
    let bar = |scripts: &Scripts, pid: u32| scripts.shared.lock().unwrap().ui.pieces.get(&(Some(pid), "hunger".into(), "food".into())).and_then(|p| p.meter).map(|m| m.0);
    let carried = |scripts: &Scripts, b| scripts.carried(7, b);
    let shown = |scripts: &Scripts, id: &str| scripts.shared.lock().unwrap().ui.pieces.get(&(Some(7), "hunger".into(), id.into())).map(|p| p.text.clone());
    run(&mut scripts, &mut voxels, &mut herd, 1.5);
    assert_eq!(bar(&scripts, 7), Some(1.0), "a full bar to start");
    assert_eq!(bar(&scripts, 8), None, "no hunger in creative");
    assert_eq!(scripts.healing_changes(), vec![(7, 2.0)], "well fed heals faster");
    // 5 minutes from full to empty: down to 12 of 20 in two, then a carrot
    run(&mut scripts, &mut voxels, &mut herd, 118.0);
    assert_eq!(carried(&scripts, carrots), 1);
    run(&mut scripts, &mut voxels, &mut herd, 4.0);
    assert_eq!(carried(&scripts, carrots), 0, "ate the carrot");
    assert_eq!(shown(&scripts, "ate").as_deref(), Some("You ate some carrots."));
    assert!(bar(&scripts, 7).is_some_and(|b| b >= 0.85), "{:?}", bar(&scripts, 7));
    // then the wheat, then nothing left: an empty belly stops healing
    run(&mut scripts, &mut voxels, &mut herd, 100.0);
    assert_eq!(carried(&scripts, wheat), 0, "chewed the wheat");
    scripts.healing_changes();
    run(&mut scripts, &mut voxels, &mut herd, 202.0);
    assert_eq!(bar(&scripts, 7), Some(0.0));
    assert_eq!(scripts.healing_changes(), vec![(7, 0.0)]);
    assert!(shown(&scripts, "hungry").is_some_and(|t| t.contains("starving") && t.chars().count() <= 60), "{:?}", shown(&scripts, "hungry"));
    // and it hurts, a little every few seconds, but only down to half
    let hurts = scripts.health_changes();
    assert!(!hurts.is_empty() && hurts.iter().all(|h| h.0 == 7 && h.1 == -1.0 && h.2 == 10.0), "{hurts:?}");
    run(&mut scripts, &mut voxels, &mut herd, 10.0);
    assert!(shown(&scripts, "hungry").is_none(), "said for a few seconds");
    assert_eq!(scripts.health_changes(), vec![(7, -2.0, 10.0)], "one every five seconds");
    // a carrot again: eaten at once, healing as usual and no more hurting
    scripts.pocket_changes();
    scripts.carry(7, true, &[(carrots, 1)], 1000);
    run(&mut scripts, &mut voxels, &mut herd, 1.5);
    assert_eq!(carried(&scripts, carrots), 0);
    assert_eq!(scripts.healing_changes(), vec![(7, 1.0)]);
    scripts.health_changes();
    run(&mut scripts, &mut voxels, &mut herd, 10.0);
    assert!(scripts.health_changes().is_empty(), "fed again");
    // into creative: the bar goes and so does the slowing
    scripts.pocket_changes();
    scripts.set_pockets(7, false, &[], 1000);
    run(&mut scripts, &mut voxels, &mut herd, 1.5);
    assert_eq!(bar(&scripts, 7), None);
    assert_eq!(scripts.vms["hunger"].error, None, "{:?}", scripts.vms["hunger"].log);
}

/// world 0.17: Interact looks through water to the block behind it, but
/// with an item in hand it uses the water (a bucket, a rod).
#[test]
fn a_ray_finds_water_before_the_block_behind_it() {
    use crate::blocks::{AIR, STONE, WATER};
    use crate::player::{raycast, water_ahead};
    let mut w = world();
    let (eye, dir) = (Vec3::new(100.5, 110.5, 100.5), Vec3::X);
    for x in 101..106 {
        w.set(IVec3::new(x, 110, 100), AIR);
    }
    w.set(IVec3::new(102, 110, 100), WATER);
    w.set(IVec3::new(104, 110, 100), STONE);
    assert_eq!(raycast(&w, eye, dir, 6.0).map(|h| h.block), Some(IVec3::new(104, 110, 100)), "the block behind the water");
    assert_eq!(water_ahead(&w, eye, dir, 6.0), Some(IVec3::new(102, 110, 100)), "the water in front of it");
    w.set(IVec3::new(102, 110, 100), AIR);
    w.set(IVec3::new(105, 110, 100), WATER);
    assert_eq!(water_ahead(&w, eye, dir, 6.0), None, "water behind a block isn't reached");
}

/// world 0.8 and the Tools creation: a tool in hand digs its kind of block
/// faster on the player's own game, wears with every block it breaks, and
/// breaks when it's worn out; items can't be put down.
#[test]
fn tools_dig_faster_wear_out_and_break() {
    use crate::blocks::{by_name, AIR, STONE};
    use crate::net::{decode, frame_msg, Event, Msg};
    use crate::player::Me;
    home();
    let tools = Library::load().list.remove("tools").expect("the base game has tools");
    let thing = |n: &str| crate::items::by_name(n).unwrap();
    let (pick, axe, sword) = (thing("Wooden pickaxe"), thing("Stone axe"), thing("Wooden sword"));
    assert!(crate::items::is_item(pick) && by_name("Wooden pickaxe").is_none() && !crate::items::is_item(STONE.into()), "tools are things to hold, never blocks");
    assert_eq!((pick, crate::items::stack(pick), crate::items::stack(STONE.into())), (crate::items::WOODEN_PICKAXE, 1, 99), "one to a slot");
    let mut voxels = world();
    let mut herd = Herd::default();
    let mut scripts = Scripts::default();
    scripts.players = vec![PlayerInfo { id: 7, name: "Ada".into(), pos: Vec3::ZERO }, PlayerInfo { id: 8, name: "Bo".into(), pos: Vec3::ZERO }];
    scripts.carry(7, true, &[(pick, 1), (axe, 1)], 0);
    scripts.set_pockets(8, false, &[], 0);
    scripts.start(&tools, &mut herd).unwrap();
    let digging = scripts.digging_changes().expect("the tools say how they dig");
    assert_eq!(digging.len(), 21, "five materials, three kinds each, three hoes, shears, a sickle and a hammer: {digging:?}");
    let speeds = |b| digging.iter().find(|d| d.0 == b).map(|d| d.1).unwrap();
    assert_eq!(speeds(pick), crate::items::Digging { speeds: [1.0, 2.5, 1.0, 1.0, 1.0], level: 1 }, "a wooden pickaxe digs stone faster");
    assert_eq!((speeds(axe).speeds[2], speeds(axe).level), (4.0, 2), "a stone axe chops faster, and reaches further");
    assert!(scripts.digging_changes().is_none(), "only told again when it changes");

    // on the player's game: faster with the right tool, as usual by hand or with the wrong one
    let mut me = Me::default();
    me.rules.digging = digging.iter().copied().collect();
    me.pockets.0[0] = crate::items::Slot { thing: pick, count: 1 };
    let (oak, dirt) = (by_name("Oak log").unwrap(), by_name("Dirt").unwrap());
    assert_eq!((me.dig_speed(STONE), me.dig_speed(oak), me.dig_speed(dirt)), (2.5, 1.0, 1.0));
    me.pockets.0[0].count = 0;
    assert_eq!(me.dig_speed(STONE), 1.0, "an empty slot in survival is an empty hand");
    me.pockets.0[0] = crate::items::Slot { thing: axe, count: 1 };
    assert_eq!(me.dig_speed(oak), 4.0);

    // the wear bar shows while a tool is in hand, with a line saying what it is
    let piece = |scripts: &Scripts, pid: u32, id: &str| scripts.shared.lock().unwrap().ui.pieces.get(&(Some(pid), "tools".into(), id.into())).cloned();
    let carried = |scripts: &Scripts, b: crate::items::Thing| scripts.carried(7, b);
    run(&mut scripts, &mut voxels, &mut herd, 0.6);
    assert!(piece(&scripts, 7, "wear").is_none(), "nothing in hand, no bar");
    scripts.set_held(7, pick);
    scripts.set_held(8, pick);
    run(&mut scripts, &mut voxels, &mut herd, 0.6);
    assert_eq!(piece(&scripts, 7, "wear").and_then(|p| p.meter).map(|m| m.0), Some(1.0));
    assert!(piece(&scripts, 7, "wear_says").is_some_and(|p| p.text.contains("wear") && p.text.chars().count() <= 60));
    assert!(piece(&scripts, 8, "wear").is_none(), "tools don't wear in creative");
    // every block it breaks wears it, and the 60th breaks it
    let at = IVec3::new(10, 50, 12);
    for _ in 0..30 {
        scripts.player_block(7, at, STONE, AIR);
        scripts.player_block(8, at, STONE, AIR);
    }
    run(&mut scripts, &mut voxels, &mut herd, 0.6);
    assert_eq!(piece(&scripts, 7, "wear").and_then(|p| p.meter).map(|m| m.0), Some(0.5));
    for _ in 0..29 {
        scripts.player_block(7, at, STONE, AIR);
    }
    run(&mut scripts, &mut voxels, &mut herd, 0.6);
    assert_eq!(carried(&scripts, pick), 1, "still going after 59");
    scripts.player_block(7, at, STONE, AIR);
    run(&mut scripts, &mut voxels, &mut herd, 0.6);
    assert_eq!(carried(&scripts, pick), 0, "worn out after 60");
    assert_eq!(piece(&scripts, 7, "broke").map(|p| p.text).as_deref(), Some("Your wooden pickaxe broke."));
    assert_eq!(carried(&scripts, axe), 1, "only the one in hand wears");
    // a sword isn't a tool: no bar
    scripts.set_held(7, sword);
    run(&mut scripts, &mut voxels, &mut herd, 0.6);
    assert!(piece(&scripts, 7, "wear").is_none());
    assert_eq!(scripts.vms["tools"].error, None, "{:?}", scripts.vms["tools"].log);
    // stopping takes its speeds away
    scripts.stop("tools", &mut herd);
    assert_eq!(scripts.digging_changes(), Some(vec![]));

    // the API's edges
    let code = r#"
local world = require("@potluck/world@0.8")
print("kinds " .. world.kind("Stone") .. " " .. world.kind("Oak log") .. " " .. world.kind("Dirt") .. " " .. world.kind("Oak leaves") .. " " .. world.kind("Glass") .. " " .. world.kind("Water") .. " " .. world.kind("Air"))
print("held " .. tostring(world.held(7)))
print(pcall(world.kind, "Mud") and "mud ok" or "no mud")
print(pcall(world.digging, "Air", { hard = 2 }) and "air ok" or "no air")
print(pcall(world.digging, "Wooden pickaxe", { mud = 2 }) and "kind ok" or "no kind")
print(pcall(world.digging, "Wooden pickaxe", { hard = 11 }) and "fast ok" or "no fast")
print(pcall(world.digging, "Wooden pickaxe", { hard = -1 }) and "slow ok" or "no slow")
world.digging("Stone", { hard = 3 })
world.digging("Stone", { hard = 3, soft = 1 })
world.after(0.1, function()
	print("held now " .. tostring(world.held(7)))
	print(pcall(world.set_block, vector.create(10, 50, 12), "Wooden pickaxe") and "set ok" or "no set")
	world.digging("Stone", nil)
end)
"#;
    scripts.set_held(7, 0);
    scripts.start(&creation(code), &mut herd).unwrap();
    let log = scripts.vms["test"].log.join("\n");
    assert_eq!(scripts.vms["test"].error, None, "{log}");
    for want in ["kinds hard wood soft plant glass water air", "held nil", "no mud", "no air", "no kind", "no fast", "no slow"] {
        assert!(log.contains(want), "{want}: {log}");
    }
    assert_eq!(scripts.digging_changes(), Some(vec![(STONE.into(), crate::items::Digging { speeds: [1.0, 3.0, 1.0, 1.0, 1.0], level: 0 })]), "said twice the same, told once");
    scripts.set_held(7, sword);
    run(&mut scripts, &mut voxels, &mut herd, 0.2);
    let log = scripts.vms["test"].log.join("\n");
    assert!(log.contains("held now Wooden sword") && log.contains("no set"), "{log}");
    assert_eq!(scripts.digging_changes(), Some(vec![]), "nil takes it away");
    // what goes between devices
    let m = Msg::Digging(vec![(pick, crate::items::Digging { speeds: [1.0, 2.5, 1.0, 1.0, 1.0], level: 1 })]);
    let Some(Event::Msg(_, back)) = decode(Wire::Data(1, frame_msg(&m))) else { panic!("didn't decode") };
    assert_eq!(serde_json::to_string(&back).unwrap(), serde_json::to_string(&m).unwrap());
}

#[test]
fn ores_need_a_good_enough_pickaxe_and_break_into_what_they_hold() {
    use crate::blocks::{by_name, GLASS, STONE};
    use crate::items::{self, Digging, Drop, Rules};
    use crate::net::{decode, frame_msg, Event, Msg};
    use crate::player::Me;
    let (coal, iron, chest) = (crate::blocks::COAL_ORE, crate::blocks::IRON_ORE, crate::blocks::OLD_CHEST);
    assert_eq!((by_name("Coal ore"), by_name("Moonsilver ore"), by_name("Old chest"), by_name("Glowcap")), (Some(coal), Some(crate::blocks::MOONSILVER_ORE), Some(chest), Some(crate::blocks::GLOWCAP)));
    assert_eq!((crate::sound::material(iron).name(), crate::sound::material(chest).name(), crate::sound::material(crate::blocks::GLOWCAP).name()), ("hard", "wood", "plant"));
    assert_eq!((items::by_name("Raw iron"), items::stack(items::RAW_IRON), items::name(items::CRYSTAL)), (Some(items::RAW_IRON), 99, "Crystal"), "what ores give stacks as blocks do");

    // the rules on their own: a level for the kinds a tool digs faster
    let mut rules = Rules::default();
    let (wood, stone) = (items::WOODEN_PICKAXE, items::STONE_PICKAXE);
    rules.digging.insert(wood, Digging { speeds: [1.0, 2.5, 1.0, 1.0, 1.0], level: 1 });
    rules.digging.insert(stone, Digging { speeds: [1.0, 4.0, 1.0, 1.0, 1.0], level: 2 });
    rules.digging.insert(items::STONE_AXE, Digging { speeds: [1.0, 1.0, 4.0, 1.0, 1.0], level: 2 });
    rules.drops.insert(iron, Drop { gives: items::RAW_IRON, least: 1, most: 1, level: 2 });
    rules.drops.insert(coal, Drop { gives: items::COAL, least: 1, most: 3, level: 1 });
    rules.drops.insert(GLASS, Drop { gives: 0, least: 0, most: 0, level: 0 });
    assert!(!rules.can_break(0, iron) && !rules.can_break(wood, iron) && rules.can_break(stone, iron));
    assert!(!rules.can_break(items::STONE_AXE, iron), "an axe's level is for wood");
    assert!(rules.can_break(0, STONE) && rules.can_break(0, GLASS), "blocks with no level break by hand");
    assert_eq!(rules.tool_for(iron), Some(stone));
    assert_eq!(crate::player::needs(&rules, iron), "Iron ore needs a stone pickaxe or better");
    rules.digging.clear();
    assert_eq!(crate::player::needs(&rules, iron), "Iron ore needs a better tool");
    assert_eq!((rules.gives(coal, 0.0), rules.gives(coal, 0.5), rules.gives(coal, 1.0)), (Some((items::COAL, 1)), Some((items::COAL, 2)), Some((items::COAL, 3))));
    assert_eq!((rules.gives(GLASS, 0.5), rules.gives(STONE, 0.5)), (None, Some((items::of_block(STONE), 1))), "nothing, or the block itself");

    // the API, from creations
    let mut voxels = world();
    let mut herd = Herd::default();
    let mut scripts = Scripts::default();
    let code = r#"
local world = require("@potluck/world@0.13")
print(pcall(world.drops, "Mud", { gives = "Coal" }) and "mud ok" or "no mud")
print(pcall(world.drops, "Raw iron", { gives = "Coal" }) and "item ok" or "no item")
print(pcall(world.drops, "Air", {}) and "air ok" or "no air")
print(pcall(world.drops, "Iron ore", { gives = "Water" }) and "water ok" or "no water")
print(pcall(world.drops, "Iron ore", { gives = "Mud" }) and "gives ok" or "no gives")
print(pcall(world.drops, "Iron ore", { count = 65 }) and "many ok" or "no many")
print(pcall(world.drops, "Iron ore", { count = { 3, 1 } }) and "back ok" or "no back")
print(pcall(world.drops, "Iron ore", { level = 10 }) and "level ok" or "no level")
print(pcall(world.drops, "Iron ore", { colour = "red" }) and "colour ok" or "no colour")
print(pcall(world.digging, "Stone pickaxe", { hard = 4, level = 1.5 }) and "half ok" or "no half")
world.digging("Stone pickaxe", { hard = 4, level = 2 })
world.drops("Iron ore", { gives = "Raw iron", level = 2 })
world.drops("Coal ore", { gives = "Coal", count = { 1, 2 } })
world.drops("Glass", { gives = false })
world.drops("Gold ore", { level = 3 })
"#;
    scripts.start(&creation(code), &mut herd).unwrap();
    let log = scripts.vms["test"].log.join("\n");
    assert_eq!(scripts.vms["test"].error, None, "{log}");
    for want in ["no mud", "no item", "no air", "no water", "no gives", "no many", "no back", "no level", "no colour", "no half"] {
        assert!(log.contains(want), "{want}: {log}");
    }
    let drops = scripts.drops_changes().expect("told what blocks give");
    let gold = crate::blocks::GOLD_ORE;
    assert_eq!(
        drops,
        vec![
            (GLASS, Drop { gives: 0, least: 1, most: 1, level: 0 }),
            (coal, Drop { gives: items::COAL, least: 1, most: 2, level: 0 }),
            (iron, Drop { gives: items::RAW_IRON, least: 1, most: 1, level: 2 }),
            (gold, Drop { gives: items::of_block(gold), least: 1, most: 1, level: 3 }),
        ]
    );
    assert_eq!(scripts.digging_changes().map(|d| d[0].1.level), Some(2));
    assert!(scripts.drops_changes().is_none(), "only told again when it changes");
    // another creation that says it later has the last word, until it stops
    let mut other = creation("local world = require(\"@potluck/world@0.13\")\nworld.drops(\"Iron ore\", { gives = \"Coal\" })");
    other.id = "other".into();
    scripts.start(&other, &mut herd).unwrap();
    let rules = scripts.rules();
    assert_eq!(rules.drops[&iron], Drop { gives: items::COAL, least: 1, most: 1, level: 0 });
    scripts.stop("other", &mut herd);
    assert_eq!(scripts.rules().drops[&iron].gives, items::RAW_IRON, "the first one's again");
    scripts.stop("test", &mut herd);
    assert_eq!(scripts.drops_changes(), Some(vec![]), "and none once both stop");
    run(&mut scripts, &mut voxels, &mut herd, 0.1);

    // on the player's game: the block doesn't give until the hand can break it
    let mut me = Me::default();
    me.rules = rules;
    me.pockets.0[0] = crate::items::Slot { thing: wood, count: 1 };
    me.rules.drops.insert(iron, Drop { gives: items::RAW_IRON, least: 1, most: 1, level: 2 });
    assert_eq!(me.dig_speed(iron), 0.0, "a wooden pickaxe makes no headway on iron");
    me.pockets.0[0] = crate::items::Slot { thing: stone, count: 1 };
    assert_eq!(me.dig_speed(iron), 4.0);

    // what goes between devices
    let m = Msg::Drops(drops);
    let Some(Event::Msg(_, back)) = decode(Wire::Data(1, frame_msg(&m))) else { panic!("didn't decode") };
    assert_eq!(serde_json::to_string(&back).unwrap(), serde_json::to_string(&m).unwrap());
}

/// A flat stone pad with open sky over it, for fliers and balloons: its
/// middle and the height of its top.
fn pad(voxels: &mut Voxels) -> (IVec3, i32) {
    let (cx, cz, ground) = (world::GREEN.x as i32 + 80, world::GREEN.z as i32 - 60, 40);
    for x in cx - 14..=cx + 14 {
        for z in cz - 14..=cz + 14 {
            for y in 1..world::WY {
                voxels.set(IVec3::new(x, y, z), if y <= ground { crate::blocks::STONE } else { crate::blocks::AIR });
            }
        }
    }
    (IVec3::new(cx, ground + 1, cz), ground)
}

/// world 0.9: fliers float with no gravity and ease into the climb they're
/// told (a rider's view never jolts), riders steer them only in the air, and
/// a creature can become another kind of the same creation.
#[test]
fn fliers_ease_into_their_climb_and_creatures_become_other_kinds() {
    home();
    let mut voxels = world();
    let (mid, ground) = pad(&mut voxels);
    let mut herd = Herd::default();
    let mut scripts = Scripts::default();
    let code = format!(
        r#"
local world = require("@potluck/world@0.9")
local Bird = world.creature {{ name = "Bird", count = 0, flies = true, rideable = true, speed = 2, body = {{ {{ "body", size = {{ 0.6, 0.4, 0.8 }} }} }} }}
local Egg = world.creature {{ name = "Egg", count = 0, body = {{ {{ "shell", size = {{ 0.3, 0.4, 0.3 }} }} }} }}
function Egg:used() print("an egg " .. tostring(self:climb(1)) .. " " .. tostring(self:on_ground())) end
function Egg:hit()
	self:become(Egg)
	self:become(Bird)
	print("hatched " .. self.kind .. " " .. tostring(self:climb(1)))
	print(pcall(self.become, self, {{ name = "Nobody" }}) and "nobody ok" or "no nobody")
	print(pcall(self.climb, self, 0/0) and "nan ok" or "no nan")
end
function Bird:used() print("a bird, climbing " .. tostring(self:climb(9))) end
world.after(0.1, function() world.spawn(Egg, vector.create({x}, {y}, {z})) end)
"#,
        x = mid.x as f32 + 0.5,
        y = mid.y as f32 + 2.0,
        z = mid.z as f32 + 0.5
    );
    let ada = PlayerInfo { id: 7, name: "Ada".into(), pos: mid.as_vec3() };
    scripts.players = vec![ada.clone()];
    scripts.start(&creation(&code), &mut herd).unwrap();
    run(&mut scripts, &mut voxels, &mut herd, 1.5);
    let id = *herd.bodies.keys().next().expect("an egg");
    assert!(herd.bodies[&id].on_ground && !herd.bodies[&id].flies, "the egg fell and landed");
    scripts.event(&mut voxels, &mut herd, id, "used", &ada);
    let rev = herd.bodies[&id].rev;
    scripts.event(&mut voxels, &mut herd, id, "hit", &ada);
    scripts.event(&mut voxels, &mut herd, id, "used", &ada);
    let log = scripts.vms["test"].log.join("\n");
    assert_eq!(scripts.vms["test"].error, None, "{log}");
    for want in ["an egg false true", "hatched Bird true", "no nobody", "no nan", "a bird, climbing true"] {
        assert!(log.contains(want), "{want}: {log}");
    }
    let b = &herd.bodies[&id];
    assert!(b.kind == "Bird" && b.flies && b.rideable && b.rev > rev && b.parts[0].name == "body", "{} {}", b.kind, b.rev);
    assert_eq!(b.climb, creatures::MAX_CLIMB, "climbs no faster than the most");

    // rising: no gravity, and the climb eases in a little each step, never jumping
    let mut rng = Rng::new(1);
    herd.bodies.get_mut(&id).unwrap().climb = 1.5;
    let (start, mut last) = (herd.bodies[&id].pos.y, herd.bodies[&id].vel.y);
    for _ in 0..200 {
        creatures::step(&mut herd, &voxels, 0.05, &mut rng);
        let v = herd.bodies[&id].vel.y;
        assert!(v >= last - 1e-4 && v - last <= 0.1 && v <= 1.5 + 1e-4, "{last} to {v}");
        last = v;
    }
    let risen = herd.bodies[&id].pos.y - start;
    assert!((9.0..15.0).contains(&risen) && last > 1.4, "rose {risen}, at {last}");
    // told to hold, it stays where it is
    herd.bodies.get_mut(&id).unwrap().climb = 0.0;
    for _ in 0..200 {
        creatures::step(&mut herd, &voxels, 0.05, &mut rng);
    }
    let held = herd.bodies[&id].pos.y;
    for _ in 0..100 {
        creatures::step(&mut herd, &voxels, 0.05, &mut rng);
    }
    assert!((herd.bodies[&id].pos.y - held).abs() < 0.05 && herd.bodies[&id].vel.y.abs() < 0.01, "hovers");

    // a rider steers it in the air, gathering speed gently
    herd.bodies.get_mut(&id).unwrap().rider = Some(7);
    herd.riders.insert(7, (Vec2::new(0.0, 1.0), 0.0));
    let from = herd.bodies[&id].pos;
    let mut last = Vec2::new(herd.bodies[&id].vel.x, herd.bodies[&id].vel.z).length();
    for _ in 0..60 {
        creatures::step(&mut herd, &voxels, 0.05, &mut rng);
        let v = Vec2::new(herd.bodies[&id].vel.x, herd.bodies[&id].vel.z).length();
        assert!((v - last).abs() <= 0.25, "speed changes by {} in a step", v - last);
        last = v;
    }
    assert!(herd.bodies[&id].pos.z < from.z - 2.0, "flew forward: {} to {}", from.z, herd.bodies[&id].pos.z);
    // down again: landed, it stays put however its rider pushes
    herd.bodies.get_mut(&id).unwrap().climb = -3.0;
    herd.bodies.get_mut(&id).unwrap().pos = Vec3::new(mid.x as f32 + 0.5, herd.bodies[&id].pos.y, mid.z as f32 + 0.5);
    herd.riders.insert(7, (Vec2::ZERO, 0.0));
    for _ in 0..400 {
        creatures::step(&mut herd, &voxels, 0.05, &mut rng);
    }
    let landed = herd.bodies[&id].pos;
    herd.riders.insert(7, (Vec2::new(0.0, 1.0), 0.0));
    assert!(herd.bodies[&id].on_ground && (landed.y - (ground + 1) as f32).abs() < 0.1, "landed at {landed}");
    for _ in 0..40 {
        creatures::step(&mut herd, &voxels, 0.05, &mut rng);
    }
    assert!(herd.bodies[&id].pos.distance(landed) < 0.05, "it doesn't drive about on the ground");
    // and it can't climb out of the world
    herd.bodies.get_mut(&id).unwrap().pos.y = world::WY as f32 + 20.0;
    herd.bodies.get_mut(&id).unwrap().climb = 4.0;
    for _ in 0..200 {
        creatures::step(&mut herd, &voxels, 0.05, &mut rng);
    }
    assert!(herd.bodies[&id].pos.y < world::WY as f32 + creatures::FLY_ABOVE + 0.5, "{}", herd.bodies[&id].pos.y);
}

/// The Hot air balloon: a basket put down fills up into a balloon; its
/// rider holds Burner to rise and lets go to sink, gently; it lands, packs
/// back into a basket, and the world keeps where balloons are.
#[test]
fn a_basket_fills_into_a_balloon_that_rises_lands_and_packs_away() {
    use crate::blocks::{by_name, AIR};
    home();
    let balloon = Library::load().list.remove("balloon").expect("the base game has the balloon");
    let basket = by_name("Balloon basket").unwrap();
    assert!(crate::items::block(basket.into()) == Some(basket) && crate::blocks::is_solid(basket), "a basket is a block to put down");
    let mut voxels = world();
    let (mid, ground) = pad(&mut voxels);
    let mut herd = Herd::default();
    let mut scripts = Scripts::default();
    let ada = PlayerInfo { id: 7, name: "Ada".into(), pos: mid.as_vec3() + Vec3::new(3.0, 0.0, 0.0) };
    scripts.players = vec![ada.clone()];
    scripts.set_pockets(7, true, &[], 0);
    scripts.start(&balloon, &mut herd).unwrap();
    assert!(herd.bodies.is_empty(), "no balloons until a basket is put down");
    let piece = |scripts: &Scripts, id: &str| scripts.shared.lock().unwrap().ui.pieces.get(&(Some(7), "balloon".into(), id.into())).map(|p| p.text.clone());
    let log = |scripts: &Scripts| scripts.vms["balloon"].log.join("\n");
    let kind = |herd: &Herd| herd.bodies.values().next().map(|b| b.kind.clone()).unwrap_or_default();

    // put down, it becomes a balloon and fills up
    voxels.set(mid, basket);
    scripts.player_block(7, mid, AIR, basket);
    run(&mut scripts, &mut voxels, &mut herd, 0.3);
    assert_eq!(voxels.at(mid), AIR, "the basket turns into the balloon");
    assert_eq!(kind(&herd), "Balloon (empty)", "{}", log(&scripts));
    let id = *herd.bodies.keys().next().unwrap();
    scripts.event(&mut voxels, &mut herd, id, "used", &ada);
    assert!(piece(&scripts, "wait").is_some(), "still filling");
    run(&mut scripts, &mut voxels, &mut herd, 1.5);
    assert_eq!(kind(&herd), "Balloon (filling)");
    run(&mut scripts, &mut voxels, &mut herd, 2.5);
    assert_eq!(kind(&herd), "Hot air balloon");
    let b = &herd.bodies[&id];
    assert!(b.rideable && b.flies && b.parts.len() <= 64, "{}", b.parts.len());
    assert!(b.parts.iter().map(|p| p.at.y + p.size.y / 2.0).fold(0.0, f32::max) > 7.5, "a tall envelope");

    // climb in and hold Burner: it rises, easing into it
    herd.bodies.get_mut(&id).unwrap().rider = Some(7);
    scripts.event(&mut voxels, &mut herd, id, "ridden", &ada);
    assert!(piece(&scripts, "aboard").is_some_and(|t| t.contains("Burner") && t.chars().count() <= 60));
    scripts.press(7, "balloon", "burner", true);
    let start = herd.bodies[&id].pos.y;
    run(&mut scripts, &mut voxels, &mut herd, 0.3);
    assert_eq!(kind(&herd), "Hot air balloon (burner on)", "the flame's lit");
    assert_eq!(herd.bodies[&id].rider, Some(7), "still aboard when the flame lights");
    run(&mut scripts, &mut voxels, &mut herd, 6.0);
    let up = herd.bodies[&id].pos.y - start;
    assert!(up > 4.0, "rose {up}");
    assert!(herd.bodies[&id].vel.y <= 1.5 + 1e-4, "no faster than the burner lifts it");
    // let go: the flame's out and it slows, then sinks gently
    scripts.press(7, "balloon", "burner", false);
    run(&mut scripts, &mut voxels, &mut herd, 0.3);
    assert_eq!(kind(&herd), "Hot air balloon");
    assert!(herd.bodies[&id].vel.y > 0.5, "it doesn't stop at once");
    run(&mut scripts, &mut voxels, &mut herd, 6.0);
    assert!((-0.65..0.0).contains(&herd.bodies[&id].vel.y), "sinking slowly: {}", herd.bodies[&id].vel.y);
    // bopped in the air, it won't pack up
    herd.bodies.get_mut(&id).unwrap().rider = None;
    scripts.event(&mut voxels, &mut herd, id, "hit", &ada);
    assert!(herd.bodies.contains_key(&id), "not in the air");
    // where it is is kept with the world
    let kept = |scripts: &Scripts| scripts.shared.lock().unwrap().store.get("balloon", "balloons").and_then(|v| v.as_object().map(|a| a.len())).unwrap_or(0);
    assert_eq!(kept(&scripts), 1, "{:?} {}", scripts.shared.lock().unwrap().store.get("balloon", "balloons"), log(&scripts));
    for _ in 0..60 {
        run(&mut scripts, &mut voxels, &mut herd, 1.0);
        if herd.bodies[&id].on_ground {
            break;
        }
    }
    let b = &herd.bodies[&id];
    assert!(b.on_ground && (b.pos.y - (ground + 1) as f32).abs() < 0.1, "landed at {}", b.pos);

    // a restart brings it back full
    let file = std::env::temp_dir().join(format!("potluck-balloons-{}.json", std::process::id()));
    run(&mut scripts, &mut voxels, &mut herd, 5.5);
    scripts.shared.lock().unwrap().store.save(&file);
    let mut again = Scripts::default();
    again.shared.lock().unwrap().store = crate::script::Store::load(&file);
    let _ = std::fs::remove_file(&file);
    let mut fresh = Herd::default();
    again.start(&balloon, &mut fresh).unwrap();
    run(&mut again, &mut voxels, &mut fresh, 0.3);
    assert_eq!(fresh.bodies.values().map(|b| b.kind.as_str()).collect::<Vec<_>>(), ["Hot air balloon"], "{:?}", again.vms["balloon"].log);
    assert!(fresh.bodies.values().next().unwrap().pos.distance(herd.bodies[&id].pos) < 1.0);
    // and a reload doesn't make two
    scripts.start(&balloon, &mut herd).unwrap();
    run(&mut scripts, &mut voxels, &mut herd, 0.3);
    assert_eq!(herd.bodies.len(), 1);

    // landed and empty, a bop packs it back into a basket
    scripts.event(&mut voxels, &mut herd, id, "hit", &ada);
    assert!(herd.bodies.is_empty(), "packed up");
    assert_eq!(Some(scripts.carried(7, basket)).filter(|n| *n > 0), Some(1));
    run(&mut scripts, &mut voxels, &mut herd, 0.2);
    assert_eq!(kept(&scripts), 0, "and forgotten");
    assert_eq!(scripts.vms["balloon"].error, None, "{}", log(&scripts));
}

/// Woollies in survival: patting one shears it for wool, and it grows back.
#[test]
fn survival_players_shear_woollies_for_wool() {
    use crate::blocks::{by_name, GLASS, STONE};
    home();
    let mut woolly = Library::load().list.remove("woolly").expect("the base game has woollies");
    woolly.knobs.insert("regrow".into(), KnobValue::Number(0.5));
    let wool = by_name("Cream wool").unwrap();
    let mut voxels = world();
    let mut herd = Herd::default();
    let mut scripts = Scripts::default();
    scripts.set_pockets(7, true, &[], 0);
    scripts.set_pockets(8, false, &[], 0);
    scripts.start(&woolly, &mut herd).unwrap();
    run(&mut scripts, &mut voxels, &mut herd, 0.5);
    let id = *herd.bodies.keys().next().expect("woollies");
    let at = herd.bodies[&id].pos;
    let (ada, bo) = (PlayerInfo { id: 7, name: "Ada".into(), pos: at }, PlayerInfo { id: 8, name: "Bo".into(), pos: at });
    scripts.players = vec![ada.clone(), bo.clone()];
    let wool_of = |scripts: &Scripts| scripts.carried(7, wool);
    // creative: just a pat
    scripts.event(&mut voxels, &mut herd, id, "used", &bo);
    assert_eq!(herd.bodies[&id].kind, "Woolly");
    scripts.event(&mut voxels, &mut herd, id, "used", &ada);
    assert_eq!((herd.bodies[&id].kind.as_str(), wool_of(&scripts)), ("Shorn woolly", 1));
    let shorn = herd.bodies[&id].parts.len();
    // nothing more till it grows back
    scripts.event(&mut voxels, &mut herd, id, "used", &ada);
    assert_eq!(wool_of(&scripts), 1);
    run(&mut scripts, &mut voxels, &mut herd, 31.0);
    assert_eq!(herd.bodies[&id].kind, "Woolly", "grown back");
    assert!(herd.bodies[&id].parts.len() > shorn);
    scripts.event(&mut voxels, &mut herd, id, "used", &ada);
    assert_eq!(wool_of(&scripts), 2);
    assert_eq!(scripts.vms["woolly"].error, None, "{:?}", scripts.vms["woolly"].log);
}

/// world 0.7: a creation can hurt a player only down to some health, which
/// their own game works out, since only it knows their health.
#[test]
fn creations_can_hurt_players_only_down_to_some_health() {
    use crate::net::{decode, frame_msg, Event, Msg};
    use crate::player::{Me, MAX_HEALTH};
    use bevy::ecs::system::RunSystemOnce;
    use crate::settings::Settings;
    home();
    let mut voxels = world();
    let mut herd = Herd::default();
    let mut scripts = Scripts::default();
    let code = r#"
local world = require("@potluck/world@0.7")
world.after(0.1, function()
	print("hurt " .. tostring(world.hurt(7, 4, { down_to = 10 })) .. " " .. tostring(world.hurt(7, 3, { down_to = 10 })) .. " " .. tostring(world.hurt(7, 2)) .. " " .. tostring(world.hurt(7, 1, {})))
	print("heal " .. tostring(world.heal(7, 1, { down_to = 5 })))
	print("creative " .. tostring(world.hurt(8, 3, { down_to = 10 })))
	print(pcall(world.hurt, 7, 1, { down_to = 21 }) and "over ok" or "no over")
	print(pcall(world.hurt, 7, 1, { down_to = -1 }) and "under ok" or "no under")
	print(pcall(world.hurt, 7, 1, { down_to = 0/0 }) and "nan ok" or "no nan")
end)
"#;
    scripts.players = vec![PlayerInfo { id: 7, name: "Ada".into(), pos: Vec3::ZERO }, PlayerInfo { id: 8, name: "Bo".into(), pos: Vec3::ZERO }];
    scripts.set_pockets(7, true, &[], 0);
    scripts.set_pockets(8, false, &[], 0);
    scripts.start(&creation(code), &mut herd).unwrap();
    run(&mut scripts, &mut voxels, &mut herd, 0.3);
    let l = scripts.vms["test"].log.join("\n");
    assert_eq!(scripts.vms["test"].error, None, "{l}");
    for want in ["hurt true true true true", "heal true", "creative false", "no over", "no under", "no nan"] {
        assert!(l.contains(want), "{want}: {l}");
    }
    // the plain ones together (a heal has no down_to), then the ones that stop at 10
    assert_eq!(scripts.health_changes(), vec![(7, -2.0, 0.0), (7, -7.0, 10.0)]);

    // the host's own game: it stops at 10, and doesn't hurt at all below it
    for survival in [true, false] {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        crate::sim(&mut app, crate::SEED);
        app.insert_resource(Settings { survival, ..default() }).insert_resource(Me::default());
        app.init_resource::<crate::input::Controls>().init_resource::<crate::ui::Focus>().init_resource::<crate::ui::Toasts>();
        app.world_mut().resource_mut::<crate::ui::WorldInfo>().in_world = true;
        let bite = creation("local world = require(\"@potluck/world@0.7\")\nworld.after(0.1, function() for _, p in world.players() do world.hurt(p, 15, { down_to = 10 }) end end)");
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
        let me = app.world().resource::<Me>();
        assert_eq!((me.health_due, me.hurts.clone()), (0.0, if survival { vec![(15.0, 10.0)] } else { vec![] }), "survival {survival}");
        app.world_mut().resource_mut::<Me>().hurts = vec![(15.0, 10.0)]; // as if sent anyway
        app.world_mut().run_system_once(crate::player::walk).unwrap();
        let me = app.world().resource::<Me>();
        assert_eq!((me.health, me.hurts.len()), (if survival { 10.0 } else { MAX_HEALTH }, 0), "survival {survival}");
        // at or below it, nothing happens, not even the flash
        let mut me = app.world_mut().resource_mut::<Me>();
        me.health = 8.0;
        me.hurt = 9.0;
        me.hurts = vec![(3.0, 10.0)];
        app.world_mut().run_system_once(crate::player::walk).unwrap();
        let me = app.world().resource::<Me>();
        assert!(me.health == 8.0 && me.hurt >= 9.0 && me.hurts.is_empty(), "survival {survival}: {} {}", me.health, me.hurt);
    }

    // what goes between devices
    let m = Msg::HurtTo(7.0, 10.0);
    let Some(Event::Msg(_, back)) = decode(Wire::Data(1, frame_msg(&m))) else { panic!("didn't decode") };
    assert_eq!(serde_json::to_string(&back).unwrap(), serde_json::to_string(&m).unwrap());
}

/// The Gnome Gardeners tell survival players how to get by.
#[test]
fn gnomes_tell_survival_players_how_to_get_by() {
    home();
    let gnome = Library::load().list.remove("gnome").expect("the base game has the gnomes");
    let mut voxels = world();
    let mut herd = Herd::default();
    let mut scripts = Scripts::default();
    let ada = PlayerInfo { id: 7, name: "Ada".into(), pos: Vec3::ZERO };
    let bo = PlayerInfo { id: 8, name: "Bo".into(), pos: Vec3::ZERO };
    scripts.players = vec![ada.clone(), bo.clone()];
    scripts.set_pockets(7, true, &[], 0);
    scripts.set_pockets(8, false, &[], 0);
    scripts.start(&gnome, &mut herd).unwrap();
    run(&mut scripts, &mut voxels, &mut herd, 0.3);
    let id = herd.bodies.values().find(|b| b.creation == "gnome").map(|b| b.id).expect("a gardener");
    let asked = |scripts: &Scripts, pid: u32| scripts.shared.lock().unwrap().ui.panels.get(&pid).and_then(|q| q.front().cloned());
    scripts.event(&mut voxels, &mut herd, id, "used", &ada);
    scripts.event(&mut voxels, &mut herd, id, "used", &bo);
    run(&mut scripts, &mut voxels, &mut herd, 0.2);
    let (a, b) = (asked(&scripts, 7).unwrap(), asked(&scripts, 8).unwrap());
    assert_eq!(a.choices.last().map(String::as_str), Some("Getting by"), "{:?}", a.choices);
    assert_eq!(b.choices.last().map(String::as_str), Some("Playing together"), "{:?}", b.choices);
    scripts.answer(7, a.ask, Some(a.choices.len() as u32 - 1));
    run(&mut scripts, &mut voxels, &mut herd, 0.2);
    let topic = asked(&scripts, 7).unwrap();
    assert!(topic.text.contains("workbench") && topic.text.ends_with("can't bear light."), "{}", topic.text);
    assert_eq!(scripts.vms["gnome"].error, None);
}

/// One message to the dev bridge over HTTP, answered while this thread plays
/// the game's part: (status, body, helpers who said hello).
fn bridge_post(open: &crate::bridge::Open, cx: &mut crate::bridge::Cx, method: &str, headers: &str, body: &str) -> (u16, String, Vec<String>) {
    use crate::bridge::{answer, Call};
    let port = open.port;
    let host = if headers.lines().any(|l| l.to_ascii_lowercase().starts_with("host:")) { String::new() } else { format!("Host: 127.0.0.1:{port}\r\n") };
    let req = format!("{method} /mcp HTTP/1.1\r\n{host}{headers}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.write_all(req.as_bytes()).unwrap();
        let mut out = String::new();
        let _ = s.read_to_string(&mut out);
        let _ = tx.send(out);
    });
    let mut hellos = Vec::new();
    let until = Instant::now() + Duration::from_secs(20);
    loop {
        for call in open.calls() {
            match call {
                Call::Hello(who) => hellos.push(who),
                Call::Tool { name, args, reply } => {
                    let _ = reply.send(answer(&name, &args, cx));
                }
            }
        }
        if let Ok(out) = rx.try_recv() {
            let (head, body) = out.split_once("\r\n\r\n").unwrap_or((&out, ""));
            let status = head.split_whitespace().nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
            return (status, body.to_string(), hellos);
        }
        assert!(Instant::now() < until, "the dev bridge never answered");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// A tool call to the dev bridge: (its text, whether it's an error).
fn bridge_tool(open: &crate::bridge::Open, cx: &mut crate::bridge::Cx, tool: &str, args: serde_json::Value) -> (String, bool) {
    let body = serde_json::json!({ "jsonrpc": "2.0", "id": 9, "method": "tools/call", "params": { "name": tool, "arguments": args } }).to_string();
    let (status, body, _) = bridge_post(open, cx, "POST", "", &body);
    assert_eq!(status, 200, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    let result = &v["result"];
    let text = result["content"].as_array().unwrap().iter().filter_map(|c| c["text"].as_str()).collect::<Vec<_>>().join("\n");
    (text, result["isError"].as_bool().unwrap())
}

/// The dev bridge: a coding helper on this computer makes a creation in the
/// player's folder, starts it, reads its log, runs a line in it, turns a
/// knob, looks around and sees a mistake's line, all over MCP; nothing but
/// programs on this computer can reach it.
#[test]
fn a_coding_helper_works_on_creations_through_the_dev_bridge() {
    use crate::bridge::{self, Cx};
    use serde_json::json;
    home();
    let id = "bridge-moths";
    let dir = crate::creations::my_folder().join(id);
    struct Gone(std::path::PathBuf);
    impl Drop for Gone {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _gone = Gone(dir.clone());
    let open = bridge::listen(0).unwrap();
    let mut voxels = world();
    let (mid, _) = pad(&mut voxels);
    let mut lib = Library::load();
    let (mut scripts, mut herd, mut net) = (Scripts::default(), Herd::default(), crate::net::Net::default());
    let (mut toasts, mut edits) = (crate::ui::Toasts::default(), crate::KnobEdits::default());
    let mut me = crate::player::Me::default();
    me.pos = mid.as_vec3() + Vec3::new(3.0, 0.0, 0.0);
    let mut cx = Cx { lib: &mut lib, scripts: &mut scripts, herd: &mut herd, net: &mut net, toasts: &mut toasts, voxels: &mut voxels, edits: &mut edits, me: Some(&me), halves: None, in_world: true, survival: false, day: 0.5 };

    // the handshake
    let hello = json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "test helper", "version": "1" } } }).to_string();
    let (status, body, hellos) = bridge_post(&open, &mut cx, "POST", "", &hello);
    assert_eq!(status, 200, "{body}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(v["result"]["serverInfo"]["name"], "potluck");
    assert!(v["result"]["instructions"].as_str().unwrap().contains(&crate::creations::my_folder().display().to_string()), "says where creations go");
    assert_eq!(hellos, vec!["test helper".to_string()]);
    let (status, ..) = bridge_post(&open, &mut cx, "POST", "", &json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }).to_string());
    assert_eq!(status, 202, "notifications get no answer");
    let ping = json!({ "jsonrpc": "2.0", "id": 2, "method": "ping" }).to_string();
    // only this computer: not a name pointed at it, not a web page
    assert_eq!(bridge_post(&open, &mut cx, "POST", "Host: evil.example\r\n", &ping).0, 403);
    assert_eq!(bridge_post(&open, &mut cx, "POST", "Origin: http://evil.example\r\n", &ping).0, 403);
    assert_eq!(bridge_post(&open, &mut cx, "POST", &format!("Origin: http://localhost:{}\r\n", open.port), &ping).0, 200);
    assert_eq!(bridge_post(&open, &mut cx, "GET", "", "").0, 405, "no stream from the bridge");
    let (_, body, _) = bridge_post(&open, &mut cx, "POST", "", &json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/list" }).to_string());
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    let names: Vec<&str> = v["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["creations", "guide", "reload", "log", "run", "test", "check", "tweak", "look_around", "screenshot"]);
    let (_, body, _) = bridge_post(&open, &mut cx, "POST", "", &json!({ "jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": { "name": "fly" } }).to_string());
    assert!(body.contains("-32602"), "an unknown tool is a protocol error: {body}");

    // a new creation, made in the player's folder and started by the helper
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("potluck.toml"), "name = \"Lantern moths\"\nby = \"a helper\"\nkind = \"creature\"\nmain = \"main.luau\"\n").unwrap();
    let code = |bad: bool| {
        format!(
            r##"local world = require("@potluck/world")
local tweak = require("@potluck/tweak")
{}
local knobs = tweak {{ glow = {{ 2, min = 0, max = 5, label = "Glow" }} }}
print("moths awake")
local Moth = world.creature {{
	name = "Lantern moth",
	count = 2,
	home = {{ {}, {}, {} }},
	size = {{ 0.5, 0.5, 0.5 }},
	body = {{ {{ "wing", size = {{ 0.5, 0.1, 0.3 }}, at = {{ 0, 0.25, 0 }}, color = "#ffd27f" }} }},
}}
function Moth:spawned()
	self.flaps = 0
end
function Moth:tick(dt)
	self.flaps += 1
end
"##,
            if bad { "local oops = = 1" } else { "" },
            mid.x,
            mid.y,
            mid.z
        )
    };
    std::fs::write(dir.join("main.luau"), code(false)).unwrap();
    let (text, err) = bridge_tool(&open, &mut cx, "reload", json!({ "creation": id }));
    assert!(!err && text.contains("Lantern moths is running") && text.contains("Lantern moth"), "{text}");
    assert!(dir.join("AGENTS.md").is_file() && dir.join("types/potluck.d.luau").is_file(), "a new folder gets the guide and types");
    assert!(cx.lib.running.iter().any(|r| r == id));
    run(cx.scripts, cx.voxels, cx.herd, 0.5);
    assert_eq!(cx.herd.bodies.values().filter(|b| b.creation == id).count(), 2);
    let (text, err) = bridge_tool(&open, &mut cx, "log", json!({ "creation": id }));
    assert!(!err && text.contains("moths awake"), "{text}");
    // a line run inside it sees what it sees, and prints to its log
    let (text, err) = bridge_tool(&open, &mut cx, "run", json!({ "creation": id, "code": "print('hello from run') local world = require('@potluck/world') return #world.creatures(), 'ok'" }));
    assert!(!err && text.contains("hello from run") && text.contains("returned: 2, \"ok\""), "{text}");
    assert!(cx.scripts.vms[id].log.iter().any(|l| l == "hello from run"));
    let (text, err) = bridge_tool(&open, &mut cx, "run", json!({ "creation": id, "code": "error('nope')" }));
    assert!(err && text.contains("nope"), "{text}");
    assert!(cx.scripts.vms[id].error.is_none(), "a helper's mistake isn't the creation's");
    // the world near the player, with what the moths' code keeps on them
    let (text, err) = bridge_tool(&open, &mut cx, "look_around", json!({}));
    let v: serde_json::Value = serde_json::from_str(&text).unwrap_or_else(|_| panic!("{text}"));
    assert!(!err && v["in_world"] == true && v["you"]["mode"] == "build");
    let moths: Vec<&serde_json::Value> = v["creatures"].as_array().unwrap().iter().filter(|c| c["creation"] == id).collect();
    assert_eq!(moths.len(), 2, "{text}");
    assert!(moths[0]["fields"]["flaps"].as_f64().unwrap() >= 1.0, "{text}");
    assert_eq!(v["blocks_within_8"]["Stone"].as_u64().map(|n| n > 0), Some(true), "{text}");
    // knobs, as Tweak turns them
    let (text, err) = bridge_tool(&open, &mut cx, "tweak", json!({ "creation": id, "knob": "glow", "value": 4 }));
    assert!(!err && text.contains("Glow"), "{text}");
    assert!(matches!(cx.edits.0.last(), Some((c, k, KnobValue::Number(n))) if c == id && k == "glow" && *n == 4.0));
    assert!(bridge_tool(&open, &mut cx, "tweak", json!({ "creation": id, "knob": "glow", "value": true })).1, "a number knob takes numbers");
    // a mistake says where, and the last good version keeps running
    std::fs::write(dir.join("main.luau"), code(true)).unwrap();
    let (text, err) = bridge_tool(&open, &mut cx, "reload", json!({ "creation": id }));
    assert!(err && text.contains("main.luau:3") && text.contains("still running"), "{text}");
    run(cx.scripts, cx.voxels, cx.herd, 0.3);
    assert_eq!(cx.herd.bodies.values().filter(|b| b.creation == id).count(), 2, "the moths are still about");
    let (text, _) = bridge_tool(&open, &mut cx, "creations", json!({}));
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    let mine = v["creations"].as_array().unwrap().iter().find(|c| c["id"] == id).unwrap();
    assert!(mine["running"] == true && mine["folder"].as_str().is_some() && mine["knobs"]["glow"]["label"] == "Glow" && mine["error"].as_str().is_some_and(|e| e.contains("main.luau:3")), "{mine}");
    // its budgets, as Look inside shows them
    let budgets = mine["budgets"].as_str().unwrap_or_default();
    assert!(budgets.starts_with("a tick takes ") && budgets.contains("of its 15") && budgets.contains("of 32 MB"), "{budgets}");
    assert!(cx.scripts.vms[id].tick_ms > 0.0, "its ticks are timed");
    assert!(v["creations"].as_array().unwrap().iter().any(|c| c["id"] == "mossback" && c["origin"] == "the base game"));
    let (text, err) = bridge_tool(&open, &mut cx, "guide", json!({}));
    assert!(!err && text.contains("@potluck/world"));
    // its own tests, from its folder as saved, in a copy of the world
    let (text, err) = bridge_tool(&open, &mut cx, "test", json!({ "creation": id }));
    assert!(err && text.contains("has no test.luau"), "{text}");
    std::fs::write(dir.join("main.luau"), code(false)).unwrap();
    let tests = r##"local test = require("@potluck/test@0.1")
local world = require("@potluck/world")
test.case("two moths", function()
	test.same(#world.creatures(test.kind("Lantern moth")), 2, "moths")
end)
test.case("three moths", function()
	test.same(#world.creatures(), 3, "moths")
end)
"##;
    std::fs::write(dir.join("test.luau"), tests).unwrap();
    let before = cx.herd.bodies.len();
    let (text, err) = bridge_tool(&open, &mut cx, "test", json!({ "creation": id }));
    assert!(!err && text.contains("1 of 2 cases passed") && text.contains("FAIL  three moths: test.luau:7: moths: got 2, wanted 3"), "{text}");
    assert_eq!(cx.herd.bodies.len(), before, "the world's own creatures are left alone");
    let (text, err) = bridge_tool(&open, &mut cx, "test", json!({ "creation": "balloon" }));
    assert!(!err && text.contains("5 of 5 cases passed"), "{text}");
    // the type checker, on the folder as saved
    let (text, err) = bridge_tool(&open, &mut cx, "check", json!({ "creation": "balloon" }));
    assert!(!err && text.contains("found nothing it's sure is wrong"), "{text}");
    std::fs::write(dir.join("main.luau"), "local world = require(\"@potluck/world\")\nworld.spwan()\n").unwrap();
    let (text, err) = bridge_tool(&open, &mut cx, "check", json!({ "creation": id }));
    assert!(!err && text.contains("sure of 1 thing\nmain.luau line 2: world has no spwan (did you mean world.spawn?)"), "{text}");
    // a screenshot needs the game's screen, which this test hasn't got
    assert!(bridge_tool(&open, &mut cx, "screenshot", json!({})).1);
    drop(open);
}

// ---------- the map, waypoints, teleporting and chat ----------

#[test]
fn the_map_shows_the_world_from_above_and_keeps_up_with_changes() {
    use crate::map::{self, Column, MapImage, Palette};
    let mut v = world();
    let mut m = MapImage::default();
    m.draw_all(&v);
    // the sea round the island is the sea's blue; the hill at the start isn't
    let sea = m.at(2.0, 2.0);
    assert!(sea[2] > sea[0], "the sea far out is blue: {sea:?}");
    let (sx, sz) = (world::START.x, world::START.z);
    assert_ne!(m.at(sx, sz), sea, "land at the start");
    assert_eq!(m.height(sx, sz), v.top(sx as i32, sz as i32), "heights from above are the ground's");
    // deeper water is darker, and a hill's north face lighter than its south
    let p = Palette::default();
    let shallow = map::shade(&p, Column { block: crate::blocks::SAND, y: 17, water: 1 }, 17);
    let deep = map::shade(&p, Column { block: crate::blocks::SAND, y: 10, water: 8 }, 10);
    assert!(deep.iter().take(3).map(|&c| c as u32).sum::<u32>() < shallow.iter().take(3).map(|&c| c as u32).sum::<u32>());
    let up = map::shade(&p, Column { block: crate::blocks::GRASS, y: 30, water: 0 }, 28);
    let down = map::shade(&p, Column { block: crate::blocks::GRASS, y: 30, water: 0 }, 32);
    assert!(up[1] > down[1], "a column above its north neighbour is lit");
    // a tower built: its column, and only near it, is drawn again
    let (x, z) = (sx as i32 + 3, sz as i32 + 3);
    let top = v.top(x, z);
    for y in top + 1..top + 9 {
        v.set(IVec3::new(x, y, z), crate::blocks::PLANKS);
    }
    let rev = m.rev;
    let here = Vec2::new(sx, sz);
    m.draw_changes(&mut v, here);
    assert_eq!(m.height(x as f32, z as f32), top + 8);
    assert!(m.rev > rev && v.map_dirty.is_empty());
    // land past the island is drawn as it comes in, and kept when the map
    // moves away from it and back
    let far = IVec2::new(world::CX + 4, 3);
    let (fx, fz) = ((far.x * world::CHUNK + 9) as f32, (far.y * world::CHUNK + 9) as f32);
    assert_eq!(m.at(fx, fz), map::UNSEEN, "nobody's been there");
    v.ensure(far);
    m.draw_changes(&mut v, here);
    assert_ne!(m.at(fx, fz), map::UNSEEN, "drawn once it's made");
    assert_eq!(m.height(fx, fz), map::surface(&v, fx as i32, fz as i32, world::WY - 1).top());
    let seen = m.at(fx, fz);
    m.follow(&v, Vec2::new(5000.0, 5000.0));
    assert!(m.origin.x > 4000, "the picture follows the player");
    v.unload(far);
    m.follow(&v, here);
    assert_eq!(m.at(fx, fz), seen, "and what was seen is kept");
    // a different world is drawn whole
    let other = world::generate(7);
    let mut other = other;
    m.draw_changes(&mut other, here);
    assert_eq!(m.height(sx, sz), other.top(sx as i32, sz as i32));
}

#[test]
fn under_a_roof_the_map_shows_the_level_youre_on() {
    use crate::map;
    let mut v = world();
    let (x, z) = (world::START.x as i32, world::START.z as i32);
    let floor = v.top(x, z);
    let eye = Vec3::new(x as f32 + 0.5, floor as f32 + 2.6, z as f32 + 0.5);
    assert_eq!(map::roof_level(&v, eye), None, "out under the sky");
    for dx in -2..=2 {
        for dz in -2..=2 {
            v.set(IVec3::new(x + dx, floor + 5, z + dz), crate::blocks::PLANKS);
        }
    }
    assert_eq!(map::roof_level(&v, eye), Some(floor + 2), "a roof over you and round you");
    let mut m = map::MapImage::default();
    m.draw_all(&v);
    let from_above = m.at(x as f32, z as f32);
    m.show_level(&v, map::roof_level(&v, eye), Vec2::new(eye.x, eye.z));
    assert_ne!(m.at(x as f32, z as f32), from_above, "the floor, not the roof");
}

#[test]
fn the_minimap_is_round_with_you_in_the_middle() {
    use crate::map::{self, Dot, MapImage};
    let v = world();
    let mut m = MapImage::default();
    m.draw_all(&v);
    let size = 64;
    let mut out = vec![0u8; size * size * 4];
    let me = Vec2::new(world::START.x, world::START.z);
    let friend = Dot { at: me + Vec2::new(10.0, 0.0), rgb: [255, 0, 255], r: 3.0 };
    map::draw_minimap(&mut out, size, &m, me, map::facing(0.0), false, 1.0, &[friend]);
    let px = |x: usize, y: usize| &out[(y * size + x) * 4..(y * size + x) * 4 + 4];
    assert_eq!(px(0, 0)[3], 0, "outside the circle is clear");
    assert_eq!(px(32, 30), &[255, 255, 255, 255][..], "the arrow is in the middle");
    assert!(px(42, 32)[0] > 200 && px(42, 32)[1] < 80, "a friend ten blocks east is ten pixels right: {:?}", px(42, 32));
    // turned with you, facing east: the friend is straight up
    let mut out2 = vec![0u8; size * size * 4];
    map::draw_minimap(&mut out2, size, &m, me, Vec2::X, true, 1.0, &[Dot { at: me + Vec2::new(20.0, 0.0), rgb: [255, 0, 255], r: 3.0 }]);
    let p = &out2[(12 * size + 32) * 4..(12 * size + 32) * 4 + 4];
    assert!(p[0] > 200 && p[1] < 80, "ahead is up: {p:?}");
    assert_eq!(map::next_zoom("far"), "off");
    assert_eq!(map::zoom_of("off"), 0.0);
}

#[test]
fn waypoints_are_kept_for_each_world_and_know_where_youve_stood() {
    use crate::waypoints::{world_key, Kind, Waypoints};
    home();
    let mut w = Waypoints::load();
    w.here = world_key(true, None, 26);
    assert_eq!(w.here, "seed 26");
    let home_at = IVec3::new(100, 30, 100);
    let a = w.add("Home", home_at, true, Kind::Mine).unwrap();
    let b = w.add("", IVec3::new(200, 0, 50), false, Kind::Mine).unwrap();
    assert_eq!(w.get(b).unwrap().name, "Place 2", "a name when none is given");
    assert_eq!(w.next_name(), "Place 1");
    assert!(w.get(a).unwrap().stood && !w.get(b).unwrap().stood);
    assert_ne!(w.get(a).unwrap().color, w.get(b).unwrap().color, "neighbours differ");
    // marked on the map, it gets its height when you stand there
    assert!(w.stand(Vec3::new(201.0, 41.0, 51.0)));
    assert_eq!(w.get(b).unwrap().at, [200, 41, 50]);
    assert!(w.get(b).unwrap().stood);
    w.fell(IVec3::new(5, 20, 5));
    w.fell(IVec3::new(6, 21, 6));
    let fell: Vec<_> = w.list().iter().filter(|p| p.kind == Kind::Fell).collect();
    assert_eq!(fell.len(), 1, "one mark for where you fell, moved");
    assert_eq!((fell[0].at, fell[0].letter()), ([6, 21, 6], "X".to_string()));
    w.save();
    // another world has its own
    let mut again = Waypoints::load();
    again.here = world_key(true, Some("Play.Example.org:47126/"), 26);
    assert!(again.list().is_empty(), "{:?}", again.list());
    assert!(again.add("Spawn", IVec3::new(1, 1, 1), true, Kind::Mine).is_some());
    again.here = "seed 26".into();
    assert_eq!(again.list().len(), 3, "saved and loaded");
    again.remove(a);
    assert_eq!(again.list().len(), 2);
    again.here = String::new();
    assert!(again.add("Nowhere", IVec3::ZERO, true, Kind::Mine).is_none(), "outside a world there's nowhere to mark");
}

#[test]
fn teleporting_is_free_in_build_mode_and_earned_in_survival() {
    use crate::waypoints::{may_go, Goal, WAIT};
    let mine = Goal::Waypoint { stood: true };
    let unseen = Goal::Waypoint { stood: false };
    assert_eq!(may_go(&Goal::Spot, false, true, false, 0.0), Ok(0.0), "anywhere at once when building");
    assert_eq!(may_go(&Goal::Player, false, true, false, 50.0), Ok(0.0));
    assert_eq!(may_go(&mine, true, true, false, 0.0), Ok(WAIT), "your own places, after standing still");
    assert!(may_go(&unseen, true, true, false, 0.0).unwrap_err().contains("stood"));
    assert!(may_go(&Goal::Spot, true, true, false, 0.0).is_err());
    assert!(may_go(&Goal::Player, true, true, false, 0.0).is_err());
    assert!(may_go(&mine, true, true, false, 12.0).unwrap_err().contains("12 s"), "and not again straight away");
    assert!(may_go(&mine, false, false, false, 0.0).is_err(), "a world can turn it off");
    assert!(may_go(&mine, false, true, true, 0.0).is_err(), "not while riding");
}

#[test]
fn a_trip_fades_lands_on_open_ground_and_stops_if_you_move() {
    use crate::player::Me;
    use crate::waypoints::{landing, Travel, REST};
    let mut v = world();
    let (x, z) = (world::START.x as i32 + 20, world::START.z as i32 - 10);
    // a spot picked on the map lands on the ground, never in it
    let at = landing(&v, Vec3::new(x as f32, 0.0, z as f32), false);
    let feet = at.floor().as_ivec3();
    assert!(!crate::blocks::is_solid(v.at(feet)) && !crate::blocks::is_solid(v.at(feet + IVec3::Y)), "room to stand");
    assert!(crate::blocks::is_solid(v.at(feet - IVec3::Y)) || v.at(feet - IVec3::Y) == crate::blocks::WATER, "something under you");
    // a waypoint in a cave lands in the cave; one now filled in, on top
    let top = v.top(x, z);
    for y in top - 6..=top - 4 {
        v.set(IVec3::new(x, y, z), crate::blocks::AIR);
    }
    let cave = landing(&v, Vec3::new(x as f32, (top - 6) as f32, z as f32), true);
    assert_eq!(cave.y.floor() as i32, top - 6);
    let buried = landing(&v, Vec3::new(x as f32, (top - 20) as f32, z as f32), true);
    assert!(buried.y > top as f32, "not inside the rock");

    // survival: stand still, the screen goes dark, you're there, it comes back
    let mut me = Me::default();
    me.respawn(&v, None);
    let mut t = Travel::default();
    t.start(&me, Vec3::new(x as f32, 0.0, z as f32), false, "the hill", 3.0);
    let mut said = vec![];
    let mut darkest: f32 = 0.0;
    for _ in 0..200 {
        said.extend(t.step(0.05, &mut me, &v, true));
        darkest = darkest.max(t.dark);
    }
    assert!(!t.going() && t.dark == 0.0, "the trip is over and the screen clear");
    assert_eq!(darkest, 1.0, "the screen went dark rather than the camera moving");
    assert_eq!(me.pos.floor().as_ivec3().xz(), IVec2::new(x, z));
    assert!(said.iter().any(|s| s.contains("the hill")));
    assert!(t.rest > REST - 10.0, "a rest before the next");
    // moving while it counts down stops it
    let mut t = Travel::default();
    t.start(&me, Vec3::new(10.0, 0.0, 10.0), false, "far", 3.0);
    t.step(0.05, &mut me, &v, true);
    me.pos.x += 2.0;
    let stop = t.step(0.05, &mut me, &v, true);
    assert!(stop.is_some_and(|s| s.contains("Stopped")) && !t.going());
}

#[test]
fn chat_hides_listed_words_keeps_younger_players_to_quick_lines_and_slows_floods() {
    use crate::chat::{Chat, Say, Speaker};
    let mut chat = Chat::default();
    let say = |t: &str| Say { text: t.into(), place: None };
    let drew = Speaker { id: 2, name: "Drew", young: false };
    let l = chat.hear(Speaker { ..drew }, say("well   that's   fucking great"), true, 0.0).unwrap();
    assert_eq!(l.text, "well that's ******* great", "tidied and hidden");
    let l = chat.hear(Speaker { ..drew }, say("what a SH1T idea"), true, 1.0).unwrap();
    assert_eq!(l.text, "what a **** idea", "look-alikes too");
    let l = chat.hear(Speaker { ..drew }, say("a classic class assessment in Scunthorpe"), true, 2.0).unwrap();
    assert_eq!(l.text, "a classic class assessment in Scunthorpe", "and not words that only hold one");
    chat.hear(Speaker { ..drew }, say("four"), true, 3.0).unwrap();
    chat.hear(Speaker { ..drew }, say("five"), true, 4.0).unwrap();
    assert!(chat.hear(Speaker { ..drew }, say("six"), true, 5.0).unwrap_err().contains("Slow down"));
    assert!(chat.hear(Speaker { ..drew }, say("later"), true, 20.0).is_ok(), "a while later is fine");
    assert!(chat.hear(Speaker { ..drew }, say("   "), true, 30.0).is_err(), "nothing to say");
    assert!(chat.hear(Speaker { ..drew }, say("hello"), false, 40.0).unwrap_err().contains("off"), "a world can turn chat off");

    let cy = Speaker { id: 3, name: "Cy", young: true };
    assert!(chat.hear(Speaker { ..cy }, say("my address is"), true, 0.0).unwrap_err().contains("quick lines"));
    let l = chat.hear(Speaker { ..cy }, say("Nice build!"), true, 1.0).unwrap();
    assert!(l.young && !l.may_leave(), "a younger player's lines stay in the world");
    chat.keep(l, 1.0);
    // a place, with its name tidied
    let l = chat.hear(Speaker { ..drew }, Say { text: String::new(), place: Some(crate::chat::Place { name: "  the   mill ".into(), at: [10, 30, 10] }) }, true, 50.0).unwrap();
    assert_eq!(l.place.as_ref().unwrap().name, "the mill");
    assert!(l.may_leave());
    chat.keep(l, 50.0);
    assert!(chat.hear(Speaker { ..drew }, Say { text: String::new(), place: Some(crate::chat::Place { name: "x".into(), at: [crate::world::BORDER + 5, 30, 10] }) }, true, 60.0).is_err(), "places in the world only");
    assert_eq!(chat.places(99).len(), 1);
    assert_eq!(chat.outbox.len(), 1, "only Drew's line may go to a bridge");
}

#[test]
fn chat_goes_through_the_host_to_everyone_and_nothing_from_outside_reaches_younger_players() {
    use crate::chat::{Chat, Line, Say, Source};
    use crate::net::{self, Event, Msg};
    home();
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    crate::sim(&mut app, crate::SEED);
    app.insert_resource(crate::settings::Settings { name: "Ken's world".into(), ..Default::default() });
    app.update();
    let port = free_port();
    app.world_mut().resource_mut::<net::Net>().host_with(net::HostOptions { name: "Ken's world".into(), port, tls: None, beacon: false, rules: Default::default() }).unwrap();
    // a player who joins, and what reaches them
    struct Guest {
        to: std::sync::mpsc::Sender<Vec<u8>>,
        from: Receiver<Wire>,
        lines: Vec<Line>,
        toasts: Vec<String>,
        welcome: Option<(bool, bool)>,
    }
    let join = |name: &str, young: bool| {
        let (tx, rx) = channel();
        let to = transport::connect(&format!("127.0.0.1:{port}"), port, tx).unwrap();
        to.send(net::frame_msg(&Msg::Hello { name: name.into(), protocol: net::PROTOCOL, pass: None, young })).unwrap();
        Guest { to, from: rx, lines: vec![], toasts: vec![], welcome: None }
    };
    let pump = |app: &mut App, guests: &mut [&mut Guest], until: &dyn Fn(&[&mut Guest]) -> bool| {
        let t = Instant::now();
        while t.elapsed() < Duration::from_secs(20) && !until(guests) {
            app.update();
            std::thread::sleep(Duration::from_millis(5));
            for g in guests.iter_mut() {
                for w in g.from.try_iter() {
                    match net::decode(w) {
                        Some(Event::Msg(_, Msg::Welcome { teleport, chat, .. })) => g.welcome = Some((teleport, chat)),
                        Some(Event::Msg(_, Msg::Chat(l))) => g.lines.extend(l),
                        Some(Event::Msg(_, Msg::Toast(t))) => g.toasts.push(t),
                        _ => {}
                    }
                }
            }
        }
    };
    let mut drew = join("Drew", false);
    let mut cy = join("Cy", true);
    pump(&mut app, &mut [&mut drew, &mut cy], &|g| g.iter().all(|g| g.welcome.is_some()));
    assert_eq!(drew.welcome, Some((true, true)), "a friend's world lets players teleport and chat");
    let say = |g: &Guest, t: &str| g.to.send(net::frame_msg(&Msg::Say(Say { text: t.into(), place: None }))).unwrap();
    say(&drew, "hello everyone");
    // two sockets race, so Cy speaks once Drew's line is in
    pump(&mut app, &mut [&mut drew, &mut cy], &|g| !g[0].lines.is_empty());
    say(&cy, "where do you live");
    say(&cy, "Hi!");
    pump(&mut app, &mut [&mut drew, &mut cy], &|g| g.iter().all(|g| g.lines.len() >= 2) && !g[1].toasts.is_empty());
    let texts = |g: &Guest| g.lines.iter().map(|l| format!("{}: {}", l.name, l.text)).collect::<Vec<_>>();
    assert_eq!(texts(&drew), ["Drew: hello everyone", "Cy: Hi!"]);
    assert_eq!(texts(&cy), ["Drew: hello everyone", "Cy: Hi!"]);
    assert!(cy.toasts.iter().any(|t| t.contains("quick lines")), "{:?}", cy.toasts);
    // a line from outside the world (a bridge, later) reaches Drew but not Cy
    app.world_mut().resource_mut::<Chat>().inbox.push(Line { id: 0, name: "Kaden".into(), from: Source::Bridge("Fluxer".into()), text: "hi from outside".into(), place: None, young: false, to: None, at: 0 });
    pump(&mut app, &mut [&mut drew, &mut cy], &|g| g[0].lines.len() >= 3);
    assert_eq!(drew.lines[2].who(), "Kaden on Fluxer");
    for _ in 0..20 {
        app.update();
    }
    for w in cy.from.try_iter() {
        if let Some(Event::Msg(_, Msg::Chat(l))) = net::decode(w) {
            cy.lines.extend(l);
        }
    }
    assert_eq!(cy.lines.len(), 2, "younger players never get a bridge's lines");
    let out: Vec<String> = app.world().resource::<Chat>().outbox.iter().map(|l| l.text.clone()).collect();
    assert_eq!(out, ["hello everyone"], "and their own never go out");
    // whoever joins later gets the last lines, minus the bridge's when younger
    let mut kaden = join("Kaden", false);
    let mut dot = join("Dot", true);
    pump(&mut app, &mut [&mut kaden, &mut dot], &|g| g[0].lines.len() >= 3 && g[1].lines.len() >= 2);
    assert_eq!(kaden.lines.len(), 3);
    assert!(dot.lines.iter().all(|l| !matches!(l.from, Source::Bridge(_))), "{:?}", texts(&dot));
}

#[test]
fn slash_commands_read_what_players_type() {
    use crate::commands::{parse, Command, Coord};
    let ok = |t: &str| parse(t).unwrap().unwrap();
    let err = |t: &str| parse(t).unwrap().unwrap_err();
    assert!(parse("hello / goodbye").is_none(), "a line, not a command");
    assert_eq!(ok("/msg Kaden come and see"), Command::Msg("Kaden come and see".into()));
    assert_eq!(ok("/TELL Kaden hi"), Command::Msg("Kaden hi".into()));
    assert_eq!(ok("/w Kaden hi"), Command::Msg("Kaden hi".into()));
    assert!(err("/msg").contains("/msg NAME"));
    assert_eq!(ok("/r on my way"), Command::Reply("on my way".into()));
    assert_eq!(ok("/tp 10 40 -20.5"), Command::Tp(Coord::At(10.0), Some(Coord::At(40.0)), Coord::At(-20.5)));
    assert_eq!(ok("/tp ~ ~10 ~-3"), Command::Tp(Coord::On(0.0), Some(Coord::On(10.0)), Coord::On(-3.0)));
    assert_eq!(ok("/tp 100 -200"), Command::Tp(Coord::At(100.0), None, Coord::At(-200.0)), "on the ground there");
    assert_eq!(ok("/tp Ken V"), Command::TpTo("Ken V".into()), "names may have spaces");
    assert_eq!(ok("/tp 7"), Command::TpTo("7".into()), "one word is a name");
    assert_eq!(ok("/tp inf 1 nan"), Command::TpTo("inf 1 nan".into()), "and nowhere is not a place");
    assert!(err("/tp 1 2 3 4").contains("/tp X Y Z"));
    assert!(err("/tp").contains("/tp NAME"));
    assert_eq!(ok("/gamemode survival"), Command::Mode { survival: true });
    assert_eq!(ok("/gamemode Creative"), Command::Mode { survival: false });
    assert_eq!(ok("/gamemode c"), Command::Mode { survival: false });
    assert!(err("/gamemode spectator").contains("survival and creative"));
    assert_eq!(ok("/help"), Command::Help);
    assert!(err("/fly").contains("There's no /fly"));
    assert!(err("/").contains("after the slash"));
    assert_eq!(Coord::On(-3.0).of(10.0), 7.0);
    // names, longest first, in any case
    let here = |id: u32, name: &str| crate::commands::Player { id, name: name.into(), young: false, pos: Vec3::ZERO };
    let all = [here(2, "Ken"), here(3, "Ken V"), here(4, "Bo"), here(5, "bo")];
    let (p, rest) = crate::commands::named(&all, "ken v hello there").unwrap();
    assert_eq!((p.id, rest), (3, "hello there"));
    let (p, rest) = crate::commands::named(&all, "KEN hi").unwrap();
    assert_eq!((p.id, rest), (2, "hi"));
    assert!(crate::commands::named(&all, "Bo hi").unwrap_err().contains("2 players"));
    assert!(crate::commands::named(&all, "Kenny hi").unwrap_err().contains("Nobody called Kenny"));
}

#[test]
fn private_messages_reach_only_whoever_theyre_for_and_never_younger_players() {
    use crate::chat::{Chat, Line, Say};
    use crate::net::{self, Event, Msg};
    home();
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    crate::sim(&mut app, crate::SEED);
    app.insert_resource(crate::settings::Settings { name: "Ken's world".into(), ..Default::default() });
    app.update();
    let port = free_port();
    app.world_mut().resource_mut::<net::Net>().host_with(net::HostOptions { name: "Ken's world".into(), port, tls: None, beacon: false, rules: Default::default() }).unwrap();
    struct Guest {
        to: std::sync::mpsc::Sender<Vec<u8>>,
        from: Receiver<Wire>,
        lines: Vec<Line>,
        toasts: Vec<String>,
        found: Vec<(String, [f32; 3])>,
        id: Option<u32>,
    }
    let join = |name: &str, young: bool| {
        let (tx, rx) = channel();
        let to = transport::connect(&format!("127.0.0.1:{port}"), port, tx).unwrap();
        to.send(net::frame_msg(&Msg::Hello { name: name.into(), protocol: net::PROTOCOL, pass: None, young })).unwrap();
        Guest { to, from: rx, lines: vec![], toasts: vec![], found: vec![], id: None }
    };
    let pump = |app: &mut App, guests: &mut [&mut Guest], until: &dyn Fn(&[&mut Guest]) -> bool| {
        let t = Instant::now();
        while t.elapsed() < Duration::from_secs(20) && !until(guests) {
            app.update();
            std::thread::sleep(Duration::from_millis(5));
            for g in guests.iter_mut() {
                for w in g.from.try_iter() {
                    match net::decode(w) {
                        Some(Event::Msg(_, Msg::Welcome { id, .. })) => g.id = Some(id),
                        Some(Event::Msg(_, Msg::Chat(l))) => g.lines.extend(l),
                        Some(Event::Msg(_, Msg::Toast(t))) => g.toasts.push(t),
                        Some(Event::Msg(_, Msg::Found { name, at })) => g.found.push((name, at)),
                        _ => {}
                    }
                }
            }
        }
    };
    let say = |g: &Guest, t: &str| g.to.send(net::frame_msg(&Msg::Say(Say { text: t.into(), place: None }))).unwrap();
    let mut drew = join("Drew", false);
    let mut kaden = join("Kaden", false);
    let mut cy = join("Cy", true);
    pump(&mut app, &mut [&mut drew, &mut kaden, &mut cy], &|g| g.iter().all(|g| g.id.is_some()));
    kaden.to.send(net::frame_msg(&Msg::Me { pos: [10.5, 40.0, -20.5], yaw: 0.0 })).unwrap();
    say(&drew, "/msg kaden come and see my fucking farm");
    pump(&mut app, &mut [&mut drew, &mut kaden, &mut cy], &|g| !g[0].lines.is_empty() && !g[1].lines.is_empty());
    let (kid, did) = (kaden.id.unwrap(), drew.id.unwrap());
    let l = &kaden.lines[0];
    assert_eq!((l.text.as_str(), l.to.as_ref().map(|t| t.id)), ("come and see my ******* farm", Some(kid)), "filtered, and for Kaden");
    assert_eq!(l.who_for(kid), "Drew to you");
    assert_eq!(drew.lines[0].who_for(did), "You to Kaden", "and back to Drew");
    assert!(!l.may_leave() && !l.for_young());
    // answering with /r
    say(&kaden, "/r on my way");
    pump(&mut app, &mut [&mut drew, &mut kaden, &mut cy], &|g| g[0].lines.len() >= 2);
    assert_eq!(drew.lines[1].text, "on my way");
    assert_eq!(drew.lines[1].who_for(did), "Kaden to you");
    // nothing private for younger players, either way
    say(&drew, "/msg Cy hi");
    say(&cy, "/msg Drew Hi!");
    // and commands that aren't, or that name nobody, go to nobody
    say(&drew, "/fly");
    say(&drew, "/msg Kenny hi");
    pump(&mut app, &mut [&mut drew, &mut kaden, &mut cy], &|g| g[0].toasts.len() >= 3 && !g[2].toasts.is_empty());
    assert!(drew.toasts.iter().any(|t| t == "Cy can't get private messages."), "{:?}", drew.toasts);
    assert!(drew.toasts.iter().any(|t| t.contains("There's no /fly")), "{:?}", drew.toasts);
    assert!(drew.toasts.iter().any(|t| t.contains("Nobody called Kenny")), "{:?}", drew.toasts);
    assert!(cy.toasts.iter().any(|t| t.contains("quick lines")), "{:?}", cy.toasts);
    // where someone is, for /tp NAME, but never a younger player
    say(&drew, "/tp Kaden");
    say(&drew, "/tp Cy");
    pump(&mut app, &mut [&mut drew, &mut kaden, &mut cy], &|g| !g[0].found.is_empty() && g[0].toasts.len() >= 4);
    assert_eq!(drew.found, [("Kaden".to_string(), [10.5, 40.0, -20.5])]);
    assert!(drew.toasts.iter().any(|t| t == "You can't teleport to Cy."), "{:?}", drew.toasts);
    for _ in 0..20 {
        app.update();
    }
    for w in cy.from.try_iter() {
        if let Some(Event::Msg(_, Msg::Chat(l))) = net::decode(w) {
            cy.lines.extend(l);
        }
    }
    assert!(cy.lines.is_empty(), "Cy saw none of it: {:?}", cy.lines);
    let chat = app.world().resource::<Chat>();
    assert!(chat.outbox.is_empty(), "private lines never leave the world");
    assert!(chat.history(false).is_empty(), "nor go to whoever joins");
    assert_eq!(chat.seen().count(), 0, "nor show to the host's own player");
}

#[test]
fn slash_commands_switch_modes_and_go_places_on_your_own_game() {
    use bevy::ecs::system::RunSystemOnce;
    use crate::settings::Settings;
    use crate::ui::{Act, Acts, Toasts};
    use crate::waypoints::Travel;
    home();
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    crate::sim(&mut app, crate::SEED);
    let mut me = crate::player::Me::default();
    me.pos = Vec3::new(5.5, 30.0, 5.5);
    app.insert_resource(Settings::default()).insert_resource(me).insert_resource(crate::KnobEdits::default()).init_resource::<Travel>();
    app.world_mut().resource_mut::<crate::ui::WorldInfo>().in_world = true;
    let run = |app: &mut App, text: &str| {
        app.world_mut().resource_mut::<Acts>().0.push(Act::Say(text.into()));
        app.world_mut().run_system_once(crate::chat::acts).unwrap();
    };
    let toasts = |app: &mut App| std::mem::take(&mut app.world_mut().resource_mut::<Toasts>().0).into_iter().map(|(t, _)| t).collect::<Vec<_>>();
    // building, a place on the map is a moment away
    run(&mut app, "/tp 100 40 -20");
    let trip = app.world().resource::<Travel>().trip.clone().expect("going");
    assert_eq!((trip.to, trip.exact, trip.name.as_str()), (Vec3::new(100.0, 40.0, -20.0), true, "100, 40, -20"));
    app.world_mut().resource_mut::<Travel>().trip = None;
    run(&mut app, "/tp ~10 ~-5");
    let trip = app.world().resource::<Travel>().trip.clone().expect("going");
    assert_eq!((trip.to.x, trip.to.z, trip.exact), (15.5, 0.5, false), "on from here, on the ground");
    app.world_mut().resource_mut::<Travel>().trip = None;
    run(&mut app, "/tp 40000 1 1");
    assert!(toasts(&mut app).iter().any(|t| t.contains("edge of the world")));
    // survival, as Settings switches it, and then no going anywhere
    run(&mut app, "/gamemode survival");
    assert!(app.world().resource::<Settings>().survival);
    run(&mut app, "/gamemode s");
    assert!(toasts(&mut app).iter().any(|t| t.contains("already")));
    run(&mut app, "/tp 100 40 -20");
    assert!(app.world().resource::<Travel>().trip.is_none());
    assert!(toasts(&mut app).iter().any(|t| t.contains("In survival")));
    run(&mut app, "/gamemode creative");
    assert!(!app.world().resource::<Settings>().survival);
    // a world that doesn't let players teleport
    app.world_mut().resource_mut::<crate::net::Net>().host_with(crate::net::HostOptions { name: "x".into(), port: free_port(), tls: None, beacon: false, rules: crate::hosting::Rules { teleport: false, ..Default::default() } }).unwrap();
    run(&mut app, "/tp 1 2 3");
    assert!(app.world().resource::<Travel>().trip.is_none());
    assert!(toasts(&mut app).iter().any(|t| t.contains("doesn't let players teleport")));
    app.world_mut().resource_mut::<crate::net::Net>().leave();
    // the host's commands go to it as lines would, and /tp NAME waits for its answer
    run(&mut app, "/msg Kaden hi");
    run(&mut app, "/tp Kaden");
    let out: Vec<String> = app.world().resource::<crate::chat::Chat>().out.iter().map(|s| s.text.clone()).collect();
    assert_eq!(out, ["/msg Kaden hi", "/tp Kaden"]);
    app.world_mut().resource_mut::<crate::chat::Chat>().found.push(("Kaden".into(), Vec3::new(50.0, 20.0, 50.0)));
    app.world_mut().run_system_once(crate::chat::acts).unwrap();
    let trip = app.world().resource::<Travel>().trip.clone().expect("off to Kaden");
    assert_eq!((trip.to, trip.name.as_str()), (Vec3::new(50.0, 20.0, 50.0), "Kaden"));
    app.world_mut().resource_mut::<Travel>().trip = None;
    // an answer nobody asked for goes nowhere
    app.world_mut().resource_mut::<crate::chat::Chat>().found.push(("Kaden".into(), Vec3::ZERO));
    app.world_mut().run_system_once(crate::chat::acts).unwrap();
    assert!(app.world().resource::<Travel>().trip.is_none());
    // /help, for this player only; typos say so
    run(&mut app, "/help");
    let chat = app.world().resource::<crate::chat::Chat>();
    let shown: Vec<String> = chat.seen().map(|(l, _)| l.who_for(chat.me)).collect();
    assert_eq!(shown.len(), crate::commands::HELP.len());
    assert!(shown[0].starts_with("/msg"), "{shown:?}");
    assert!(chat.history(false).is_empty(), "nobody else gets it");
    run(&mut app, "/gamemod creative");
    assert!(toasts(&mut app).iter().any(|t| t.contains("There's no /gamemod")));
}

#[test]
fn a_server_can_turn_teleporting_and_chat_off() {
    let cfg: crate::hosting::ServerConfig = toml::from_str("teleport = false\nchat = false").unwrap();
    let rules = cfg.rules();
    assert!(!rules.teleport && !rules.chat);
    let all = crate::hosting::ServerConfig::default().rules();
    assert!(all.teleport && all.chat, "both on unless a server says");
}

#[test]
fn the_keyboard_on_the_screen_types_with_a_pad() {
    use crate::keyboard::Keys;
    let mut k = Keys::new("say", "Say something", "", 8);
    // starts on q; right to w, press, then capitals for one letter
    k.step(1, 0);
    assert!(!k.press());
    k.step(0, 3); // down to the wide keys, Capitals
    k.col = 0;
    k.press();
    k.row = 1;
    k.col = 5; // y
    k.press();
    assert_eq!(k.text, "wY");
    k.step(0, -2); // up through the number row's neighbours wraps round
    assert!(k.row < 5);
    // Space, Delete and Done along the bottom
    k.row = 4;
    k.col = 1;
    k.press();
    k.col = 2;
    k.press();
    k.press();
    assert_eq!(k.text, "w");
    for _ in 0..20 {
        k.row = 0;
        k.col = 3;
        k.press();
    }
    assert_eq!(k.text, "w4444444", "no longer than the field takes");
    k.row = 4;
    k.col = 3;
    assert!(k.press(), "Done");
    // wrapping round the edges
    let mut k = Keys::new("x", "", "", 8);
    k.step(-1, 0);
    assert_eq!((k.row, k.col), (1, 9));
}

#[test]
fn a_pad_opens_the_keyboard_on_a_field_and_esc_from_a_menu_goes_back_to_playing() {
    use bevy::ecs::system::RunSystemOnce;
    use crate::input::Controls;
    use crate::ui::{Act, Acts, Focus, Screen};
    home();
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    crate::sim(&mut app, crate::SEED);
    app.insert_resource(crate::settings::Settings::default()).insert_resource(crate::player::Me::default()).insert_resource(crate::KnobEdits::default()).init_resource::<crate::waypoints::Travel>();
    app.world_mut().resource_mut::<crate::ui::WorldInfo>().in_world = true;
    // chat with a pad: A on the field brings up the keyboard
    app.world_mut().resource_mut::<Focus>().go(Screen::Chat);
    app.world_mut().run_system_once(crate::chat::fill).unwrap();
    let ctl = |f: &dyn Fn(&mut Controls)| {
        let mut c = Controls::default();
        f(&mut c);
        c
    };
    app.insert_resource(ctl(&|c| (c.accept, c.pad) = (true, true)));
    app.world_mut().run_system_once(crate::ui::menu_input).unwrap();
    let keys = app.world().resource::<Focus>().keyboard.clone().expect("the keyboard opens");
    assert_eq!(keys.field, "say");
    // typing and Done says it
    {
        let mut f = app.world_mut().resource_mut::<Focus>();
        let k = f.keyboard.as_mut().unwrap();
        k.text = "hello".into();
    }
    // (what the menu drew is what the keyboard's Done looks along)
    let made = app.world().resource::<Focus>().made.1.clone();
    app.world_mut().resource_mut::<Focus>().items = made;
    app.insert_resource(ctl(&|c| (c.pause, c.pad) = (true, true)));
    app.world_mut().run_system_once(crate::keyboard::input).unwrap();
    assert!(app.world().resource::<Focus>().keyboard.is_none());
    let acts = std::mem::take(&mut app.world_mut().resource_mut::<Acts>().0);
    assert_eq!(acts, [Act::Say("hello".into())]);
    app.world_mut().resource_mut::<Acts>().0 = acts;
    app.world_mut().run_system_once(crate::chat::acts).unwrap();
    assert_eq!(app.world().resource::<crate::chat::Chat>().out.len(), 1);
    assert!(app.world().resource::<Focus>().playing(), "saying it goes back to playing");

    // Esc on the pause menu: back to playing, and it doesn't open again
    app.world_mut().resource_mut::<Focus>().go(Screen::Pause);
    app.insert_resource(ctl(&|c| (c.back, c.pause) = (true, true)));
    app.world_mut().run_system_once(crate::ui::menu_input).unwrap();
    app.world_mut().run_system_once(crate::ui::menu_owns).unwrap();
    assert!(!app.world().resource::<Controls>().pause, "the menu had the key");
    assert!(app.world().resource::<Acts>().0.contains(&Act::Resume));
}

#[test]
fn a_picture_of_a_creation_is_kept_only_with_something_in_it() {
    let (w, h) = (100usize, 50usize);
    let sky: Vec<u8> = (0..w * h).flat_map(|_| [120u8, 160, 210, 255]).collect();
    assert!(!crate::photos::worth_keeping(&sky), "a flat sky isn't worth keeping");
    assert!(!crate::photos::worth_keeping(&[]), "nor is nothing");
    // a creature in the middle of the sky
    let mut shot = sky.clone();
    for y in 10..40 {
        for x in 30..70 {
            shot[(y * w + x) * 4..(y * w + x) * 4 + 3].copy_from_slice(&[60, 90, 40]);
        }
    }
    assert!(crate::photos::worth_keeping(&shot), "a creature against the sky is");
}

#[test]
fn settings_come_in_pages_and_creations_on_shelves_of_cards() {
    use bevy::ecs::system::RunSystemOnce;
    use crate::input::Controls;
    use crate::settings::Settings;
    use crate::ui::{menu_items, pages, Act, Acts, Focus, Kind, Screen, WorldInfo, SETTINGS_PAGES};
    home();
    let lib = Library::load();
    let scripts = Scripts::default();
    let settings = Settings::default();
    let info = WorldInfo { in_world: true, ..Default::default() };
    let mut focus = Focus::default();
    focus.go(Screen::Settings);
    // every setting is on one page or another, and only one
    let mut found = vec![];
    for n in 0..SETTINGS_PAGES.len() {
        focus.settings_page = n;
        let (title, items) = menu_items(&focus, &lib, &scripts, &settings, &info);
        assert_eq!(title, "Settings");
        assert!(items.iter().any(|i| i.kind == Kind::Button(Act::Back)), "{} goes back", SETTINGS_PAGES[n]);
        for i in items {
            match i.kind {
                Kind::Slider { key, .. } | Kind::Toggle { key, .. } | Kind::Field { key, .. } => found.push(key),
                Kind::Choice { act, .. } | Kind::Button(act) => found.push(format!("{act:?}")),
                _ => {}
            }
        }
    }
    for k in ["name", "survival", "mouse", "stick", "invert", "fov", "fullscreen", "volume", "subtitles", "Lighting", "server", "market", "bridge", "OpenControls", "OpenDevices", "OpenParents", "Market"] {
        assert_eq!(found.iter().filter(|f| f.as_str() == k || f.starts_with(&format!("{k}("))).count(), 1, "{k} on one page: {found:?}");
    }

    // shelves: what's in this world only in one
    focus.go(Screen::Library);
    let (shelves, _) = pages(&focus, &info).unwrap();
    assert!(shelves.contains(&"In this world"));
    assert!(!pages(&focus, &WorldInfo::default()).unwrap().0.contains(&"In this world"), "only in a world");
    focus.shelf = shelves.iter().position(|s| *s == "Made by Potluck").unwrap();
    let (_, items) = menu_items(&focus, &lib, &scripts, &settings, &info);
    let cards: Vec<String> = items.iter().filter_map(|i| if let Kind::Card(Act::Inside(id)) = &i.kind { Some(id.clone()) } else { None }).collect();
    assert!(cards.contains(&"mossback".to_string()), "{cards:?}");
    assert!(cards.iter().all(|id| lib.list[id].origin == Origin::Base));

    // LB and RB (or Tab) turn the pages; cards go along rows and down columns
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    crate::sim(&mut app, crate::SEED);
    app.insert_resource(Settings::default()).insert_resource(crate::player::Me::default()).insert_resource(crate::KnobEdits::default());
    app.world_mut().resource_mut::<WorldInfo>().in_world = true;
    app.world_mut().resource_mut::<Library>().running.clear();
    let press = |app: &mut App, f: &dyn Fn(&mut Controls)| {
        let mut c = Controls::default();
        f(&mut c);
        app.insert_resource(c);
        app.world_mut().run_system_once(crate::ui::menu_input).unwrap();
    };
    app.world_mut().resource_mut::<Focus>().go(Screen::Settings);
    press(&mut app, &|c| c.page = 1);
    assert_eq!(app.world().resource::<Focus>().settings_page, 1);
    press(&mut app, &|c| c.page = -1);
    press(&mut app, &|c| c.page = -1);
    assert_eq!(app.world().resource::<Focus>().settings_page, SETTINGS_PAGES.len() - 1, "round from the first to the last");
    app.world_mut().resource_mut::<Focus>().go(Screen::Library);
    app.world_mut().resource_mut::<Focus>().cols = 4;
    press(&mut app, &|c| c.down = true);
    press(&mut app, &|c| c.right = true);
    let f = app.world().resource::<Focus>();
    let (_, items) = menu_items(f, app.world().resource::<Library>(), app.world().resource::<Scripts>(), app.world().resource::<Settings>(), &info);
    let first = items.iter().position(|i| matches!(i.kind, Kind::Card(_))).unwrap();
    assert_eq!(f.sel, first + 5, "a row down and one along");
    let Kind::Card(Act::Inside(id)) = items[f.sel].kind.clone() else { panic!("a card") };
    // X brings it in; A looks inside
    press(&mut app, &|c| c.interact = true);
    press(&mut app, &|c| c.accept = true);
    assert_eq!(app.world().resource::<Acts>().0, [Act::Run(id.clone()), Act::Inside(id)]);
    // an empty shelf still goes back
    app.world_mut().resource_mut::<Acts>().0.clear();
    app.world_mut().resource_mut::<Focus>().shelf = 1;
    press(&mut app, &|c| c.back = true);
    assert_eq!(app.world().resource::<Acts>().0, [Act::Back]);
}

#[test]
fn the_maps_chats_and_keyboards_systems_ask_for_what_they_can_have_together() {
    // two queries that could reach the same thing mutably stop the game at
    // its start (Bevy's B0001), so each system is put together here
    fn check<M>(s: impl IntoSystem<(), (), M>) {
        let mut world = World::new();
        let mut s = IntoSystem::into_system(s);
        let _ = s.initialize(&mut world);
    }
    check(crate::map::draw_hud);
    check(crate::map::draw_map);
    check(crate::map::map_input);
    check(crate::map::map_acts);
    check(crate::map::refresh);
    check(crate::map::make_room);
    check(crate::map::fill);
    check(crate::map::keys);
    check(crate::map::spawn_hud);
    check(crate::map::spawn_map);
    check(crate::chat::draw_feed);
    check(crate::chat::run);
    check(crate::chat::fill);
    check(crate::chat::acts);
    check(crate::keyboard::draw);
    check(crate::keyboard::input);
    check(crate::waypoints::travel);
    check(crate::waypoints::spawn_fade);
    check(crate::ui::draw_menu);
    check(crate::ui::menu_input);
    check(crate::photos::take);
    check(crate::render::menu_blur);
    check(crate::ui::update_hud);
}

#[test]
fn base_creations_pass_their_own_tests() {
    home();
    let lib = Library::load();
    let w = world();
    let mut ran = 0;
    for c in lib.list.values().filter(|c| c.origin == Origin::Base && c.files.contains_key(crate::testing::FILE)) {
        let report = crate::testing::run(c, &w);
        println!("{}", report.text());
        assert!(report.passed(), "{}", report.text());
        ran += 1;
    }
    assert!(ran >= 2, "the balloon and the woolly have tests");
}

#[test]
fn a_creations_tests_say_what_passed_what_failed_and_why() {
    let mut c = creation(BOX);
    c.files.insert(
        "test.luau".into(),
        r##"
local test = require("@potluck/test@0.1")
local world = require("@potluck/world")
test.case("the thing is there", function()
    test.same(#world.creatures(test.kind("Thing")), 1, "things")
end)
test.case("time passes", function()
    local p = test.player("Ada")
    local here = p.position
    test.place(p, here + vector.create(2, 0, 0), "Stone")
    test.wait(2)
    test.same(world.block(here + vector.create(2, 0, 0)), "Stone", "the block put down")
end)
test.case("a world of its own", function()
    local p = test.player("Bo")
    test.check(world.block(p.position + vector.create(2, 0, 0)) ~= "Stone", "the last case's block is gone")
end)
test.case("a wrong sum", function()
    test.same(1 + 1, 3, "the sum")
end)
test.case("a mistake in the test", function()
    test.ride(test.player(), 12345)
end)
test.case("waits forever", function()
    while true do test.wait(60) end
end)
"##
        .into(),
    );
    let report = crate::testing::run(&c, &world());
    let text = report.text();
    println!("{text}");
    assert!(!report.passed());
    let by = |n: &str| report.cases.iter().find(|c| c.name == n).unwrap_or_else(|| panic!("{n} ran"));
    assert!(by("the thing is there").passed && by("time passes").passed && by("a world of its own").passed, "{text}");
    assert!(by("time passes").played >= 2.0);
    let wrong = by("a wrong sum");
    assert!(!wrong.passed && wrong.problem.as_deref().unwrap().contains("the sum: got 2, wanted 3"), "{text}");
    assert!(wrong.problem.as_deref().unwrap().contains("test.luau:"), "it says the line: {text}");
    assert!(by("a mistake in the test").problem.as_deref().unwrap().contains("test.ride needs one of the creation's creatures"), "{text}");
    assert!(by("waits forever").problem.as_deref().unwrap().contains("s of play"), "{text}");
    assert!(text.contains("3 of 6 cases passed"), "{text}");
    // outside test runs the module only says where it works
    let mut herd = Herd::default();
    let mut scripts = Scripts::default();
    let mut c = creation(&format!("{BOX}\nlocal test = require(\"@potluck/test\")\ntest.case(\"x\", function() end)\nlocal ok, e = pcall(test.wait, 1)\nprint(e)\n"));
    c.files.remove("test.luau");
    scripts.start(&c, &mut herd).unwrap();
    assert!(scripts.vms["test"].log.iter().any(|l| l.contains("works only in a case")), "{:?}", scripts.vms["test"].log);
    let none = crate::testing::run(&creation(BOX), &world());
    assert!(!none.passed() && none.text().contains("has no test.luau"));
}

/// world 0.10: behaviours, with no code: a creature follows food in a
/// player's hand and whoever used it, runs from a hit, chatters with a
/// sound, and its own handlers still have the last word; mistakes in them
/// say what's wrong.
#[test]
fn behaviours_make_a_creature_with_no_code() {
    use crate::blocks::{by_name, GLASS, STONE};
    let mut voxels = world();
    let (mid, _) = pad(&mut voxels);
    let code = |extra: &str| {
        format!(
            r##"local world = require("@potluck/world@0.10")
local Hen = world.creature {{
	name = "Hen", count = 1, home = {{ {}, {}, {} }}, speed = 2,
	behaviours = {{ follow_food = {{ "Wheat", "Carrots" }}, follow_when_used = 5, flee_when_hit = true, hop_when_hit = true, chatter = {{ "Cluck." }}, sound = "chirp" }},
	body = {{ {{ "body", size = {{ 0.5, 0.5, 0.6 }}, at = {{ 0, 0.3, 0 }}, color = "#f4f1e8" }} }},
}}
{extra}
"##,
            mid.x, mid.y, mid.z
        )
    };
    let hen = |scripts: &Scripts, herd: &Herd| herd.bodies.values().find(|b| b.creation == "test").cloned().unwrap_or_else(|| panic!("{:?}", scripts.vms["test"].log));
    let flat = |a: Vec3, b: Vec3| Vec2::new(a.x - b.x, a.z - b.z).length();
    let mut herd = Herd::default();
    let mut scripts = Scripts::default();
    scripts.start(&creation(&code("")), &mut herd).unwrap();
    run(&mut scripts, &mut voxels, &mut herd, 0.2);
    let start = hen(&scripts, &herd).pos;
    // someone with wheat in hand, eight blocks off: it comes, and stops short
    let ada = PlayerInfo { id: 7, name: "Ada".into(), pos: start + Vec3::new(8.0, 0.0, 0.0) };
    scripts.players = vec![ada.clone()];
    scripts.set_held(7, by_name("Wheat").unwrap().into());
    run(&mut scripts, &mut voxels, &mut herd, 6.0);
    let d = flat(hen(&scripts, &herd).pos, ada.pos);
    assert!((1.0..3.2).contains(&d), "came for the wheat: {d}");
    // stone in hand, and it loses interest
    scripts.set_held(7, by_name("Stone").unwrap().into());
    run(&mut scripts, &mut voxels, &mut herd, 0.3);
    assert!(hen(&scripts, &herd).goal.is_none_or(|g| flat(g, ada.pos) > 0.5), "stops going after them");
    // used: it follows for its five seconds, then doesn't
    let id = hen(&scripts, &herd).id;
    scripts.event(&mut voxels, &mut herd, id, "used", &ada);
    assert_eq!(hen(&scripts, &herd).says.map(|s| s.0), Some("Cluck.".to_string()), "says one of its lines");
    assert!(scripts.sound_changes().iter().any(|(_, p)| p.name == "chirp"), "with its sound");
    scripts.players[0].pos = hen(&scripts, &herd).pos + Vec3::new(-5.0, 0.0, 2.0);
    run(&mut scripts, &mut voxels, &mut herd, 4.0);
    assert!(flat(hen(&scripts, &herd).pos, scripts.players[0].pos) < 3.2, "followed whoever used it");
    // hit: it hops and runs, faster than it walks, and follows no more
    let ada = scripts.players[0].clone();
    run(&mut scripts, &mut voxels, &mut herd, 0.2);
    let before = flat(hen(&scripts, &herd).pos, ada.pos);
    scripts.event(&mut voxels, &mut herd, id, "hit", &ada);
    assert!(hen(&scripts, &herd).vel.y > 3.0 && hen(&scripts, &herd).speed > 3.0, "hops and hurries");
    run(&mut scripts, &mut voxels, &mut herd, 3.0);
    assert!(flat(hen(&scripts, &herd).pos, ada.pos) > before + 4.0, "ran off");
    run(&mut scripts, &mut voxels, &mut herd, 3.0);
    assert_eq!(hen(&scripts, &herd).speed, 2.0, "back to its own pace");
    // its own handler has the last word
    let mut herd = Herd::default();
    let mut scripts = Scripts::default();
    scripts.start(&creation(&code("function Hen:hit(player) self:stop() self:say(\"Hmph.\") end")), &mut herd).unwrap();
    run(&mut scripts, &mut voxels, &mut herd, 0.2);
    let id = hen(&scripts, &herd).id;
    scripts.event(&mut voxels, &mut herd, id, "hit", &ada);
    let h = hen(&scripts, &herd);
    assert!(h.goal.is_none() && h.says.map(|s| s.0) == Some("Hmph.".into()), "the code's say and stop win");
    // mistakes say what's wrong
    for (bad, says) in [("flee = true", "there's no behaviour called \"flee\""), ("follow_food = \"Cake\"", "nothing called \"Cake\""), ("sound = \"moo\"", "no sound called \"moo\""), ("flee_when_hit = \"far\"", "flee_when_hit takes true, or a number")] {
        let src = format!("local world = require(\"@potluck/world\")\nworld.creature {{ name = \"Hen\", behaviours = {{ {bad} }} }}\n");
        let err = Scripts::default().start(&creation(&src), &mut Herd::default()).err().unwrap_or_default();
        assert!(err.contains(says), "{bad}: {err}");
    }
}

/// A creation's own blocks: `[blocks.marble]`, its picture in letters.
const MADE_TOML: &str = r##"
name = "Masonry"
by = "Tests"
kind = "mod"
main = "main.luau"

[blocks.marble]
kind = "hard"
hardness = 2
colors = { w = "#f0ece4", g = "#c8c2b8" }
picture = """
wwwwwwww
wgwwwwgw
wwwwwwww
wwwgwwww
wwwwwwww
wgwwwwww
wwwwwgww
wwwwwwww
"""

[blocks.paper_lantern]
name = "Test lantern"
light = true
glow = "y"
boxes = [[4, 0, 4, 12, 10, 12]]
colors = { r = "#b83a2c", y = "#ffd27a" }
side = ["rrrrrrrr", "ryyyyyyr", "ryyyyyyr", "ryyyyyyr", "ryyyyyyr", "ryyyyyyr", "ryyyyyyr", "rrrrrrrr"]
top = ["rrrrrrrr", "r......r", "r......r", "r......r", "r......r", "r......r", "r......r", "rrrrrrrr"]
bottom = ["rrrrrrrr", "rrrrrrrr", "rrrrrrrr", "rrrrrrrr", "rrrrrrrr", "rrrrrrrr", "rrrrrrrr", "rrrrrrrr"]
"##;

#[test]
fn creations_make_blocks_of_their_own() {
    use crate::{blocks, made};
    let spec = |t: &str| -> serde_json::Value { serde_json::to_value(toml::from_str::<toml::Value>(t).unwrap()).unwrap() };
    let row = "aaaaaaaa";
    let eight = format!("picture = [{}]", vec![format!("\"{row}\""); 8].join(", "));
    // what's wrong is said plainly
    for (bad, says) in [
        (format!("colors = {{ a = \"#fff\" }}\n{}", eight.replace("aaaaaaaa\"]", "aaaaaaa\"]")), "row 8 has 7 pixels"),
        (format!("colors = {{ a = \"#fff\" }}\npicture = [\"aaa\"]"), "8, 16, 32 or 64"),
        (format!("colors = {{ b = \"#fff\" }}\n{eight}"), "colors doesn't give"),
        (format!("colors = {{ a = \"white\" }}\n{eight}"), "a colour like"),
        (format!("colors = {{ a = \"#fff\" }}\n{}", eight.replacen("aaaaaaaa", "aaaa.aaa", 1)), "see-through"),
        (format!("name = \"Stone\"\ncolors = {{ a = \"#fff\" }}\n{eight}"), "has a block called Stone"),
        (format!("shape = \"round\"\ncolors = {{ a = \"#fff\" }}\n{eight}"), "shape is"),
        (format!("boxes = [[0, 0, 0, 16, 17, 16]]\ncolors = {{ a = \"#fff\" }}\n{eight}"), "box 1"),
        (format!("hardness = -1\ncolors = {{ a = \"#fff\" }}\n{eight}"), "hardness"),
        (format!("kind = \"metal\"\ncolors = {{ a = \"#fff\" }}\n{eight}"), "kind is one of"),
        (format!("colour = \"#fff\"\n{eight}"), "there's no colour for a block"),
        ("colors = { a = \"#fff\" }".to_string(), "needs a picture"),
    ] {
        let e = made::read("thing", &spec(&bad)).err().unwrap_or_else(|| panic!("{bad} should be refused"));
        assert!(e.contains(says), "{bad}: {e}");
    }
    assert!(made::read("Thing", &spec(&format!("colors = {{ a = \"#fff\" }}\n{eight}"))).is_err(), "names in potluck.toml are lowercase");
    let def = made::read("tall_marble", &spec(&format!("colors = {{ a = \"#fff\" }}\n{eight}"))).unwrap();
    assert_eq!(def.name, "Tall marble");
    assert_eq!((def.shape, def.hardness, def.same), (blocks::Shape::Solid, 1.0, [0, 0, 0]), "one picture for every side");
    assert_eq!(def.faces[0].rgba[0], [255, 255, 255, 255]);

    // a creation's blocks get ids from 128 up, and keep them
    let mut c = creation("local world = require(\"@potluck/world@0.12\")\nprint(world.kind(\"Marble\"))\nworld.after(0, function()\n  world.set_block({ 3, 90, 3 }, \"Marble\")\n  print(world.block({ 3, 90, 3 }))\nend)");
    c.id = "test-masonry".into();
    c.manifest = toml::from_str(MADE_TOML).unwrap();
    let ids = made::register(&c.id, &c.manifest.blocks).unwrap();
    assert!(ids.iter().all(|&id| id >= blocks::FIRST_MADE), "{ids:?}");
    assert_eq!(made::register(&c.id, &c.manifest.blocks).unwrap(), ids, "the same ids the second time");
    let (marble, lantern) = (ids[0], ids[1]);
    assert_eq!(blocks::by_name("marble"), Some(marble));
    assert_eq!(blocks::by_name("Test Lantern"), Some(lantern));
    let b = blocks::block(lantern);
    assert_eq!((b.shape, b.solid, b.light, b.model.len()), (blocks::Shape::Model, true, true, 1));
    assert_eq!(b.faces, ["test-masonry/paper_lantern/top", "test-masonry/paper_lantern/side", "test-masonry/paper_lantern/bottom"]);
    assert_eq!(blocks::block(marble).faces, ["test-masonry/marble/top"; 3], "one tile for one picture");
    assert!(blocks::is_solid(marble) && blocks::is_opaque(marble) && !blocks::is_opaque(lantern));
    assert_eq!(crate::sound::material(marble), crate::sound::Material::Hard, "kind says how it sounds");
    let glow = made::read("paper_lantern", &c.manifest.blocks["paper_lantern"]).unwrap();
    assert!(glow.faces[1].glow[9] && !glow.faces[1].glow[0], "the letters in glow glow");
    assert_eq!(glow.faces[0].rgba[9][3], 0, "and . is see-through");
    assert_eq!(made::color(marble).map(|c| c[0] > 200), Some(true), "the map's colour from the top");
    assert!(made::register("too-many", &(0..33).map(|i| (format!("b{i}"), spec(&format!("colors = {{ a = \"#fff\" }}\n{eight}")))).collect()).is_err());

    // its code uses them by name, as soon as it starts
    let mut voxels = world();
    let mut herd = Herd::default();
    let mut scripts = Scripts::default();
    scripts.start(&c, &mut herd).unwrap();
    run(&mut scripts, &mut voxels, &mut herd, 0.1);
    let vm = &scripts.vms["test-masonry"];
    assert_eq!(vm.error, None);
    assert!(vm.log.iter().any(|l| l == "hard") && vm.log.iter().any(|l| l == "Marble"), "{:?}", vm.log);
    assert_eq!(voxels.at(IVec3::new(3, 90, 3)), marble);

    // a changed picture keeps the id
    let mut changed = c.manifest.blocks.clone();
    changed.get_mut("marble").unwrap()["colors"]["w"] = "#ffffff".into();
    let before = made::generation();
    assert_eq!(made::register(&c.id, &changed).unwrap(), ids);
    assert!(made::generation() > before, "and the game draws it again");

    // the world saves its made blocks with it, and takes them back
    voxels.set(IVec3::new(4, 90, 3), lantern);
    let saved = made::world_bytes(&voxels);
    assert!(saved.starts_with(b"POTLUCKW3"));
    let back = made::read_world(&saved).unwrap();
    assert_eq!((back.at(IVec3::new(3, 90, 3)), back.at(IVec3::new(4, 90, 3))), (marble, lantern));
    assert!(back.lanterns.contains(&IVec3::new(4, 90, 3)), "a made light is a light");
    let mut old = b"POTLUCKW1".to_vec();
    old.extend_from_slice(&voxels.seed.to_le_bytes());
    old.extend_from_slice(&world().encode_island());
    assert!(made::read_world(&old).is_some(), "worlds saved before still load");
    // and those saved before the world went on past the island, with the
    // made blocks in them
    let mut island = b"POTLUCKW2".to_vec();
    island.extend_from_slice(&saved[9..13]);
    let n = u32::from_le_bytes(saved[13..17].try_into().unwrap()) as usize;
    island.extend_from_slice(&saved[13..17 + n]);
    island.extend_from_slice(&voxels.encode_island());
    let back = made::read_world(&island).expect("an island world");
    assert_eq!((back.at(IVec3::new(3, 90, 3)), back.at(IVec3::new(4, 90, 3))), (marble, lantern));
    assert!(made::read_world(&saved[..40]).is_none(), "a damaged world is refused");

    // a saved block whose id something else has here moves
    let mut moved = made::table().into_iter().find(|e| e.id == marble).unwrap();
    moved.creation = "test-elsewhere".into();
    let to = made::restore(&[moved.clone()]).expect("a new id");
    let new = to[marble as usize];
    assert!(new != marble && new >= blocks::FIRST_MADE);
    assert_eq!(made::table().iter().find(|e| e.creation == "test-elsewhere").map(|e| e.id), Some(new));
    let mut v = world();
    v.set(IVec3::new(5, 90, 5), marble);
    v.renumber(&to);
    assert_eq!(v.at(IVec3::new(5, 90, 5)), new);

    // a guest takes the host's ids as they are
    let mut guest = made::table().into_iter().find(|e| e.creation == "test-elsewhere").unwrap();
    guest.id = 250;
    made::adopt(&[guest]);
    assert!(blocks::made(new).is_none(), "the old place is free again");
    assert_eq!(blocks::block(250).name, "Marble");
    assert_eq!(blocks::block(251).name, "Unknown block", "an id nobody gave a block shows that it's missing");
}

#[test]
fn made_blocks_are_drawn_into_the_atlas() {
    let mut images = Assets::<Image>::default();
    let mut atlas = crate::textures::build(&mut images);
    let red = vec![[200u8, 40, 40, 255]; 16 * 16];
    let slot = atlas.put(&mut images, "test/red/top", &red, 16, &vec![false; 256]).expect("room in the atlas");
    assert_eq!(atlas.tile("test/red/top"), slot);
    assert!(atlas.icons.contains_key("test/red/top"), "an icon for the hotbar");
    // every mip level of all four maps, each a whole cell inside its image
    let full = images.get(&atlas.icons["test/red/top"]).unwrap().width() as usize;
    let levels = full.trailing_zeros() as usize - 2;
    assert_eq!(atlas.writes.len(), 4 * levels);
    for w in &atlas.writes {
        let tile = full >> w.level;
        assert_eq!(w.size as usize, tile + 2 * (tile / 8));
        assert_eq!(w.data.len(), (w.size * w.size * 4) as usize);
        assert!((w.x + w.size) as usize <= w.size as usize * crate::textures::GRID && (w.y + w.size) as usize <= w.size as usize * crate::textures::GRID);
    }
    let colour = atlas.writes.iter().find(|w| w.image == atlas.color.id() && w.level == 0).unwrap();
    assert_eq!(&colour.data[..4], &[200, 40, 40, 255]);
    // drawing it again reuses its tile; glowing ones go at the far end
    atlas.writes.clear();
    assert_eq!(atlas.put(&mut images, "test/red/top", &red, 16, &vec![false; 256]), Some(slot));
    let lit = atlas.put(&mut images, "test/lit/top", &red, 16, &vec![true; 256]).unwrap();
    assert!(lit > slot + crate::textures::GRID as u16, "{lit} {slot}");
    let glow = atlas.writes.iter().rev().find(|w| w.image == atlas.glow.id() && w.level == 0).unwrap();
    assert_eq!(&glow.data[..3], &[200, 40, 40]);
    // and it fills up, refusing what doesn't fit
    let mut n = 0;
    while atlas.put(&mut images, Box::leak(format!("test/fill{n}").into_boxed_str()), &red, 16, &vec![false; 256]).is_some() {
        n += 1;
        assert!(n < crate::textures::GRID * crate::textures::GRID);
    }
    assert!(n > 300, "room for plenty of made blocks: {n}");
}

/// world 0.15: a made block that turns is four blocks, one for each way it
/// can face, carried and named as the one facing north; boxes can reach
/// toward their creation's other blocks.
#[test]
fn made_blocks_turn_and_join() {
    use crate::blocks::{self, When};
    use crate::made;
    let spec = |t: &str| -> serde_json::Value { serde_json::to_value(toml::from_str::<toml::Value>(t).unwrap()).unwrap() };
    // a slab with a knob at the front (north), and an arrow on top
    let mut rows = vec!["aaaaaaaa".to_string(); 8];
    rows[0] = "aaaabaaa".into();
    let top = format!("[{}]", rows.iter().map(|r| format!("\"{r}\"")).collect::<Vec<_>>().join(", "));
    let side = format!("[{}]", vec!["\"cccccccc\""; 8].join(", "));
    let gadget = format!("turns = true\nboxes = [[0, 0, 0, 16, 2, 16], [7, 2, 2, 9, 7, 4]]\ncolors = {{ a = \"#888\", b = \"#f00\", c = \"#444\" }}\ntop = {top}\nside = {side}\nbottom = {side}\n");
    let plug = format!("boxes = [[6, 0, 6, 10, 1, 10], {{ box = [10, 0, 7, 16, 1, 9], joins = \"east\" }}]\ncolors = {{ c = \"#444\" }}\npicture = {side}\n");
    // what's wrong is said plainly
    for (bad, says) in [
        (gadget.replace("turns = true", "turns = 1"), "turns is true or false"),
        (plug.replace("\"east\"", "\"sideways\""), "joins is one of"),
        (plug.replace("joins =", "reaches ="), "has no reaches"),
        (plug.replace("box = [10", "box = [16"), "box 2"),
    ] {
        let e = made::read("gadget", &spec(&bad)).err().unwrap_or_else(|| panic!("{bad} should be refused"));
        assert!(e.contains(says), "{bad}: {e}");
    }
    assert!(made::read("plug>1", &spec(&plug)).is_err(), "a block that doesn't turn has no other ways");
    let def = made::read("gadget", &spec(&gadget)).unwrap();
    assert!(def.turns && def.quarter == 0);
    let east = made::read("gadget>1", &spec(&gadget)).unwrap();
    assert_eq!(east.quarter, 1);
    assert_eq!((east.boxes[1].0, east.boxes[1].1), ([12, 2, 7], [14, 7, 9]), "the knob goes round to the east");
    assert_eq!((east.boxes[0].0, east.boxes[0].1), ([0, 0, 0], [16, 2, 16]), "the slab stays");
    assert_eq!(east.faces[0].rgba[4 * 8 + 7], [255, 0, 0, 255], "the arrow's tip at the east edge");
    assert_eq!(def.faces[0].rgba[4], [255, 0, 0, 255], "facing north it's on the top row");
    assert_eq!(east.faces[1], def.faces[1], "the sides don't change");
    let west = made::read("gadget>3", &spec(&gadget)).unwrap();
    assert_eq!((west.boxes[1].0, west.boxes[1].1), ([2, 2, 7], [4, 7, 9]));

    // a world gives it four ids, and names and carries the one facing north
    let mut blocks_of = BTreeMap::new();
    blocks_of.insert("gadget".to_string(), spec(&gadget));
    blocks_of.insert("plug".to_string(), spec(&plug));
    let ids = made::register("turn-test", &blocks_of).unwrap();
    assert_eq!(ids.len(), 2, "one id for each block it makes");
    let (north, plug_id) = (ids[0], ids[1]);
    assert_eq!(made::table().iter().filter(|e| e.creation == "turn-test").count(), 5, "and three more for the ways a gadget faces");
    assert_eq!(blocks::by_name("gadget"), Some(north));
    let turned: Vec<_> = (0..4).map(|q| blocks::turned(north, q)).collect();
    assert_eq!(turned[0], north);
    assert_eq!(turned.iter().collect::<std::collections::BTreeSet<_>>().len(), 4, "{turned:?}");
    for (q, &id) in turned.iter().enumerate() {
        assert_eq!((blocks::base(id), blocks::facing(id)), (north, Some(q as u8)));
        assert_eq!(blocks::block(id).name, "Gadget");
        assert_eq!(blocks::turned(id, 2), turned[2], "from any way round");
    }
    assert_eq!((blocks::facing(plug_id), blocks::turned(plug_id, 1), blocks::base(plug_id)), (None, plug_id, plug_id), "a plug doesn't turn");
    let (b0, b1) = (blocks::block(north), blocks::block(turned[1]));
    assert_ne!(b0.faces[0], b1.faces[0], "a top of its own");
    assert_eq!(b0.faces[1..], b1.faces[1..], "the same sides and bottom");
    assert_eq!((b1.model[1].from, b1.model[1].to), ([12, 2, 7], [14, 7, 9]));
    let rules = crate::items::Rules::default();
    assert_eq!(rules.gives(turned[3], 0.5), Some((crate::items::of_block(north), 1)), "it breaks into the one facing north");

    // a plug's arm reaches only toward its creation's own blocks
    let arm = blocks::block(plug_id).model[1].when;
    assert!(arm == When::Kin(0));
    assert!(arm.holds(plug_id, |d| if d == 0 { turned[2] } else { blocks::AIR }), "a gadget to the east");
    assert!(arm.holds(plug_id, |d| if d == 0 { plug_id } else { blocks::AIR }), "another plug");
    assert!(!arm.holds(plug_id, |d| if d == 0 { blocks::STONE } else { blocks::AIR }), "not stone");
    assert!(!arm.holds(plug_id, |_| blocks::AIR));
    assert_eq!(When::Kin(5).turned(1), When::Kin(0), "north goes to east");
    assert_eq!(When::Same(2).turned(3), When::Same(2), "up stays up");

    // a player looking one way puts it down facing that way
    assert_eq!([(0.0, -1.0), (1.0, 0.1), (0.2, 1.0), (-1.0, 0.0)].map(|(x, z)| blocks::quarter_looking(x, z)), [0, 1, 2, 3]);
    assert_eq!((0..4).map(|q| blocks::turn_side(5, q)).collect::<Vec<_>>(), [5, 0, 4, 1], "north, east, south, west");

    // a guest's world takes the host's ids, turned ones and all
    let entries: Vec<_> = made::table().into_iter().filter(|e| e.creation == "turn-test").collect();
    made::adopt(&entries);
    assert_eq!(blocks::turned(north, 1), turned[1]);
    assert_eq!(blocks::base(turned[1]), north);
}

/// world 0.14: a made block's frames, read and checked like its picture,
/// and drawn once each so the game only sends the frame it shows.
#[test]
fn made_blocks_can_move() {
    use crate::made;
    let spec = |t: &str| -> serde_json::Value { serde_json::to_value(toml::from_str::<toml::Value>(t).unwrap()).unwrap() };
    let pic = |c: char| format!("[{}]", vec![format!("\"{}\"", c.to_string().repeat(8)); 8].join(", "));
    let base = format!("colors = {{ a = \"#c83c28\", b = \"#ffd27a\" }}\nglow = \"b\"\nlight = true\npicture = {}\n", pic('a'));
    // what's wrong is said plainly
    for (bad, says) in [
        (format!("{base}speed = 2\n"), "speed is for a block with frames"),
        (format!("{base}frames = []\n"), "frames is a list of 1 to 7"),
        (format!("{base}frames = \"fast\"\n"), "frames is a list"),
        (format!("{base}[[frames]]\ncolour = \"#fff\"\n"), "there's no colour for a frame"),
        (format!("{base}[[frames]]\npicture = [\"ccc\"]\n"), "frames[1].picture"),
        (format!("{base}speed = 9\n[[frames]]\npicture = {}\n", pic('b')), "speed is how many pictures a second"),
        (format!("{base}{}", (0..8).map(|_| format!("[[frames]]\npicture = {}\n", pic('b'))).collect::<String>()), "1 to 7"),
    ] {
        let e = made::read("flicker", &spec(&bad)).err().unwrap_or_else(|| panic!("{bad} should be refused"));
        assert!(e.contains(says), "{bad}: {e}");
    }
    // a frame that gives only a side keeps the block's own top and bottom
    let def = made::read("flicker", &spec(&format!("{base}speed = 2\n[[frames]]\nside = {}\n[[frames]]\npicture = {}\n", pic('b'), pic('a')))).unwrap();
    assert_eq!((def.frames.len(), def.speed), (2, 2.0));
    assert!(def.moves(1) && !def.moves(0) && !def.moves(2), "only the side changes");
    assert_eq!(def.same, [0, 1, 0], "the side has a tile of its own, the top and bottom share one");
    assert!(def.frames[0][1].glow[0] && !def.faces[1].glow[0], "frames glow by the block's letters");
    let still = made::read("flicker", &spec(&base)).unwrap();
    assert!(still.frames.is_empty() && !(0..3).any(|f| still.moves(f)));

    // frames go round at the block's speed, and stay on the first with Reduce flashing
    assert_eq!((0..8).map(|i| made::frame_at(i as f64 * 0.25, 4.0, 3, false)).collect::<Vec<_>>(), [0, 1, 2, 0, 1, 2, 0, 1]);
    assert_eq!(made::frame_at(1.6, 0.5, 4, false), 0);
    assert_eq!(made::frame_at(2.0, 0.5, 4, false), 1);
    assert!((0..40).all(|i| made::frame_at(i as f64 * 0.1, 8.0, 4, true) == 0), "still with Reduce flashing on");
    assert_eq!(made::frame_at(5.0, 4.0, 1, false), 0);

    // each frame's writes are worked out once, all into the same tile
    let mut images = Assets::<Image>::default();
    let mut atlas = crate::textures::build(&mut images);
    let (red, gold) = (vec![[200u8, 60, 40, 255]; 64], vec![[255u8, 210, 120, 255]; 64]);
    let (dark, lit) = (vec![false; 64], vec![true; 64]);
    let frames = atlas.put_moving(&mut images, "test/flicker/side", &[(&red[..], &dark[..], 8), (&gold[..], &lit[..], 8)]).expect("room in the atlas");
    let slot = atlas.tile("test/flicker/side");
    let plain = atlas.put(&mut images, "test/plain/top", &red, 8, &dark).unwrap();
    assert!(slot > plain + crate::textures::GRID as u16, "kept with the glowing tiles, since a frame glows: {slot} {plain}");
    assert_eq!(frames.len(), 2);
    assert_eq!(frames[0].len(), frames[1].len(), "every frame writes the same cells");
    let first = |f: &Vec<crate::textures::TileWrite>, image| f.iter().find(|w| w.image == image && w.level == 0).unwrap().clone();
    assert_eq!(&first(&frames[0], atlas.color.id()).data[..3], &[200, 60, 40]);
    assert_eq!(&first(&frames[1], atlas.color.id()).data[..3], &[255, 210, 120]);
    assert_eq!(&first(&frames[0], atlas.glow.id()).data[..3], &[0, 0, 0], "the first frame doesn't glow");
    assert_eq!(&first(&frames[1], atlas.glow.id()).data[..3], &[255, 210, 120], "the second does");
    let (a, b) = (first(&frames[0], atlas.color.id()), first(&frames[1], atlas.color.id()));
    assert_eq!((a.x, a.y, a.size), (b.x, b.y, b.size));
    assert_eq!(atlas.put_moving(&mut images, "test/flicker/side", &[(&gold[..], &lit[..], 8), (&red[..], &dark[..], 8)]).map(|_| atlas.tile("test/flicker/side")), Some(slot), "drawing it again reuses its tile");
}

#[test]
fn creative_has_every_block_to_choose_from() {
    use crate::blocks::{self, BlockId};
    use crate::input::Controls;
    use crate::settings::Settings;
    use crate::ui::{block_shelf, menu_items, pages, Act, Acts, Focus, Kind, Screen, WorldInfo, BLOCK_PAGES};
    use bevy::ecs::system::RunSystemOnce;
    // every block of the game's own on one page, and never air, bedrock or a tool
    let mut seen: Vec<BlockId> = BLOCK_PAGES[..3].iter().flat_map(|p| block_shelf(p)).collect();
    let n = seen.len();
    seen.sort();
    seen.dedup();
    assert_eq!(seen.len(), n, "each on one page");
    for id in 1..blocks::BLOCKS.len() as BlockId {
        let b = blocks::block(id);
        let shown = seen.contains(&id);
        assert_eq!(shown, b.shape != blocks::Shape::Air && id != blocks::BEDROCK, "{}", b.name);
    }
    for (page, id) in [("Building", blocks::GLASS), ("Building", blocks::PLANKS), ("Nature", blocks::GRASS), ("Nature", blocks::FLOWER_RED), ("Nature", blocks::WATER), ("Things", blocks::LANTERN), ("Things", blocks::TABLE)] {
        assert!(block_shelf(page).contains(&id), "{} on {page}", blocks::block(id).name);
    }
    // a creation's blocks have a page of their own
    let tile = serde_json::json!({ "name": "Picker test tile", "colors": { "a": "#c86432" }, "picture": vec!["aaaaaaaa"; 8] });
    let made = crate::made::register("picker-test", &BTreeMap::from([("tile".to_string(), tile)])).unwrap();
    assert!(block_shelf("From creations").contains(&made[0]));

    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    crate::sim(&mut app, crate::SEED);
    app.insert_resource(Settings::default()).insert_resource(crate::player::Me::default()).insert_resource(crate::KnobEdits::default());
    app.world_mut().resource_mut::<WorldInfo>().in_world = true;
    let act = |app: &mut App, a: Act| {
        app.world_mut().resource_mut::<Acts>().0.push(a);
        app.world_mut().run_system_once(crate::game::do_acts).unwrap();
    };
    let press = |app: &mut App, f: &dyn Fn(&mut Controls)| {
        let mut c = Controls::default();
        f(&mut c);
        app.insert_resource(c);
        app.world_mut().run_system_once(crate::ui::menu_input).unwrap();
    };
    // it opens on the block in your hand, on its page
    app.world_mut().resource_mut::<crate::player::Me>().slot = 4;
    let hand = |app: &App, i: usize| crate::items::block(app.world().resource::<crate::player::Me>().pockets.0[i].thing).unwrap();
    let held = hand(&app, 4);
    act(&mut app, Act::OpenBlocks);
    let f = app.world().resource::<Focus>();
    assert_eq!(f.screen, Screen::Blocks);
    let info = app.world().resource::<WorldInfo>();
    let (page, _) = pages(f, info).map(|(list, n)| (list[n], n)).unwrap();
    assert!(block_shelf(page).contains(&held), "{page}");
    let (title, items) = menu_items(f, app.world().resource::<Library>(), app.world().resource::<Scripts>(), app.world().resource::<Settings>(), info);
    assert_eq!(title, "Blocks");
    assert_eq!(items[f.sel].kind, Kind::Tile(held));
    // along the row and A: the next one goes in your hand, in the slot chosen
    app.world_mut().resource_mut::<Focus>().cols = 6;
    press(&mut app, &|c| c.right = true);
    let sel = app.world().resource::<Focus>().sel;
    let Kind::Tile(next) = items[sel].kind else { panic!("a block") };
    press(&mut app, &|c| c.accept = true);
    assert_eq!(app.world().resource::<Acts>().0, [Act::Hold(next)]);
    app.world_mut().run_system_once(crate::game::do_acts).unwrap();
    assert_eq!((hand(&app, 4), app.world().resource::<crate::player::Me>().slot), (next, 4));
    assert_eq!(app.world().resource::<Focus>().screen, Screen::Playing, "and back to the world");
    // one already in the hotbar: that slot is chosen instead
    let first = hand(&app, 0);
    act(&mut app, Act::Hold(first));
    assert_eq!((app.world().resource::<crate::player::Me>().slot, hand(&app, 4)), (0, next));
    // air isn't put in a hand
    act(&mut app, Act::Hold(blocks::AIR));
    assert_eq!(hand(&app, 0), first);
    // the middle button puts the block aimed at in your hand
    app.world_mut().resource_mut::<crate::player::Me>().target = Some(crate::player::Hit { block: IVec3::new(3, 40, 3), normal: IVec3::Y, id: blocks::SAND });
    let mut c = Controls::default();
    c.pick = true;
    app.insert_resource(c);
    app.world_mut().run_system_once(crate::game::interact).unwrap();
    app.world_mut().run_system_once(crate::game::do_acts).unwrap();
    let slot = app.world().resource::<crate::player::Me>().slot;
    assert_eq!(hand(&app, slot), blocks::SAND);
    // B or the key that opened it close it
    act(&mut app, Act::OpenBlocks);
    press(&mut app, &|c| c.back = true);
    press(&mut app, &|c| c.blocks = true);
    assert_eq!(app.world().resource::<Acts>().0, [Act::Resume, Act::Resume]);
    app.world_mut().resource_mut::<Acts>().0.clear();
    // in survival the same key opens the backpack instead
    app.world_mut().resource_mut::<Focus>().go(Screen::Playing);
    app.world_mut().resource_mut::<Settings>().survival = true;
    act(&mut app, Act::OpenBlocks);
    assert_eq!(app.world().resource::<Focus>().screen, Screen::Backpack);
    act(&mut app, Act::Hold(blocks::STONE));
    assert!(!app.world().resource::<crate::player::Me>().pockets.0.iter().any(|s| s.thing == blocks::STONE as u16), "nothing new in survival");
}

/// Survival pockets are the hotbar and a backpack of 27 more: blocks stack
/// to 99 and tools one to a slot, what's given fills the hotbar first, what's
/// taken comes from the backpack first, and the backpack's screen moves
/// things between slots.
#[test]
fn a_backpack_holds_27_more_slots_and_things_move_between_them() {
    use crate::blocks::{self, COBBLE, PLANKS, SAND, STONE};
    use crate::items::{self, Slot, Slots, HOTBAR, POCKETS, STONE_AXE, WOODEN_PICKAXE};
    use crate::input::Controls;
    use crate::settings::Settings;
    use crate::ui::{menu_items, Act, Acts, Focus, Kind, Screen, WorldInfo};
    use bevy::ecs::system::RunSystemOnce;
    let t = items::of_block;
    // things by name: blocks first, then items; air and water aren't carried
    assert_eq!((items::by_name("stone axe"), items::by_name("Planks"), items::by_name("Air")), (Some(STONE_AXE), Some(t(PLANKS)), None));
    assert!(!items::carriable(t(blocks::WATER)) && !items::carriable(t(blocks::BEDROCK)) && items::carriable(WOODEN_PICKAXE));
    assert_eq!((items::name(WOODEN_PICKAXE), items::icon(WOODEN_PICKAXE), items::icon(t(PLANKS))), ("Wooden pickaxe", "pickaxe_wood", "planks"));
    assert!(items::ITEMS.iter().all(|i| blocks::by_name(i.name).is_none()), "no item shares a block's name");

    let mut p = Slots::default();
    assert_eq!(p.give(t(STONE), 250), 250);
    assert_eq!(&p.0[..3], &[Slot { thing: t(STONE), count: 99 }, Slot { thing: t(STONE), count: 99 }, Slot { thing: t(STONE), count: 52 }], "99 to a slot, the hotbar first");
    assert_eq!(p.give(WOODEN_PICKAXE, 2), 2);
    assert_eq!((p.0[3], p.0[4]), (Slot { thing: WOODEN_PICKAXE, count: 1 }, Slot { thing: WOODEN_PICKAXE, count: 1 }), "tools one to a slot");
    // fill the hotbar, and the backpack takes the rest
    assert_eq!(p.give(t(SAND), 99 * 5 + 10), 505);
    assert_eq!((p.0[8], p.0[HOTBAR], p.0[HOTBAR + 1]), (Slot { thing: t(SAND), count: 99 }, Slot { thing: t(SAND), count: 99 }, Slot { thing: t(SAND), count: 10 }));
    // taking empties the backpack first, so the hotbar keeps what's in hand
    assert!(p.take(t(SAND), 120));
    assert_eq!((p.0[5].count, p.0[8].count, p.0[HOTBAR].count, p.0[HOTBAR + 1].count), (99, 88, 0, 0));
    assert!(!p.take(t(SAND), 1000), "not that many");
    assert_eq!(p.count(t(SAND)), 505 - 120, "and nothing taken");
    // a hotbar slot that runs out takes the backpack's next lot of it
    p.0[20] = Slot { thing: t(SAND), count: 40 };
    p.0[8].count = 0;
    p.refill(8);
    assert_eq!((p.0[8], p.0[20].count), (Slot { thing: t(SAND), count: 40 }, 0));
    // full: only what stacks onto what's there
    let mut full = Slots::default();
    full.give(WOODEN_PICKAXE, 100);
    assert_eq!((full.count(WOODEN_PICKAXE), full.give(t(COBBLE), 1), full.room_for(t(COBBLE))), (POCKETS as u32, 0, 0));

    // moving: into an empty slot, half, onto the same (as far as it stacks), swapping
    let mut p = Slots::default();
    p.0[0] = Slot { thing: t(PLANKS), count: 60 };
    p.0[1] = Slot { thing: t(PLANKS), count: 70 };
    p.0[2] = Slot { thing: STONE_AXE, count: 1 };
    assert!(p.shift(0, 20, true));
    assert_eq!((p.0[0].count, p.0[20]), (30, Slot { thing: t(PLANKS), count: 30 }), "half, rounded up");
    assert!(p.shift(1, 20, false));
    assert_eq!((p.0[1].count, p.0[20].count), (1, 99), "onto the same thing, as far as it stacks");
    assert!(p.shift(2, 0, false));
    assert_eq!((p.0[0], p.0[2]), (Slot { thing: STONE_AXE, count: 1 }, Slot { thing: t(PLANKS), count: 30 }), "swapped");
    assert!(!p.shift(2, 0, true) && !p.shift(5, 6, false) && !p.shift(3, 3, false), "half can't swap; nothing moves nothing");
    // tidying packs the backpack and leaves the hotbar alone
    p.0[30] = Slot { thing: t(PLANKS), count: 5 };
    p.0[12] = Slot { thing: t(COBBLE), count: 7 };
    p.tidy();
    let back: Vec<Slot> = p.0[HOTBAR..].iter().copied().filter(|s| !s.empty()).collect();
    assert_eq!(back, [Slot { thing: t(COBBLE), count: 7 }, Slot { thing: t(PLANKS), count: 99 }, Slot { thing: t(PLANKS), count: 5 }]);
    assert_eq!(p.0[0].thing, STONE_AXE);
    // what goes to the host, and back
    assert_eq!(Slots::from_filled(&p.filled()).filled(), p.filled());

    // the screen: the same key that opens Blocks in creative
    home();
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    crate::sim(&mut app, crate::SEED);
    let mut me = crate::player::Me::default();
    me.pockets = Slots::default();
    me.pockets.give(t(PLANKS), 40);
    me.pockets.give(STONE_AXE, 1);
    me.pockets.0[HOTBAR + 4] = Slot { thing: t(STONE), count: 12 };
    me.slot = 1;
    app.insert_resource(Settings { survival: true, ..default() }).insert_resource(me).insert_resource(crate::KnobEdits::default());
    app.world_mut().resource_mut::<WorldInfo>().in_world = true;
    let act = |app: &mut App, a: Act| {
        app.world_mut().resource_mut::<Acts>().0.push(a);
        app.world_mut().run_system_once(crate::game::do_acts).unwrap();
    };
    let press = |app: &mut App, f: &dyn Fn(&mut Controls)| {
        let mut c = Controls::default();
        f(&mut c);
        app.insert_resource(c);
        app.world_mut().run_system_once(crate::ui::menu_input).unwrap();
        app.world_mut().run_system_once(crate::game::do_acts).unwrap();
    };
    act(&mut app, Act::OpenBlocks);
    let items_now = |app: &App| {
        let w = app.world();
        menu_items(w.resource::<Focus>(), w.resource::<crate::creations::Library>(), w.resource::<Scripts>(), w.resource::<Settings>(), w.resource::<WorldInfo>())
    };
    let (title, list) = items_now(&app);
    assert_eq!(title, "Backpack");
    assert_eq!(list.iter().filter(|i| matches!(i.kind, Kind::Slot(_))).count(), POCKETS, "every slot");
    assert_eq!(list[0].kind, Kind::Slot(HOTBAR), "the backpack's rows first, then the hotbar");
    assert_eq!(list[HOTBAR + 4 - HOTBAR].label, "Stone \u{d7}12");
    let f = app.world().resource::<Focus>();
    assert_eq!((f.screen.clone(), list[f.sel].kind.clone()), (Screen::Backpack, Kind::Slot(1)), "on the slot in hand");
    // rows of nine: up from the axe is the backpack's last row
    app.world_mut().resource_mut::<Focus>().cols = HOTBAR;
    press(&mut app, &|c| c.up = true);
    let sel = app.world().resource::<Focus>().sel;
    assert_eq!(list[sel].kind, Kind::Slot(POCKETS - HOTBAR + 1));
    // the planks in the hotbar: half of them to the backpack
    press(&mut app, &|c| c.down = true);
    press(&mut app, &|c| c.left = true);
    press(&mut app, &|c| c.interact = true);
    assert_eq!(app.world().resource::<Focus>().moving, Some((0, true)));
    act(&mut app, Act::Slot(HOTBAR, false));
    let me = app.world().resource::<crate::player::Me>();
    assert_eq!((me.pockets.0[0].count, me.pockets.0[HOTBAR]), (20, Slot { thing: t(PLANKS), count: 20 }));
    assert_eq!(app.world().resource::<Focus>().pockets, me.pockets, "the screen shows it");
    // B while moving puts it back; B again leaves
    act(&mut app, Act::Slot(1, false));
    assert!(app.world().resource::<Focus>().moving.is_some());
    press(&mut app, &|c| c.back = true);
    assert!(app.world().resource::<Focus>().moving.is_none());
    assert_eq!(app.world().resource::<Focus>().screen, Screen::Backpack);
    // tidying
    act(&mut app, Act::Tidy);
    let me = app.world().resource::<crate::player::Me>();
    assert_eq!((me.pockets.0[HOTBAR], me.pockets.0[HOTBAR + 1]), (Slot { thing: t(PLANKS), count: 20 }, Slot { thing: t(STONE), count: 12 }));
    press(&mut app, &|c| c.blocks = true);
    assert_eq!(app.world().resource::<Focus>().screen, Screen::Playing, "the key that opened it closes it");
}

#[test]
fn a_client_half_reads_this_games_copy_of_the_world() {
    use crate::halves::{Halves, Seen};
    let mut voxels = world();
    let at = IVec3::new(3, 90, 3);
    voxels.set(at, crate::blocks::WOOL_RED);
    let client = r#"
local net = require("@potluck/net@0.2")
local world = require("@potluck/world@0.12")
local me = net.me()
print("block " .. world.block(vector.create(3.5, 90.2, 3.5)))
print("top " .. world.top(3, 3))
print("kind " .. world.kind("Oak log"))
local found = world.find_block(vector.create(3, 87, 3), 4, "Red wool")
print("found " .. found.x .. " " .. found.y)
print("players " .. #world.players() .. " " .. world.players()[2].name)
print("nearest " .. world.nearest_player(vector.create(2, 40, 0)).name)
print("aim " .. me.aim.y .. " look " .. me.look.z)
print(pcall(world.set_block, vector.create(0, 0, 0), "Stone") and "set ok" or "no set_block")
print(pcall(world.find_light, vector.create(0, 0, 0), 4) and "light ok" or "no find_light")
world.after(0.2, function()
	print("later " .. world.block(vector.create(3, 90, 3)) .. " aim " .. tostring(net.me().aim))
end)
"#;
    let mut c = creation("print(\"main\")");
    c.files.insert("client.luau".into(), client.into());
    c.manifest.client = Some("client.luau".into());
    let mut lib = Library::default();
    lib.list.insert("test".into(), c);
    lib.running.push("test".into());
    lib.world = 1;
    let ada = PlayerInfo { id: 7, name: "Ada".into(), pos: Vec3::new(0.0, 40.0, 0.0) };
    let bo = PlayerInfo { id: 8, name: "Bo".into(), pos: Vec3::new(2.0, 40.0, 0.5) };
    let mut h = Halves::default();
    {
        let seen = Seen { voxels: &voxels, players: vec![ada.clone(), bo.clone()], look: Vec3::new(0.0, 0.0, -1.0), aim: Some(at) };
        h.sync(Some(&lib), &ada, Some(&seen));
    }
    assert_eq!(h.list["test"].error, None, "{:?}", h.list["test"].log);
    h.take();
    let l = h.list["test"].log.join("\n");
    for want in ["block Red wool", "top 90", "kind wood", "found 3.5 90.5", "players 2 Bo", "nearest Bo", "aim 90.5 look -1", "no set_block", "no find_light"] {
        assert!(l.contains(want), "{want}: {l}");
    }
    // what changes in the world, it sees in the next frame
    voxels.set(at, crate::blocks::STONE);
    let seen = Seen { voxels: &voxels, players: vec![ada.clone()], look: Vec3::X, aim: None };
    h.frame(0.25, &ada, vec![], Some(&seen));
    h.take();
    let l = h.list["test"].log.join("\n");
    assert!(l.contains("later Stone aim nil"), "{l}");
    assert_eq!(h.list["test"].error, None);
}

// ---------- type checking ----------

const MISTAKES: &str = r#"--!strict
local world = require("@potluck/world@0.12")
local ui = require("@potluck/ui@0.2")
local sound = require("@potluck/sound@0.2")

local Frog = world.creature { name = "Frog", body = {} }

world.spwan(Frog, vector.create(0, 0, 0))
world.after("two", function() end)
pritn("hello")
local n = math.flor(2.5)
ui.show(nil, "hi", "Hello", { at = "middle" })
sound.play()
world.set_block(vector.create(0, 0, 0))
local function hop(height: number) return height end
hop("high")
function Frog:tick(dt)
	-- what the checker guesses about self isn't sure enough to show
	self.mood = "happy"
	sound.play("croak", { at = self })
	print(self.mood .. n)
end
"#;

#[test]
fn the_type_checker_says_only_what_its_sure_of() {
    use crate::check::{check, files_hash, keep_checking, Hints};
    home();
    let mut c = creation(MISTAKES);
    c.files.insert("client.luau".into(), "local net = require(\"@potluck/net@0.2\")\nnet.sned(\"hello\")\nprint(net.me().name)\n".into());
    c.manifest.client = Some("client.luau".into());
    let hints: Vec<String> = check(&c).iter().map(|h| h.to_string()).collect();
    assert_eq!(
        hints,
        [
            "client.luau line 2: net has no sned (did you mean net.send?)",
            "main.luau line 8: world has no spwan (did you mean world.spawn?)",
            "main.luau line 9: this should be a number, not some text",
            "main.luau line 10: there's nothing called pritn here (did you mean print?)",
            "main.luau line 11: math has no flor (did you mean math.floor?)",
            "main.luau line 12: this should be one of \"bottom\", \"left\" or \"right\", not \"middle\"",
            "main.luau line 13: this call takes 1 to 2 values, and it's given none",
            "main.luau line 14: this call takes 2 to 3 values, and it's given 1",
            "main.luau line 16: this should be a number, not some text",
        ]
    );
    // the base game's creations have nothing it's sure of
    let lib = Library::load();
    for c in lib.list.values().filter(|c| matches!(c.origin, Origin::Base)) {
        assert_eq!(check(c), vec![], "{}", c.id);
    }
    // the player's own creations are checked as they change, off the frame
    let mut app = App::new();
    let mut mine = Library::default();
    mine.list.insert(c.id.clone(), c.clone());
    app.insert_resource(mine).init_resource::<Hints>().add_systems(Update, keep_checking);
    let started = std::time::Instant::now();
    while app.world().resource::<Hints>().of(&c).is_none() && started.elapsed().as_secs() < 30 {
        app.update();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(app.world().resource::<Hints>().of(&c).map(|h| h.len()), Some(9));
    app.world_mut().resource_mut::<Library>().list.get_mut("test").unwrap().files.insert("main.luau".into(), "print(\"fixed\")\n".into());
    let fixed = app.world().resource::<Library>().list["test"].clone();
    assert_ne!(files_hash(&fixed), files_hash(&c));
    assert!(app.world().resource::<Hints>().of(&fixed).is_none(), "what it found before is for the old files");
    while app.world().resource::<Hints>().of(&fixed).is_none() && started.elapsed().as_secs() < 30 {
        app.update();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(app.world().resource::<Hints>().of(&fixed).map(|h| h.len()), Some(1), "the client half's still wrong");
}


/// A number for the island's blocks above y = 0, so a change to how it's
/// made shows here (galleries and saved worlds compare against it).
fn island_print(v: &Voxels) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for y in 1..world::WY {
        for z in 0..world::WZ {
            for x in 0..world::WX {
                h = (h ^ v.get(x, y, z) as u64).wrapping_mul(0x100000001b3);
            }
        }
    }
    h
}

#[test]
fn the_island_is_made_as_it_always_was() {
    // checked block for block against the island maker before the world
    // went on past it, then again when caves and ores came (world::GEN 2):
    // they change only what's under the ground and cave mouths inland
    assert_eq!(island_print(&world::generate(26)), 0x7d426827b76f8aec);
    assert_eq!(island_print(&world::generate(7)), 0xc04af4975cbce73c);
}

#[test]
fn the_world_goes_on_past_the_island_in_lands_of_many_kinds() {
    use crate::world::{wild, Biome};
    let seed = crate::SEED;
    // open sea right round the island, then land
    for (x, z) in [(-60.0, 190.0), (450.0, 190.0), (190.0, -60.0), (190.0, 450.0)] {
        assert!(world::ground_height(seed, x as i32, z as i32) < world::SEA, "sea just off the island at {x}, {z}");
    }
    // far out, every kind of land turns up somewhere
    let mut seen = std::collections::HashSet::new();
    for i in 0..160 {
        for j in 0..160 {
            let (x, z) = (-12000.0 + i as f32 * 150.0, -12000.0 + j as f32 * 150.0);
            seen.insert(wild::biome(seed, x, z));
        }
    }
    for b in [Biome::Sea, Biome::Meadow, Biome::Forest, Biome::Birchwood, Biome::Pinewood, Biome::Snowfield, Biome::Desert, Biome::Savanna, Biome::Marsh, Biome::Mountain] {
        assert!(seen.contains(&b), "no {} anywhere", b.name());
    }
    // what crosses a column's edge joins up: columns made one at a time
    // are the same as made together, trees and all
    let mut trees = 0;
    for k0 in [IVec2::new(30, 4), IVec2::new(-40, 70), IVec2::new(100, -100), IVec2::new(-200, -3)] {
        for (k, together) in wild::columns(seed, k0, IVec2::new(2, 2)) {
            let alone = world::column(seed, k);
            for y in world::MIN_Y..world::WY {
                for z in 0..world::CHUNK {
                    for x in 0..world::CHUNK {
                        let b = alone.get(x, y, z);
                        assert_eq!(b, together.get(x, y, z), "column {k} at {x}, {y}, {z}");
                        trees += (b == crate::blocks::LOG || b == crate::blocks::BIRCH) as u32;
                    }
                }
            }
        }
    }
    assert!(trees > 0, "some trees to check");
    // a column is the same whenever it's made
    let k = IVec2::new(-17, 23);
    let (a, b) = (world::column(seed, k), world::column(seed, k));
    assert!((world::MIN_Y..world::WY).all(|y| (0..32).all(|z| (0..32).all(|x| a.get(x, y, z) == b.get(x, y, z)))));
}

#[test]
fn the_ground_goes_down_to_bedrock_far_below_the_sea() {
    use crate::blocks::{BEDROCK, SLATE, STONE};
    let mut v = world();
    // under the island, and under the lands past it
    for (x, z) in [(world::START.x as i32, world::START.z as i32), (190, 190), (-700, 1200)] {
        v.ensure(world::key_of(x, z));
        assert_eq!(v.get(x, world::MIN_Y, z), BEDROCK, "bedrock at the bottom");
        assert_eq!(v.get(x, -100, z), SLATE, "slate deep down");
        assert!(v.top(x, z) > -40);
    }
    assert_eq!(v.get(world::START.x as i32, -10, world::START.z as i32), STONE, "stone under the island");
    // nobody digs through the bottom
    assert!(!v.set(IVec3::new(5, world::MIN_Y, 5), crate::blocks::AIR));
    assert!(v.set(IVec3::new(5, world::MIN_Y + 6, 5), crate::blocks::AIR), "but anywhere above it");
    assert!(crate::player::blocked(&v, Vec3::new(5.2, world::MIN_Y as f32 - 0.5, 5.2), Vec3::splat(0.6)));
}

#[test]
fn columns_come_and_go_and_keep_what_players_changed() {
    let mut v = world();
    let k = IVec2::new(world::CX + 6, -3);
    let p = IVec3::new(k.x * world::CHUNK + 5, 40, k.y * world::CHUNK + 7);
    assert!(!v.has(k), "not made until someone's near");
    assert_eq!(v.at(p), crate::blocks::AIR);
    assert!(v.set(p, crate::blocks::PLANKS), "building there makes it");
    assert!(v.has(k));
    v.unload(k);
    assert!(!v.has(k), "let go");
    v.ensure(k);
    assert_eq!(v.at(p), crate::blocks::PLANKS, "and back as it was left");
    // the island stays
    v.unload(IVec2::new(2, 2));
    assert!(v.has(IVec2::new(2, 2)));
    // saved and sent: only what changed, the rest from the seed
    let bytes = v.encode();
    assert!(bytes.len() < 200_000, "{} bytes", bytes.len());
    let mut back = Voxels::decode(v.seed, &bytes).unwrap();
    back.ensure(k);
    assert_eq!(back.at(p), crate::blocks::PLANKS);
    let island = IVec3::new(world::START.x as i32, 30, world::START.z as i32);
    assert_eq!(back.at(island), v.at(island));
    assert!(Voxels::decode(v.seed, &bytes[..bytes.len() - 3]).is_none(), "damaged bytes are refused");
    // a change from the host to land this game hasn't made waits for it
    let mut guest = world();
    guest.apply(p, crate::blocks::GLASS);
    assert!(!guest.has(k));
    guest.ensure(k);
    assert_eq!(guest.at(p), crate::blocks::GLASS);
    // what's wanted round players: this game's own player further than friends
    let w = world::stream::wanted(Some(Vec3::new(10.0, 0.0, 10.0)), &[Vec3::new(1000.0, 0.0, 0.0)], 8);
    assert_eq!(w, vec![(IVec2::ZERO, 8), (IVec2::new(31, 0), world::stream::AROUND_FRIENDS)]);
    let mut round = world();
    world::stream::make_round(&mut round, Vec3::new(-300.0, 0.0, 50.0), 1);
    assert!(round.has(world::key_of(-300, 50)) && round.has(world::key_of(-332, 82)));
}

/// A pool for swimming tests: water from y 10 to 17 (its top at 17.88)
/// over sand, with a bank on the east at the water's height.
fn pool() -> App {
    use crate::blocks::{SAND, WATER};
    let mut v = Voxels::empty(1);
    for x in -4..60 {
        for z in -4..24 {
            v.set(IVec3::new(x, 9, z), SAND);
            for y in 10..=17 {
                v.set(IVec3::new(x, y, z), if x < 16 { WATER } else { SAND });
            }
        }
    }
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    let mut focus = crate::ui::Focus::default();
    focus.go(crate::ui::Screen::Playing);
    app.insert_resource(v)
        .insert_resource(focus)
        .insert_resource(crate::settings::Settings { survival: true, ..Default::default() })
        .init_resource::<crate::input::Controls>()
        .init_resource::<crate::ui::Toasts>()
        .init_resource::<crate::sound::Heard>()
        .insert_resource(crate::player::Me::default());
    app
}

/// Moves the player for `secs` at 60 frames a second, returning the
/// highest they were.
fn swim_for(app: &mut App, secs: f32) -> f32 {
    use bevy::ecs::system::RunSystemOnce;
    let mut top = f32::MIN;
    for _ in 0..(secs * 60.0) as i32 {
        app.world_mut().resource_mut::<Time>().advance_by(std::time::Duration::from_micros(16_667));
        app.world_mut().run_system_once(crate::player::walk).unwrap();
        top = top.max(app.world().resource::<crate::player::Me>().pos.y);
    }
    top
}

#[test]
fn players_float_swim_up_and_down_and_climb_out() {
    use crate::input::Controls;
    use crate::player::Me;
    let mut app = pool();
    let me = |app: &App| (app.world().resource::<Me>().pos, app.world().resource::<Me>().vel, app.world().resource::<Me>().under_water);
    app.world_mut().resource_mut::<Me>().pos = Vec3::new(6.5, 11.0, 6.5);
    // let go under water, they come up and float with their eyes out
    swim_for(&mut app, 4.0);
    let (pos, vel, under) = me(&app);
    let rest = 17.88 - 1.35;
    assert!((pos.y - rest).abs() < 0.05 && vel.y.abs() < 0.05 && !under, "floating at {pos} {vel}");
    // and stay there without bobbing
    let top = swim_for(&mut app, 2.0);
    assert!(top - rest < 0.03, "no bob: up to {top}");
    // crouch dives, letting go comes back up
    app.world_mut().resource_mut::<Controls>().crouch = true;
    swim_for(&mut app, 1.0);
    let (pos, _, under) = me(&app);
    assert!(pos.y < rest - 2.5 && under, "dived to {pos}");
    app.world_mut().resource_mut::<Controls>().crouch = false;
    swim_for(&mut app, 4.0);
    assert!((me(&app).0.y - rest).abs() < 0.05, "back up");
    // jump swims up out of the water a little and falls back to floating
    app.world_mut().resource_mut::<Controls>().jump = true;
    let top = swim_for(&mut app, 1.0);
    assert!(top > rest + 0.2, "up to {top}");
    app.world_mut().resource_mut::<Controls>().jump = false;
    swim_for(&mut app, 3.0);
    // swimming east to the bank, they climb out onto it
    {
        let mut m = app.world_mut().resource_mut::<Me>();
        m.yaw = -std::f32::consts::FRAC_PI_2;
        m.pitch = 0.0;
    }
    app.world_mut().resource_mut::<Controls>().mv = Vec2::new(0.0, 1.0);
    swim_for(&mut app, 6.0);
    let (pos, _, _) = me(&app);
    let m = app.world().resource::<Me>();
    assert!(pos.x > 16.3 && pos.y > 17.99 && m.on_ground && !m.in_water, "out on the bank at {pos}");
    // and water breaks a fall in survival
    app.world_mut().resource_mut::<Controls>().mv = Vec2::ZERO;
    {
        let mut m = app.world_mut().resource_mut::<Me>();
        m.pos = Vec3::new(6.5, 40.0, 6.5);
        m.vel = Vec3::ZERO;
    }
    swim_for(&mut app, 4.0);
    assert_eq!(app.world().resource::<Me>().health, crate::player::MAX_HEALTH);
}

/// Where each kind of land is nearest the island, for looking at
/// (`cargo test -- --ignored lands_to_look_at --nocapture`).
#[test]
#[ignore]
fn lands_to_look_at() {
    use crate::world::{wild, Biome};
    let seed = crate::SEED;
    for b in [Biome::Meadow, Biome::Forest, Biome::Birchwood, Biome::Pinewood, Biome::Snowfield, Biome::Desert, Biome::Savanna, Biome::Marsh, Biome::Mountain] {
        let mut found = None;
        'out: for r in (8..400).map(|r| r * 24) {
            for i in 0..(r / 6) {
                let a = i as f32 / (r / 6) as f32 * std::f32::consts::TAU;
                let (x, z) = (192.0 + a.cos() * r as f32, 192.0 + a.sin() * r as f32);
                // a patch of it, not a corner
                let all = (-2..=2).all(|dx| (-2..=2).all(|dz| wild::biome(seed, x + dx as f32 * 20.0, z + dz as f32 * 20.0) == b));
                if all {
                    found = Some((x as i32, z as i32, world::ground_height(seed, x as i32, z as i32)));
                    break 'out;
                }
            }
        }
        println!("LAND {:?} {:?}", b, found);
    }
}

#[test]
fn underground_is_the_same_wherever_and_whenever_it_is_worked_out() {
    use crate::blocks::*;
    use crate::underground::carve;
    // a column worked out bottom up, then top down, then again later on its own
    let seed = 1234;
    let up: Vec<BlockId> = (-127..=60).map(|y| carve(seed, 400, y, -77, 60, STONE)).collect();
    let mut down: Vec<BlockId> = (-127..=60).rev().map(|y| carve(seed, 400, y, -77, 60, STONE)).collect();
    down.reverse();
    assert_eq!(up, down);
    let _ = carve(seed, 9, 9, 9, 40, STONE);
    assert_eq!(up, (-127..=60).map(|y| carve(seed, 400, y, -77, 60, STONE)).collect::<Vec<_>>());
    assert_ne!(up, (-127..=60).map(|y| carve(seed + 1, 400, y, -77, 60, STONE)).collect::<Vec<_>>(), "another seed, another underground");
    // only the ground changes, and nothing at the very bottom
    for b in [WATER, SAND, LOG, BEDROCK, AIR] {
        assert!((-127..=60).all(|y| carve(seed, 400, y, -77, 60, b) == b));
    }
    assert!((-127..=-120).all(|y| (0..64).all(|x| carve(seed, x, y, 5, 60, STONE) == STONE)), "a floor at the bottom");
}

#[test]
fn caves_ores_hollows_and_old_workings_are_found_at_their_depths() {
    use crate::blocks::*;
    use crate::underground::carve;
    let seed = 77;
    let mut found: std::collections::HashMap<BlockId, Vec<i32>> = Default::default();
    let (mut air, mut all) = (0, 0);
    let ground = 64;
    let started = std::time::Instant::now();
    // a 160 x 160 stretch, every block from the bottom to the ground
    for z in (0..160).step_by(1) {
        for x in 0..160 {
            for y in -127..=ground {
                let b = carve(seed, x, y, z, ground, STONE);
                all += 1;
                if b == AIR {
                    air += 1;
                }
                if b != STONE && b != AIR {
                    found.entry(b).or_default().push(y);
                }
            }
        }
    }
    let took = started.elapsed();
    println!("{all} blocks in {took:?}: {:.0} ns a block", took.as_nanos() as f64 / all as f64);
    let open = air as f32 / all as f32;
    assert!((0.02..0.14).contains(&open), "caves open {:.1}% of the ground", open * 100.0);
    for (b, from, to) in [(COAL_ORE, -16, 96), (COPPER_ORE, -48, 64), (IRON_ORE, -80, 32), (GOLD_ORE, -112, -24), (MOONSILVER_ORE, -124, -88)] {
        let at = found.get(&b).cloned().unwrap_or_default();
        assert!(at.len() > 20, "{}: {}", block(b).name, at.len());
        assert!(at.iter().all(|y| (from..=to).contains(y)), "{} outside {from} to {to}", block(b).name);
    }
    let count = |b| found.get(&b).map_or(0, |v| v.len());
    assert!(count(COAL_ORE) > 3 * count(MOONSILVER_ORE), "coal is common and moonsilver rare");
    assert!(count(CRYSTAL_ORE) > 0, "crystal in veins and hollows");
    assert!(count(LOG) > 0 && count(PLANKS) > 0, "old workings' posts and beams");
    assert!(count(OLD_CHEST) > 0, "old chests");
    assert!(count(GLOWCAP) > 0, "glowcaps on cave floors");
    // chests stand on something, and the workings' corridors are open
    for z in 0..160 {
        for x in 0..160 {
            for y in -127..ground {
                if carve(seed, x, y, z, ground, STONE) == OLD_CHEST {
                    let under = carve(seed, x, y - 1, z, ground, STONE);
                    assert!(is_solid(under), "a chest at {x} {y} {z} over {under}");
                }
            }
        }
    }
}

#[test]
fn caves_keep_away_from_the_sea_and_mostly_from_the_surface() {
    use crate::blocks::*;
    use crate::underground::carve;
    let seed = 5;
    // a sea floor: nothing opens within a few blocks of it
    for z in 0..96 {
        for x in 0..96 {
            for y in 6..=12 {
                let b = carve(seed, x, y, z, 12, STONE);
                assert!(is_solid(b), "{x} {y} {z} under the sea: {b}");
            }
        }
    }
    // inland, caves come up to the grass only now and then (cave mouths)
    let mut mouths = 0;
    for z in 0..256 {
        for x in 0..256 {
            if carve(seed, x, 70, z, 70, GRASS) == AIR {
                mouths += 1;
            }
        }
    }
    assert!(mouths > 0 && mouths < 256 * 256 / 50, "{mouths} holes in the grass");
}

/// The pup, as its creation makes it.
fn pup() -> crate::script::KindDef {
    home();
    let mut herd = Herd::default();
    let mut scripts = Scripts::default();
    scripts.start(&Library::load().list.remove("pup").expect("the base game has pups"), &mut herd).unwrap();
    scripts.vms["pup"].kinds[0].clone()
}

#[test]
fn creatures_step_up_a_block_and_stand_where_their_feet_are() {
    use crate::blocks::{FENCE, GRASS};
    let k = pup();
    assert!(k.half.x < 0.45, "a pup bumps into things about as wide as its legs stand, not as long as it is: {:?}", k.half);
    // flat ground with its top at 21; east of x 24 a step one block up, a
    // wall three high to the north, and a fence to the south
    let mut w = Voxels::empty(1);
    for x in 0..48 {
        for z in 0..48 {
            for y in 0..=20 {
                w.set(IVec3::new(x, y, z), GRASS);
            }
            if x >= 24 {
                w.set(IVec3::new(x, 21, z), GRASS);
            }
            if z == 10 && x < 24 {
                for y in 21..24 {
                    w.set(IVec3::new(x, y, z), GRASS);
                }
            }
            if z == 38 && x < 24 {
                w.set(IVec3::new(x, 21, z), FENCE);
            }
        }
    }
    let mut rng = Rng::new(1);
    let mut herd = Herd::default();
    let run = |herd: &mut Herd, rng: &mut Rng, secs: f32, each: &mut dyn FnMut(&creatures::Body)| {
        for _ in 0..(secs * 60.0) as usize {
            creatures::step(herd, &w, 1.0 / 60.0, rng);
            each(herd.bodies.values().next().unwrap());
        }
    };
    let id = herd.spawn("pup", &k, Vec3::new(18.5, 21.0, 24.5));
    let walk = |herd: &mut Herd, to: Vec3| {
        let b = herd.bodies.get_mut(&id).unwrap();
        b.goal = Some(to);
        b.wander = false;
    };
    // up the step, with no hop
    walk(&mut herd, Vec3::new(30.5, 22.0, 24.5));
    let mut rose = 0.0f32;
    run(&mut herd, &mut rng, 5.0, &mut |b| rose = rose.max(b.vel.y));
    let b = &herd.bodies[&id];
    assert_eq!(b.pos.y, 22.0, "it walked up onto the step: {:?}", b.pos);
    assert!(b.pos.x > 29.0, "and on to where it was going: {:?}", b.pos);
    assert!(rose <= 0.0, "without a hop: it rose at {rose}");
    // and back down
    walk(&mut herd, Vec3::new(18.5, 21.0, 24.5));
    run(&mut herd, &mut rng, 5.0, &mut |_| {});
    assert_eq!(herd.bodies[&id].pos.y, 21.0);
    // a wall it can't climb stops it where it stands, still on the ground
    walk(&mut herd, Vec3::new(18.5, 21.0, 4.5));
    let mut highest = 0.0f32;
    run(&mut herd, &mut rng, 5.0, &mut |b| highest = highest.max(b.pos.y));
    assert_eq!(highest, 21.0, "it never left the ground at the wall");
    assert!((herd.bodies[&id].pos.z - 11.0 - k.half.z).abs() < 0.01, "it's up against the wall: {:?}", herd.bodies[&id].pos);
    // nor does it step over a fence
    walk(&mut herd, Vec3::new(18.5, 21.0, 44.5));
    run(&mut herd, &mut rng, 8.0, &mut |b| highest = highest.max(b.pos.y));
    assert_eq!(highest, 21.0, "a fence keeps it in");
    assert!(herd.bodies[&id].pos.z < 38.0);
    // dropped beside the step, it stands on the ground under its feet, not
    // on the block beside it
    let b = herd.bodies.get_mut(&id).unwrap();
    b.pos = Vec3::new(24.0 - 0.45, 23.5, 24.5);
    b.goal = None;
    b.yaw = 0.0;
    b.vel = Vec3::ZERO;
    run(&mut herd, &mut rng, 2.0, &mut |_| {});
    assert_eq!(herd.bodies[&id].pos.y, 21.0, "it stands where its feet are: {:?}", herd.bodies[&id].pos);
    // a step up shows as a climb, a few frames long, not a pop
    let mut shown = Vec3::new(10.0, 21.0, 10.0);
    let mut frames = 0;
    while shown.y < 21.99 && frames < 60 {
        shown = creatures::shown_at(shown, Vec3::new(10.0, 22.0, 10.0), 1.0 / 60.0);
        frames += 1;
    }
    assert!((8..30).contains(&frames), "{frames} frames to show a step");
}

#[test]
fn creatures_are_picked_anywhere_on_their_bodies() {
    let k = pup();
    let mut herd = Herd::default();
    let id = herd.spawn("pup", &k, Vec3::ZERO);
    for yaw in [0.0f32, 1.0, 2.5, 4.0] {
        let b = herd.bodies.get_mut(&id).unwrap();
        b.yaw = yaw;
        let b = &herd.bodies[&id];
        let nose = Quat::from_rotation_y(yaw) * Vec3::new(0.0, 0.85, -0.84);
        let side = Quat::from_rotation_y(yaw) * Vec3::X;
        // aimed down at its nose from above and in front
        let eye = nose + Vec3::new(0.0, 1.5, 0.0) - side * 0.01;
        assert!(b.ray(eye, Vec3::NEG_Y).is_some(), "its nose is part of it, facing {yaw}");
        // beside it, a hand's width off its flank, misses
        let flank = side * 0.5 + Vec3::Y * 2.0;
        assert!(b.ray(flank, Vec3::NEG_Y).is_none(), "beside it misses, facing {yaw}");
    }
}

#[test]
fn creatures_near_the_start_neither_hop_by_themselves_nor_float() {
    home();
    let lib = Library::load();
    let mut voxels = world();
    let mut herd = Herd::default();
    let mut scripts = Scripts::default();
    // these only hop when someone rides, uses or hits them
    for id in ["pup", "woolly", "hedgehog", "mossback", "villagers"] {
        scripts.start(&lib.list[id], &mut herd).unwrap();
    }
    let start = crate::script::spot("start").unwrap();
    let ground = |v: &Voxels, p: Vec3| {
        let (x, z) = (p.x.floor() as i32, p.z.floor() as i32);
        let mut y = p.y.floor() as i32;
        while y > 0 && !crate::blocks::is_solid(v.get(x, y, z)) {
            y -= 1;
        }
        y as f32 + 1.0
    };
    let mut rng = Rng::new(1);
    // each body's creation, hops, frames on the ground, frames held up
    // with no foot on the ground, and whether it was on the ground last frame
    let mut seen: BTreeMap<u64, (String, u32, u32, u32, bool)> = BTreeMap::new();
    let dt = 1.0 / 60.0;
    for i in 0..(60.0 / dt) as usize {
        // a player walks round the start, so the pups follow
        let a = i as f32 * dt / 20.0 * std::f32::consts::TAU;
        let mut p = start + Vec3::new(a.cos() * 6.0, 0.0, a.sin() * 6.0);
        p.y = voxels.top(p.x.floor() as i32, p.z.floor() as i32) as f32 + 1.0;
        scripts.players = vec![PlayerInfo { id: 1, name: "Ken".into(), pos: p }];
        creatures::step(&mut herd, &voxels, dt, &mut rng);
        scripts.tick(&mut voxels, &mut herd, dt);
        for b in herd.bodies.values() {
            let s = seen.entry(b.id).or_insert((b.creation.clone(), 0, 0, 0, false));
            // (a body that walks into water floats up, which isn't a hop)
            let wet = voxels.at(b.pos.floor().as_ivec3()) == crate::blocks::WATER;
            if s.4 && !b.on_ground && b.vel.y > 0.0 && !wet {
                s.1 += 1;
            }
            if b.on_ground {
                s.2 += 1;
                // held up in the air: no leg (or, with none, not its middle)
                // over the block it stands on
                let turn = Quat::from_rotation_y(b.yaw);
                let mut feet: Vec<Vec3> = b.parts.iter().filter(|p| p.leg).map(|p| b.pos + turn * Vec3::new(p.at.x, 0.0, p.at.z)).collect();
                if feet.is_empty() {
                    feet.push(b.pos);
                }
                if feet.iter().all(|f| ground(&voxels, *f) < b.pos.y - 0.5) {
                    s.3 += 1;
                }
            }
            s.4 = b.on_ground;
        }
    }
    for (creation, hops, grounded, held_up, _) in seen.values() {
        assert_eq!(*hops, 0, "a {creation} hopped by itself");
        let held = *held_up as f32 / (*grounded).max(1) as f32;
        println!("{creation}: on the ground {grounded} frames, held up by the block beside it {:.1}% of them", held * 100.0);
        if creation == "pup" {
            assert!(held < 0.05, "a pup stood a block up in the air {:.0}% of the time", held * 100.0);
        }
    }
}

/// The climate's tint leaves the island and the lands near it exactly as
/// they were, and shifts grass and leaves further out.
#[test]
fn climate_tints_spare_the_island() {
    use crate::mesh::{climate_tint, leaf_tint, Tints};
    for seed in [crate::SEED, 7, 99_991] {
        for z in (-140..520).step_by(23) {
            for x in (-140..520).step_by(29) {
                let t = climate_tint(world::climate(seed, x as f32, z as f32));
                assert_eq!(t, Vec3::ONE, "tint at {x}, {z} with seed {seed}");
                assert_eq!(leaf_tint(t), Vec3::ONE);
            }
        }
        for k in [IVec2::ZERO, IVec2::new(5, 7), IVec2::new(-3, 14)] {
            let t = Tints::of(seed, k);
            for (dx, dz) in [(0.0, 0.0), (7.5, 31.0), (16.0, 16.0)] {
                assert_eq!(t.at(k.x as f32 * 32.0 + dx, k.y as f32 * 32.0 + dz), Vec3::ONE, "column {k}");
            }
        }
    }
    // far out, somewhere is yellower (warm and dry) and somewhere cooler
    let (mut dry, mut cold) = (false, false);
    for i in 0..60 {
        for j in 0..60 {
            let t = climate_tint(world::climate(crate::SEED, -9000.0 + i as f32 * 300.0, -9000.0 + j as f32 * 300.0));
            dry |= t.z < 0.85 && t.x > 1.05;
            cold |= t.x < 0.9 && t.z >= 0.95;
        }
    }
    assert!(dry && cold, "dry {dry}, cold {cold}");
}

/// The far land fills every column in reach that isn't drawn, with holes
/// where columns are, ground where the seed puts it, the sea over what's
/// under it, and the open sea on past.
#[test]
fn far_land_fills_what_isnt_drawn() {
    use crate::render::far::{Job, Spot, STEP};
    let seed = crate::SEED;
    let palette = crate::map::Palette::default();
    let at = Vec2::new(190.0, 190.0);
    let drawn: std::collections::HashSet<IVec2> = (4..8).flat_map(|z| (4..8).map(move |x| IVec2::new(x, z))).collect();
    let job = |spots| Job { seed, at, reach: 300.0, sea: 2000.0, drawn: drawn.clone(), spots, colours: crate::render::far::Colours::from_palette(&palette) };
    let built = job(Default::default()).build();
    let pos = |m: &Mesh| match m.attribute(Mesh::ATTRIBUTE_POSITION) {
        Some(bevy::mesh::VertexAttributeValues::Float32x3(p)) => p.clone(),
        _ => vec![],
    };
    let land = pos(&built.land);
    assert!(!land.is_empty());
    // nothing inside a drawn column (its edges are shared with skirts)
    for p in &land {
        let k = IVec2::new((p[0] / 32.0).floor() as i32, (p[2] / 32.0).floor() as i32);
        let inside = p[0] % 32.0 != 0.0 && p[2] % 32.0 != 0.0;
        assert!(!(inside && drawn.contains(&k)), "far land at {p:?} in a drawn column");
    }
    // the ground where the seed puts it (the canopy over it where there are
    // trees), short of where it starts sinking toward the far end
    let mut matched = 0;
    for p in land.iter().filter(|p| Vec2::new(p[0], p[2]).distance(at) < 240.0 && p[0] as i32 % STEP == 0 && p[2] as i32 % STEP == 0) {
        let s = Spot::at(seed, p[0] as i32, p[2] as i32);
        if s.trees.0 <= 0.0 && (p[1] - s.ground).abs() < 0.01 {
            matched += 1;
        }
        assert!(p[1] >= s.ground - crate::render::far::SKIRT - 0.01, "far land at {p:?} under the ground at {}", s.ground);
    }
    assert!(matched > 20, "only {matched} points at the seed's ground");
    // the water: over the sea round the island, and on out past the land
    let water = pos(&built.water);
    assert!(water.iter().all(|p| (p[1] - (world::SEA as f32 + 0.88)).abs() < 1e-4));
    assert!(water.iter().any(|p| Vec2::new(p[0], p[2]).distance(at) > 1500.0), "the open sea goes on");
    // what was worked out is kept: a second go round the same spot needs nothing new
    let n = built.spots.len();
    let again = job(built.spots).build();
    assert_eq!(again.spots.len(), n);
}

/// Under water it's darker the deeper you go, and dark at night.
#[test]
fn under_water_darkens() {
    use crate::render::under_water_colour as c;
    assert!(c(1.0, 0.0).length() > c(1.0, 20.0).length());
    assert!(c(1.0, 0.0).length() > 4.0 * c(0.0, 0.0).length());
}

#[test]
fn chunks_know_which_faces_open_space_joins() {
    use crate::blocks::{AIR, STONE};
    use crate::mesh::cull::{connections, joins, ALL};
    let mut w = Voxels::empty(1);
    // high in the sky over the island, a chunk of stone with a tunnel
    // running east to west through it, and a closed room in its middle
    let c = IVec3::new(1, 3, 1);
    let o = c * 32;
    assert_eq!(connections(&w, c), ALL, "all air");
    for y in 0..32 {
        for z in 0..32 {
            for x in 0..32 {
                w.set(o + IVec3::new(x, y, z), STONE);
            }
        }
    }
    assert_eq!(connections(&w, c), 0, "solid right through");
    for x in 0..32 {
        w.set(o + IVec3::new(x, 5, 9), AIR);
    }
    for (x, y, z) in [(10, 20, 10), (11, 20, 10), (11, 21, 10)] {
        w.set(o + IVec3::new(x, y, z), AIR);
    }
    let j = connections(&w, c);
    assert!(joins(j, 0, 1) && joins(j, 1, 0), "the tunnel joins east and west");
    assert!(!joins(j, 2, 3) && !joins(j, 0, 2) && !joins(j, 4, 5), "nothing else: {j:b}");
}

#[test]
fn cave_culling_reaches_what_open_space_leads_to() {
    use crate::mesh::cull::{reached, ALL};
    // a column of chunks: sky from y 1 up, solid rock under it with a cave
    // in chunk y -2 that no tunnel reaches, and a shaft down from the
    // surface beside it at x 1
    let join = |c: IVec3| {
        if c.y >= 1 {
            ALL
        } else if c.x == 1 && c.z == 0 && c.y >= -3 {
            // the shaft: up and down only
            1 << (2 * 6 + 3) | 1 << (3 * 6 + 2)
        } else {
            0
        }
    };
    let shown = |c: IVec3| c.x.abs() <= 3 && c.z.abs() <= 3;
    let seen = reached(IVec3::new(0, 2, 0), shown, join);
    assert!(seen.contains(&IVec3::new(3, 1, -3)), "the open air all round");
    assert!(seen.contains(&IVec3::new(0, 0, 0)), "the ground under the camera");
    assert!(!seen.contains(&IVec3::new(0, -2, 0)), "a cave nothing leads to");
    assert!(seen.contains(&IVec3::new(1, -3, 0)), "the shaft's foot");
    assert!(!seen.contains(&IVec3::new(2, -3, 0)), "rock beside the shaft");
    // from inside the closed cave, only the chunks touching it
    let inside = reached(IVec3::new(0, -2, 0), shown, join);
    assert!(inside.contains(&IVec3::new(0, -1, 0)) && !inside.contains(&IVec3::new(0, 1, 0)), "rock walls the cave in");
    // never back toward the camera: up the shaft and over to a chunk
    // that's only open sideways, from below, isn't seen
    let seen_from_below = reached(IVec3::new(1, -3, 0), shown, join);
    assert!(seen_from_below.contains(&IVec3::new(1, 2, 0)), "up the shaft to the sky");
    assert!(!seen_from_below.contains(&IVec3::new(3, 0, 0)), "not back down onto the ground past it");
}

#[test]
fn the_block_atlas_leaves_room_for_creations_blocks() {
    let mut images = Assets::<Image>::default();
    let atlas = crate::textures::build(&mut images);
    // the base game's creations alone made more than 66 tiles fill it (#59)
    assert!(atlas.room() >= 300, "room for {} more tiles", atlas.room());
}

/// Storage's dispensers and Rails' powered rails take power from the
/// Circuits creation's blocks beside them, by name: their own tests can't
/// put down a lever (only the tested creation's blocks are there), so
/// these play with Circuits' blocks registered.
#[test]
fn dispensers_and_powered_rails_run_off_circuits() {
    home();
    let mut lib = Library::load().list;
    let circuits = lib.remove("circuits").expect("the base game has circuits");
    crate::made::register(&circuits.id, &circuits.manifest.blocks).unwrap();
    for (id, script) in [("storage", DISPENSERS), ("rails", POWERED_RAILS)] {
        let mut c = lib.remove(id).expect("a base creation");
        c.files.insert(crate::testing::FILE.into(), script.into());
        let report = crate::testing::run(&c, &world());
        println!("{}", report.text());
        assert!(report.passed(), "{}", report.text());
    }
}

const DISPENSERS: &str = r##"
local test = require("@potluck/test@0.7")
local world = require("@potluck/world@0.15")
local store = require("@potluck/store@0.1")
local EAST, SOUTH = vector.create(1, 0, 0), vector.create(0, 0, 1)

local function held(at)
	return (store.get("boxes") or {})[math.floor(at.x) .. "," .. math.floor(at.y) .. "," .. math.floor(at.z)] or {}
end
local function put(p, at, thing)
	test.hold(p, thing)
	test.use_block(p, at)
	test.answer(p, 1)
end
-- a dispenser facing east, with a lever beside it, and the player near
local function dispenser(name, carrying)
	carrying.Dispenser = 1
	local p = test.player(name, { survival = true, carrying = carrying })
	local x, z = math.floor(p.position.x) + 2, math.floor(p.position.z)
	local d = vector.create(x, world.top(x, z) + 1, z)
	for _, c in { d, d + EAST, d + EAST * 2, d + SOUTH } do
		world.set_block(c, "Air")
	end
	test.place(p, d, "Dispenser", "east")
	test.move(p, d + vector.create(1.5, 0, 2.5))
	return p, d
end
local function pulse(lever)
	world.set_block(lever, "Pulled lever")
	test.wait(0.3)
	world.set_block(lever, "Lever")
	test.wait(0.3)
end

test.case("power gives out a thing at a time: a block is put down, the rest handed over", function()
	local p, d = dispenser("Ada", { Stone = 2, Bread = 1 })
	put(p, d, "Stone")
	put(p, d, "Bread")
	test.same(world.has(p, "Bread"), 0, "the bread's in")
	pulse(d + SOUTH)
	test.same(world.has(p, "Bread"), 1, "bread first, by name, handed over")
	pulse(d + SOUTH)
	test.same(world.block(d + EAST), "Stone", "a block put down in front")
	pulse(d + SOUTH)
	test.same(world.has(p, "Stone"), 1, "with no room in front, handed over")
	test.wait(2.1)
	test.same(next(held(d)), nil, "it's empty")
	test.check(test.heard("click") >= 3, "it clicks")
end)

test.case("a lit inverter facing it powers it, and it pours water and fills the bucket again", function()
	local p, d = dispenser("Bo", { ["Water bucket"] = 1 })
	put(p, d, "Water bucket")
	world.set_block(d - EAST, "Lit inverter", "east")
	test.wait(0.3)
	test.same(world.block(d + EAST), "Water", "poured")
	test.wait(2.1)
	test.same(held(d).Bucket, 1, "the bucket's kept")
	world.set_block(d - EAST, "Inverter", "east")
	test.wait(0.3)
	world.set_block(d - EAST, "Lit inverter", "east")
	test.wait(2.4)
	test.same(held(d)["Water bucket"], 1, "filled again from the water in front")
end)
"##;

const POWERED_RAILS: &str = r##"
local test = require("@potluck/test@0.7")
local world = require("@potluck/world@0.23")

local function floor()
	local g = world.spot("green")
	local x0, z0, y = math.floor(g.x) + 60, math.floor(g.z) - 12, 90
	for dz = 0, 23 do
		for dx = 0, 11 do
			world.set_block(vector.create(x0 + dx, y, z0 + dz), "Stone")
		end
		test.wait(0)
	end
	return vector.create(x0, y + 1, z0)
end
local function at(o, x, y, z)
	return o + vector.create(x, y, z)
end
local function cart_on(c, from)
	local p = test.player("Ada", { survival = true, carrying = { Cart = 1 }, at = from })
	test.place(p, c + vector.create(0, 1, 0), "Cart")
	test.wait(0.2)
	local list = world.creatures(test.kind("Cart"))
	test.same(#list, 1, "a cart")
	return p, list[1]
end
local function where(id)
	return (test.creature(id) :: any).position
end

test.case("a powered rail with live wire beside it sends a cart on fast", function()
	local o = floor()
	for z = 2, 22 do
		world.set_block(at(o, 4, 0, z), if z == 6 then "Powered rail" else "Rail", "north")
	end
	world.set_block(at(o, 5, 0, 6), "Live wire")
	local p, id = cart_on(at(o, 4, 0, 3), at(o, 4, 0, 1))
	test.ride(p, test.creature(id))
	test.press(p, "push")
	test.wait(0.2)
	test.release(p, "push")
	local before, after, last = 0, 0, where(id)
	for _ = 1, 40 do
		test.wait(0.1)
		local now = where(id)
		local speed = vector.magnitude(now - last) * 10
		if now.z < o.z + 6 then
			before = math.max(before, speed)
		elseif now.z > o.z + 9 then
			after = math.max(after, speed)
		end
		last = now
	end
	test.check(after > before + 2, "faster after it: " .. before .. " then " .. after)
end)

test.case("a cart resting on a powered rail sets off when power reaches it", function()
	local o = floor()
	for z = 2, 22 do
		world.set_block(at(o, 4, 0, z), if z == 12 then "Powered rail" else "Rail", "north")
	end
	local _, id = cart_on(at(o, 4, 0, 12), at(o, 6, 0, 12))
	test.wait(1)
	local rest = where(id)
	world.set_block(at(o, 5, 0, 12), "Pulled lever")
	test.wait(2)
	test.check(vector.magnitude(where(id) - rest) > 3, "it went: " .. tostring(where(id) - rest))
end)
"##;

/// Every base creation's blocks fit in the block atlas and a world's ids
/// for made blocks, with room to spare for players' own creations.
#[test]
fn the_base_games_blocks_leave_room_for_players_own() {
    home();
    let mut images = Assets::<Image>::default();
    let mut atlas = crate::textures::build(&mut images);
    let (mut ids, mut used) = (0, 0);
    for c in Library::load().list.values().filter(|c| c.origin == Origin::Base) {
        for (key, def) in crate::made::read_all(&c.manifest.blocks).unwrap() {
            ids += if def.turns { 4 } else { 1 };
            // a picture each, but faces with the same picture share, and a
            // block that turns has a top for each way it faces
            for i in (0..3).filter(|&i| def.same[i] == i) {
                for q in 0..if i == 0 && def.turns { 4 } else { 1 } {
                    let name: &'static str = Box::leak(format!("base/{}/{key}/{i}/{q}", c.id).into_boxed_str());
                    let f = &def.faces[i];
                    assert!(atlas.put(&mut images, name, &f.rgba, f.size, &f.glow).is_some(), "room in the atlas for {}'s {key}", c.id);
                    used += 1;
                }
            }
        }
    }
    let plain = vec![[200u8, 40, 40, 255]; 16 * 16];
    let mut left = 0;
    while atlas.put(&mut images, Box::leak(format!("test/spare{left}").into_boxed_str()), &plain, 16, &vec![false; 256]).is_some() {
        left += 1;
    }
    println!("the base game's own blocks take {ids} ids and {used} pictures, leaving {left} pictures");
    assert!(left >= 64, "room in the atlas for players' blocks: {left} pictures");
    assert!(ids <= 128 - 24, "ids for players' blocks: the base game takes {ids} of 128");
}

#[test]
fn glowing_ores_and_glowcaps_glow_a_little_without_lighting_anything() {
    use crate::blocks::*;
    use crate::mesh::{Detail, FAINT, GLOW};
    let mut images = Assets::<Image>::default();
    let atlas = crate::textures::build(&mut images);
    let quads = |b: BlockId| {
        let mut w = Voxels::empty(1);
        w.set(IVec3::new(4, 4, 4), b);
        let m = crate::mesh::mesh_chunk(&w, &atlas, IVec3::ZERO, crate::mesh::Cover::default(), Detail::Near);
        let rock = m.rock.into_mesh(true).count_vertices() / 4;
        let n: Vec<usize> = m.layers.into_iter().map(|l| l.into_mesh(false).count_vertices() / 4).collect();
        (n, rock)
    };
    // crystal and moonsilver show all six faces glowing, and the ray
    // tracer gets the same faces as plain rock
    for ore in [CRYSTAL_ORE, MOONSILVER_ORE] {
        let (n, rock) = quads(ore);
        assert_eq!((n[0], n[FAINT], n[GLOW], rock), (0, 6, 0, 6), "{}", block(ore).name);
    }
    // a glowcap's sheets glow, both ways round, and the tracer leaves them out
    let (n, rock) = quads(GLOWCAP);
    assert!(n[FAINT] >= 4 && n[3] == 0 && rock == 0, "{n:?} {rock}");
    // the other ores don't glow, and lanterns stay lamps
    let (n, rock) = quads(IRON_ORE);
    assert_eq!((n[0], n[FAINT], rock), (6, 0, 0));
    let (n, _) = quads(LANTERN);
    assert!(n[GLOW] > 0 && n[FAINT] == 0, "{n:?}");
}

/// The triangles of a mesh, as corners, each with the normal its corners
/// carry.
fn triangles(m: &Mesh) -> Vec<([Vec3; 3], Vec3)> {
    use bevy::mesh::VertexAttributeValues;
    let Some(VertexAttributeValues::Float32x3(pos)) = m.attribute(Mesh::ATTRIBUTE_POSITION) else { panic!("no positions") };
    let Some(VertexAttributeValues::Float32x3(nrm)) = m.attribute(Mesh::ATTRIBUTE_NORMAL) else { panic!("no normals") };
    let idx: Vec<usize> = m.indices().unwrap().iter().collect();
    idx.chunks(3).map(|t| ([0, 1, 2].map(|k| Vec3::from(pos[t[k]])), Vec3::from(nrm[t[0]]))).collect()
}

#[test]
fn things_in_hand_are_cubes_or_cut_out_pictures_with_edges() {
    use crate::held;
    use crate::textures::Outline;
    let mut images = Assets::<Image>::default();
    let atlas = crate::textures::build(&mut images);
    // a block is a cube, every face outward
    let cube = held::cube([[0.0, 0.0, 1.0, 1.0]; 3]);
    let tris = triangles(&cube);
    assert_eq!(tris.len(), 12);
    for ([a, b, c], n) in &tris {
        assert!((b - a).cross(c - a).dot(*n) > 0.0, "a cube's face turns in");
        assert!((a + b + c).dot(*n) > 0.0, "a cube's face is on the far side");
    }
    // a picture all filled is its front, its back and a strip along each
    // pixel of its edge, all facing out
    let full = Outline { n: 8, solid: vec![true; 64] };
    let tris = triangles(&held::sprite(&full, [0.0, 0.0, 1.0, 1.0]));
    assert_eq!(tris.len() / 2, 2 + 4 * 8);
    for ([a, b, c], n) in &tris {
        assert!((b - a).cross(c - a).dot(*n) > 0.0, "a picture's face turns in");
        let mid = (a + b + c) / 3.0;
        assert!(mid.x.abs() <= 0.5 && mid.y.abs() <= 0.5 && mid.z.abs() <= 0.5 / 8.0 + 1e-6);
    }
    // a tool's picture has an edge wherever a pixel that shows meets one
    // that doesn't
    let o = &atlas.outlines["pickaxe_iron"];
    let edges: usize = (0..o.n as i32)
        .flat_map(|y| (0..o.n as i32).map(move |x| (x, y)))
        .filter(|&(x, y)| o.at(x, y))
        .map(|(x, y)| [(-1, 0), (1, 0), (0, -1), (0, 1)].iter().filter(|(dx, dy)| !o.at(x + dx, y + dy)).count())
        .sum();
    assert!(edges > 20, "the iron pickaxe's picture is empty");
    let tris = triangles(&held::sprite(o, BlockAtlas::uv(atlas.tile("pickaxe_iron"))));
    assert_eq!(tris.len() / 2, 2 + edges);
}

use crate::textures::BlockAtlas;

#[test]
fn blocks_in_hand_are_cubes_tools_are_pictures_and_flames_glow() {
    use crate::held::{self, Making};
    use crate::items::by_name;
    use bevy::ecs::system::RunSystemOnce;
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    let mut images = Assets::<Image>::default();
    let atlas = crate::textures::build(&mut images);
    // the chunks' own materials, as the game makes them
    let mut mats = Assets::<StandardMaterial>::default();
    let blocks = crate::render::make_materials(&atlas, &mut images, &mut mats, &mut Assets::default(), crate::render::Raytraced(false), crate::render::Tier::Medium);
    app.insert_resource(atlas).insert_resource(images).insert_resource(blocks).insert_resource(mats).init_resource::<held::HeldLooks>().init_resource::<Assets<Mesh>>();
    let looks = app
        .world_mut()
        .run_system_once(|mut m: Making| ["Stone", "Iron pickaxe", "Torch", "Crystal ore", "Planks"].map(|n| (m.look(by_name(n).unwrap()), m.look(by_name(n).unwrap()))))
        .unwrap();
    let mats = app.world().resource::<Assets<StandardMaterial>>();
    let glows = |l: &held::Look| l.parts.iter().any(|(_, m)| mats.get(m).unwrap().emissive != LinearRgba::BLACK);
    let [stone, pick, torch, crystal, planks] = looks.map(|(a, b)| {
        let (a, b) = (a.unwrap(), b.unwrap());
        // made once
        assert_eq!(a.parts[0].0, b.parts[0].0);
        a
    });
    assert!(stone.cube && planks.cube && crystal.cube && torch.cube, "blocks and models are themselves");
    assert!(!pick.cube, "a tool is its picture");
    // a torch is its stick and its glowing flame
    assert_eq!(torch.parts.len(), 2);
    assert!(glows(&torch) && glows(&crystal) && !glows(&stone) && !glows(&pick));
    // a picture shows both sides
    assert!(mats.get(&pick.parts[0].1).unwrap().double_sided && mats.get(&stone.parts[0].1).unwrap().cull_mode.is_some());
    // held up in the air, nothing in hand takes the sky light the chunks' corners do
    assert!([&stone, &pick, &torch].iter().flat_map(|l| &l.parts).all(|(_, m)| mats.get(m).unwrap().occlusion_texture.is_none()));
    assert!(app.world_mut().run_system_once(|mut m: Making| m.look(0).is_none()).unwrap());
}

#[test]
fn a_flame_in_hand_lights_the_way_and_people_hold_things_in_their_right_hand() {
    use crate::held::{self, HandLight, Holds, InHand};
    use crate::items::{by_name, Slot};
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    let mut images = Assets::<Image>::default();
    let torch = by_name("Torch").unwrap();
    let pick = by_name("Iron pickaxe").unwrap();
    let mut me = crate::player::Me::default();
    me.slot = 0;
    me.pockets.0[0] = Slot { thing: torch, count: 1 };
    app.insert_resource(crate::textures::build(&mut images))
        .insert_resource(images)
        .insert_resource(me)
        .insert_resource(crate::settings::Settings { survival: true, ..Default::default() })
        .init_resource::<crate::render::Clock>()
        .init_resource::<crate::render::Eye>()
        .init_resource::<held::HeldLooks>()
        .init_resource::<creatures::PartMaterials>()
        .init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<StandardMaterial>>()
        .add_systems(Update, (held::hand_light, (creatures::people, held::hands).chain()));
    let body = app.world_mut().spawn((Transform::default(), Visibility::default(), creatures::Person::new(30.0), Holds(pick))).id();
    for _ in 0..3 {
        app.update();
    }
    let lights = |app: &mut App| app.world_mut().query::<&HandLight>().iter(app.world()).count();
    assert_eq!(lights(&mut app), 1, "a torch in hand gives light");
    // the pickaxe is in the person's right hand, and only there
    let held: Vec<(Entity, Thing)> = app.world_mut().query::<(Entity, &InHand)>().iter(app.world()).map(|(e, h)| (e, h.0)).collect();
    assert_eq!(held.len(), 1);
    assert_eq!(held[0].1, pick);
    let hand = app.world().get::<ChildOf>(held[0].0).unwrap().parent();
    let part = app.world().get::<creatures::PartView>(hand).unwrap();
    assert!(app.world().get::<creatures::Person>(body).unwrap().is_right_hand(part));
    // putting the torch away puts its light out; emptying a hand empties it
    app.world_mut().resource_mut::<crate::player::Me>().pockets.0[0] = Slot { thing: by_name("Stone").unwrap(), count: 1 };
    app.world_mut().get_mut::<Holds>(body).unwrap().0 = 0;
    for _ in 0..2 {
        app.update();
    }
    assert_eq!(lights(&mut app), 0);
    assert_eq!(app.world_mut().query::<&InHand>().iter(app.world()).count(), 0);
}

use crate::items::Thing;

#[test]
fn flames_have_stand_ins_for_the_ray_tracer_and_caves_keep_the_sky_haze_out() {
    use crate::blocks::*;
    use crate::mesh::Detail;
    use bevy::ecs::system::RunSystemOnce;
    use bevy::pbr::AtmosphereSettings;
    let mut images = Assets::<Image>::default();
    let atlas = crate::textures::build(&mut images);
    let mut w = Voxels::empty(1);
    w.set(IVec3::new(4, 4, 4), LANTERN);
    w.set(IVec3::new(8, 4, 4), TORCH);
    w.set(IVec3::new(8, 5, 4), STONE);
    w.set(IVec3::new(12, 4, 4), STONE);
    let m = crate::mesh::mesh_chunk(&w, &atlas, IVec3::ZERO, crate::mesh::Cover::default(), Detail::Near);
    let tris = triangles(&m.lamps.into_mesh(true));
    // a box each, facing out, inside its own block
    assert_eq!(tris.len(), 2 * 12);
    for ([a, b, c], n) in &tris {
        assert!((b - a).cross(c - a).dot(*n) > 0.0, "a stand-in's face turns in");
        let mid = (a + b + c) / 3.0;
        let block = mid.floor().as_ivec3();
        assert!(matches!(w.get(block.x, block.y, block.z), LANTERN | TORCH), "a stand-in pokes out of its block at {mid}");
    }
    // deep underground the sky's haze is spread too far to fall in the cave
    let mut app = App::new();
    app.add_plugins(MinimalPlugins).init_resource::<crate::render::Eye>();
    let cam = app.world_mut().spawn((crate::player::MainCamera, AtmosphereSettings::default())).id();
    let far = |app: &mut App| app.world().get::<AtmosphereSettings>(cam).unwrap().aerial_view_lut_max_distance;
    app.world_mut().run_system_once(crate::render::cave_air).unwrap();
    assert_eq!(far(&mut app), AtmosphereSettings::default().aerial_view_lut_max_distance);
    app.world_mut().resource_mut::<crate::render::Eye>().below = 1.0;
    app.world_mut().run_system_once(crate::render::cave_air).unwrap();
    assert!(far(&mut app) > 500.0 * AtmosphereSettings::default().aerial_view_lut_max_distance);
}

/// sky 0.1: what creations say of the weather reaches the host and every
/// game, lightning is heard by every creation, and each game eases into
/// the weather and glows softly at a strike (never with Reduce flashing).
#[test]
fn the_sky_eases_in_everywhere_and_lightning_glows_softly() {
    use crate::net::{decode, frame_msg, Event, Msg};
    use crate::sky::{Kind, Season, Sky, Thunder, Weather, GLOW_FOR};
    use crate::transport::Wire;
    home();
    let mut voxels = world();
    let mut herd = Herd::default();
    let mut scripts = Scripts::default();
    // one creation makes a storm, another hears where it strikes
    let maker = r#"
local sky = require("@potluck/sky@0.1")
local world = require("@potluck/world")
sky.set_weather("storm", { strength = 0.8, wind = vector.create(30, 0, 0), over = 10 })
sky.set_season("autumn", 0.25)
world.after(0.2, function() assert(sky.strike(vector.create(5, 60, 5))) end)
"#;
    let hearer = r#"
local sky = require("@potluck/sky@0.1")
local world = require("@potluck/world")
sky.on("strike", function(at) print("struck " .. at.x .. " " .. at.y .. " " .. at.z) end)
"#;
    scripts.start(&creation(maker), &mut herd).unwrap();
    let mut other = creation(hearer);
    other.id = "hearer".into();
    scripts.start(&other, &mut herd).unwrap();
    let sky = scripts.sky_changes().expect("the sky changed");
    assert_eq!((sky.kind, sky.season, sky.progress, sky.over), (Kind::Storm, Season::Autumn, 0.25, 10.0));
    assert!((sky.strength - 0.8).abs() < 1e-6 && (Vec2::from(sky.wind).length() - crate::sky::MAX_WIND).abs() < 1e-4, "the wind's as strong as it gets: {sky:?}");
    assert_eq!(scripts.sky_changes(), None, "said once");
    run(&mut scripts, &mut voxels, &mut herd, 0.5);
    assert_eq!(scripts.vms["test"].error, None, "{:?}", scripts.vms["test"].log);
    assert!(scripts.vms["hearer"].log.iter().any(|l| l == "struck 5 60 5"), "{:?}", scripts.vms["hearer"].log);
    assert_eq!(scripts.strike_changes(), vec![Vec3::new(5.0, 60.0, 5.0)], "for every game");
    assert!(scripts.strike_changes().is_empty());

    // what goes between devices, tidied when it comes in
    for m in [Msg::Sky(sky), Msg::Strike([5.0, 60.0, 5.0])] {
        let Some(Event::Msg(_, back)) = decode(Wire::Data(1, frame_msg(&m))) else { panic!("didn't decode") };
        assert_eq!(serde_json::to_string(&back).unwrap(), serde_json::to_string(&m).unwrap());
    }
    let wild = Sky { strength: f32::NAN, wind: [1e9, -1e9], over: -3.0, progress: 7.0, ..sky }.tidy();
    assert_eq!((wild.strength, wild.over, wild.progress), (0.0, 0.0, 1.0));
    assert!(Vec2::from(wild.wind).length() <= crate::sky::MAX_WIND + 1e-4);

    // a game eases into it: nothing all at once
    let mut w = Weather::default();
    w.take(Sky::default());
    assert!(!w.fresh && w.rain == 0.0, "a new world starts as it is");
    w.take(sky);
    w.step(1.0);
    assert!(w.rain > 0.0 && w.rain < 0.2 && w.cloud < 0.2, "a second in: {w:?}");
    for _ in 0..12 {
        w.step(1.0);
    }
    let [rain, _, _, cloud, storm] = Weather::targets(&sky);
    assert!((w.rain - rain).abs() < 1e-4 && (w.cloud - cloud).abs() < 1e-4 && (w.storm - storm).abs() < 1e-4, "there after `over`: {w:?}");
    assert!(w.autumn > 0.0 && w.autumn < 0.2, "the leaves turn slowly: {}", w.autumn);
    // and joining a world, it's there at once
    let mut joined = Weather { fresh: true, ..default() };
    joined.take(sky);
    assert_eq!((joined.rain, joined.storm), (rain, storm));
    // the land never jumps from one season to the next
    let at = |season, progress| Weather::season_targets(&Sky { season, progress, ..default() });
    for (a, b) in [(Season::Spring, Season::Summer), (Season::Summer, Season::Autumn), (Season::Autumn, Season::Winter), (Season::Winter, Season::Spring)] {
        let (end, start) = (at(a, 1.0), at(b, 0.0));
        assert!((end.0 - start.0).abs() < 1e-6 && (end.1 - start.1).abs() < 1e-6, "{a:?} to {b:?}: {end:?} then {start:?}");
    }
    assert_eq!(at(Season::Winter, 0.5), (0.0, 1.0), "deep in winter, snow and no leaves turning");
    assert_eq!(at(Season::Autumn, 0.5), (1.0, 0.0));

    // lightning: a soft swell and fade, never a flash, and none at all
    // with Reduce flashing on; thunder after, by how far it was
    let mut thunder = Thunder::default();
    crate::sky::strike(&mut w, &mut thunder, Vec3::new(0.0, 60.0, 0.0), Some(Vec3::new(340.0, 60.0, 0.0)));
    assert!(w.glow(false) == 0.0 && w.glow(true) == 0.0);
    let mut last = 0.0;
    let mut most = 0.0f32;
    for i in 1..=40 {
        w.step(0.05);
        let g = w.glow(false);
        assert!((g - last).abs() <= 0.5 * 0.05 / 0.15 + 1e-4, "no sudden change at {i}: {last} to {g}");
        assert_eq!(w.glow(true), 0.0, "never with Reduce flashing");
        most = most.max(g);
        last = g;
    }
    assert!(most > 0.4 && most <= 0.5, "a soft brightening: {most}");
    w.step(GLOW_FOR);
    assert!(w.strikes.is_empty() && w.glow(false) == 0.0, "and gone");
    assert_eq!(thunder.0.len(), 1);
    assert!((thunder.0[0].0 - 1.0).abs() < 1e-4, "heard a second later from 340 blocks: {:?}", thunder.0);
}

const PANTRY_TOML: &str = r##"
name = "Pantry"
by = "Tests"
kind = "mod"
main = "main.luau"

[items.plum_jam]
kind = "food"
food = 7
says = "You ate some plum jam. Sweet!"
colors = { a = "#803060" }
picture = ["aaaaaaaa", "aaaaaaaa", "aaaaaaaa", "aaaaaaaa", "aaaaaaaa", "aaaaaaaa", "aaaaaaaa", "aaaaaaaa"]

[items.felt_hat]
kind = "armor"
slot = "head"
guard = 4
warm = true
colors = { a = "#605040" }
picture = ["........", "..aaaa..", ".aaaaaa.", ".aaaaaa.", "aaaaaaaa", "aaaaaaaa", "........", "........"]

[items.club]
kind = "weapon"
power = 7
colors = { a = "#806040" }
picture = ["......aa", ".....aaa", "....aaa.", "...aaa..", "..aaa...", ".aa.....", "aa......", "a......."]

[items.flint_pick]
name = "Flint pick"
kind = "tool"
digs = { hard = 3, level = 2 }
recipe = { Flint = 2, Planks = 1 }
colors = { a = "#404048", b = "#806040" }
picture = ["aaaaaa..", "......a.", "....b..a", "...b...a", "..b.....", ".b......", "b.......", "........"]
"##;

/// world 0.25: creations make items of their own, which Hunger eats, Armor
/// wears, the Workbench makes and tools dig with, and guests take as the
/// host gave them.
#[test]
fn creations_make_items_of_their_own() {
    use crate::items::{self, FIRST_MADE_ITEM};
    use crate::made::items as made;
    home();
    let spec = |t: &str| -> serde_json::Value { serde_json::to_value(toml::from_str::<toml::Value>(t).unwrap()).unwrap() };
    let pic = "colors = { a = \"#fff\" }\npicture = [\"aaaaaaaa\", \"aaaaaaaa\", \"aaaaaaaa\", \"aaaaaaaa\", \"aaaaaaaa\", \"aaaaaaaa\", \"aaaaaaaa\", \"aaaaaaaa\"]";
    // what's wrong is said plainly
    for (bad, says) in [
        (format!("kind = \"gadget\"\n{pic}"), "kind is one of"),
        (format!("name = \"Iron sword\"\n{pic}"), "has something called Iron sword"),
        (format!("name = \"Stone\"\n{pic}"), "has something called Stone"),
        (format!("kind = \"armor\"\n{pic}"), "needs a slot"),
        (format!("kind = \"armor\"\nslot = \"tail\"\n{pic}"), "needs a slot"),
        (format!("food = 4\n{pic}"), "food is for an item of kind = \"food\""),
        (format!("kind = \"weapon\"\npower = 50\n{pic}"), "power is"),
        (format!("kind = \"tool\"\ndigs = {{ metal = 2 }}\n{pic}"), "digs knows"),
        (format!("recipe = {{}}\n{pic}"), "recipe is"),
        (format!("makes = 2\n{pic}"), "give it a recipe too"),
        (format!("recipe = {{ Coal = 0.5 }}\n{pic}"), "recipe"),
        (format!("smell = \"sweet\"\n{pic}"), "there's no smell for an item"),
        ("colors = { a = \"#fff\" }".to_string(), "needs a picture"),
    ] {
        let e = made::read("thing", &spec(&bad)).err().unwrap_or_else(|| panic!("{bad} should be refused"));
        assert!(e.contains(says), "{bad}: {e}");
    }
    let def = made::read("lamp_oil", &spec(pic)).unwrap();
    assert_eq!((def.name.as_str(), def.kind, def.stack), ("Lamp oil", "material", items::BLOCK_STACK));
    assert_eq!(made::read("shiv", &spec(&format!("kind = \"weapon\"\n{pic}"))).unwrap().stack, 1, "a weapon stacks one to a slot");

    // a creation's items get ids from FIRST_MADE_ITEM up, and keep them
    let code = r#"local world = require("@potluck/world@0.25")
local jam = world.item("Plum jam")
print(jam and jam.kind, jam and jam.food, jam and jam.by)
local pick = world.item("Iron pickaxe")
print(pick and pick.by, tostring(world.item("Stone") == nil), #world.items())"#;
    let mut c = creation(code);
    c.id = "test-pantry".into();
    c.manifest = toml::from_str(PANTRY_TOML).unwrap();
    let ids = made::register(&c.id, &c.manifest.items).unwrap();
    assert!(ids.iter().all(|&id| id >= FIRST_MADE_ITEM), "{ids:?}");
    assert_eq!(made::register(&c.id, &c.manifest.items).unwrap(), ids, "the same ids the second time");
    let (club, hat, pick, jam) = (ids[0], ids[1], ids[2], ids[3]);
    assert_eq!(items::by_name("plum jam"), Some(jam));
    assert_eq!(items::by_name("Flint pick"), Some(pick));
    assert_eq!(items::name(hat), "Felt hat");
    assert_eq!(items::icon(jam), "test-pantry/plum_jam/item");
    assert_eq!((items::stack(jam), items::stack(club)), (items::BLOCK_STACK, 1));
    assert!(items::carriable(club) && items::is_item(club));
    let (info, kind) = made::info(hat).unwrap();
    assert_eq!(kind, "armor");
    assert_eq!((info["slot"].as_str(), info["guard"].as_f64(), info["by"].as_str()), (Some("head"), Some(4.0), Some("test-pantry")));
    assert!(info.get("picture").is_none(), "its picture is the game's to draw");

    // its code reads them as soon as it starts, and its tools dig
    let mut voxels = world();
    let mut herd = Herd::default();
    let mut scripts = Scripts::default();
    scripts.players = vec![PlayerInfo { id: 7, name: "Ada".into(), pos: Vec3::ZERO }];
    scripts.start(&c, &mut herd).unwrap();
    run(&mut scripts, &mut voxels, &mut herd, 0.1);
    let vm = &scripts.vms["test-pantry"];
    assert_eq!(vm.error, None, "{:?}", vm.log);
    assert!(vm.log.iter().any(|l| l == "food 7 test-pantry"), "{:?}", vm.log);
    assert!(vm.log.iter().any(|l| l.starts_with("game true ") && !l.ends_with(" 0")), "{:?}", vm.log);
    let digs = scripts.digging().into_iter().find(|d| d.0 == pick).map(|d| d.1).expect("the pick digs");
    assert_eq!((digs.speeds[1], digs.level), (3.0, 2));

    // Hunger eats made food, the most filling first
    let lib = Library::load();
    let mut hunger = lib.list["hunger"].clone();
    hunger.knobs.insert("minutes".into(), KnobValue::Number(5.0));
    let carrots = crate::blocks::by_name("Carrots").unwrap();
    scripts.carry(7, true, &[(jam, 1), (carrots.into(), 1), (hat, 1), (items::by_name("Flint").unwrap(), 2), (crate::blocks::PLANKS.into(), 1)], 0);
    scripts.start(&hunger, &mut herd).unwrap();
    run(&mut scripts, &mut voxels, &mut herd, 122.0);
    assert_eq!((scripts.carried(7, jam), scripts.carried(7, carrots)), (0, 1), "the jam first");
    let ate = scripts.shared.lock().unwrap().ui.pieces.get(&(Some(7), "hunger".into(), "ate".into())).map(|p| p.text.clone());
    assert_eq!(ate.as_deref(), Some("You ate some plum jam. Sweet!"));

    // Armor wears made armor by its slot and guard, and draws it on them
    scripts.start(&lib.list["armor"], &mut herd).unwrap();
    scripts.set_held(7, hat);
    scripts.use_item(7, Vec3::Y);
    run(&mut scripts, &mut voxels, &mut herd, 0.6);
    assert_eq!(scripts.carried(7, hat), 0, "it's on");
    let sh = scripts.shared.lock().unwrap();
    let g = sh.guard.get(&(7, "armor".to_string())).copied();
    assert!(g.is_some_and(|g| (g - (1.0 - 4.0 * 0.035)).abs() < 1e-4), "{g:?}");
    assert_eq!(sh.worn.get(&7).map(|w| w[0]), Some(hat));
    drop(sh);

    // the Workbench makes what has a recipe, with its kind's
    let table = crate::blocks::by_name("Table").unwrap();
    scripts.start(&lib.list["workbench"], &mut herd).unwrap();
    scripts.use_block(7, IVec3::new(3, 60, 3), table);
    run(&mut scripts, &mut voxels, &mut herd, 0.1);
    let asked = |scripts: &mut Scripts| scripts.shared.lock().unwrap().ui.panels.get(&7).and_then(|q| q.front().cloned());
    let kinds = asked(&mut scripts).expect("what to make");
    let tools = kinds.choices.iter().position(|c| c == "Tools").expect("Tools");
    scripts.answer(7, kinds.ask, Some(tools as u32));
    run(&mut scripts, &mut voxels, &mut herd, 0.1);
    let mut found = false;
    for _ in 0..6 {
        let page = asked(&mut scripts).expect("the tools");
        if let Some(i) = page.choices.iter().position(|c| c.contains("Flint pick")) {
            scripts.answer(7, page.ask, Some(i as u32));
            found = true;
            break;
        }
        assert_eq!(page.choices.last().map(String::as_str), Some("More"), "{:?}", page.choices);
        scripts.answer(7, page.ask, Some(page.choices.len() as u32 - 1));
        run(&mut scripts, &mut voxels, &mut herd, 0.1);
    }
    assert!(found, "the Flint pick is on the bench");
    run(&mut scripts, &mut voxels, &mut herd, 0.1);
    assert_eq!(scripts.carried(7, pick), 1, "made");
    assert_eq!(scripts.carried(7, items::by_name("Flint").unwrap()), 0);
    for id in ["hunger", "armor", "workbench", "test-pantry"] {
        assert_eq!(scripts.vms[id].error, None, "{id}: {:?}", scripts.vms[id].log);
    }

    // a guest takes the host's ids as they are
    let mut guest = made::table().into_iter().find(|e| e.id == jam).unwrap();
    let to = (FIRST_MADE_ITEM..FIRST_MADE_ITEM + items::MADE_ITEMS as items::Thing).rev().find(|&t| items::made_item(t).is_none()).unwrap();
    guest.id = to;
    let before = made::generation();
    made::adopt(&[guest]);
    assert!(made::generation() > before);
    assert!(items::made_item(jam).is_none(), "the old place is free again");
    assert_eq!(items::by_name("Plum jam"), Some(to));
    assert!(made::names().contains(&"Plum jam"));
}
