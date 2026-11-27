//! potluck-server: a Potluck world that runs with nobody at the keyboard,
//! for friends to join at any time. It serves plain WebSockets on one port
//! and, given a certificate, TLS on another, so it can live at an https
//! address. A web browser pointed at either port sees who's playing.
//!
//! Its settings are in server.toml in its data folder (an example is
//! written there on the first run), or the file --config names:
//! who may join, what they may do, the creations it always runs, whether
//! it's in the market's server list, and other servers it can send
//! players to. The command line and the POTLUCK_* environment variables
//! win over the file:
//!
//!     potluck-server [--config=server.toml] [--name="Riverbend"] [--port=47126] [--seed=26]
//!                    [--tls-port=443 --cert=fullchain.pem --key=privkey.pem]
//!                    [--data=/var/lib/potluck] [--no-beacon] [--max-players=50]

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use bevy::app::ScheduleRunnerPlugin;
use bevy::prelude::*;

use potluck::creations::{Creation, Library, Origin, Payload};
use potluck::hosting::ServerConfig;
use potluck::*;

const SAVE_EVERY: f32 = 60.0;

#[derive(Resource)]
struct Opts {
    cfg: ServerConfig,
    /// The market's key, for passes (fetched before starting).
    market_key: Option<String>,
    /// Creations the server always runs, fetched from the market.
    pinned: Vec<Payload>,
}

#[derive(Resource, Default)]
struct Saving {
    since: f32,
}

fn arg(args: &[String], key: &str, env: &str) -> Option<String> {
    args.iter().find_map(|a| a.strip_prefix(&format!("--{key}=")).map(|s| s.to_string())).or_else(|| std::env::var(env).ok().filter(|v| !v.is_empty()))
}

fn world_file() -> PathBuf {
    settings::data_dir().join("world.potluck")
}

fn creations_file() -> PathBuf {
    settings::data_dir().join("world-creations.json")
}

fn boards_file() -> PathBuf {
    settings::data_dir().join("boards.json")
}

const HELP: &str = "Options (or the POTLUCK_* environment variables; these win over server.toml):
  --config=FILE        the settings file (POTLUCK_CONFIG), server.toml in the data folder by default
  --name=NAME          what players see (POTLUCK_NAME)
  --port=47126         plain WebSocket and status page (POTLUCK_PORT)
  --tls-port=443       TLS: wss:// and https:// (POTLUCK_TLS_PORT)
  --cert=FILE          TLS certificate chain, PEM (POTLUCK_CERT)
  --key=FILE           TLS private key, PEM (POTLUCK_KEY)
  --seed=26            the world's seed for a new world (POTLUCK_SEED)
  --max-players=50     how many may play at once (POTLUCK_MAX_PLAYERS)
  --data=DIR           where the world is kept (POTLUCK_HOME)
  --no-beacon          don't announce the world on the local network
  --check              read the settings, say what's wrong, and stop";

/// The settings: server.toml, then the environment and command line over it.
fn settings_from(args: &[String]) -> Result<ServerConfig, String> {
    let path = arg(args, "config", "POTLUCK_CONFIG").map(PathBuf::from).unwrap_or_else(|| settings::data_dir().join("server.toml"));
    if !path.exists() && arg(args, "config", "POTLUCK_CONFIG").is_none() {
        // somewhere to start: every setting, commented
        let _ = std::fs::write(&path, ServerConfig::example());
    }
    let mut c = ServerConfig::read(&path)?;
    if let Some(v) = arg(args, "name", "POTLUCK_NAME") {
        c.name = v;
    }
    if let Some(v) = arg(args, "port", "POTLUCK_PORT").and_then(|p| p.parse().ok()) {
        c.port = v;
    }
    if let Some(v) = arg(args, "seed", "POTLUCK_SEED").and_then(|p| p.parse().ok()) {
        c.seed = v;
    }
    if let Some(v) = arg(args, "max-players", "POTLUCK_MAX_PLAYERS").and_then(|p| p.parse().ok()) {
        c.max_players = v;
    }
    if let Some(v) = arg(args, "cert", "POTLUCK_CERT") {
        c.cert = Some(v);
    }
    if let Some(v) = arg(args, "key", "POTLUCK_KEY") {
        c.key = Some(v);
    }
    if let Some(v) = arg(args, "tls-port", "POTLUCK_TLS_PORT").and_then(|p| p.parse().ok()) {
        c.tls_port = Some(v);
    }
    if args.iter().any(|a| a == "--no-beacon") {
        c.beacon = false;
    }
    c.check()?;
    Ok(c)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("potluck-server {}\n\n{HELP}", share::version());
        return;
    }
    if let Some(d) = args.iter().find_map(|a| a.strip_prefix("--data=")) {
        // set before any thread starts
        unsafe { std::env::set_var("POTLUCK_HOME", d) };
    }
    let _ = std::fs::create_dir_all(settings::data_dir());
    let cfg = match settings_from(&args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("potluck-server: {e}");
            std::process::exit(2);
        }
    };
    if args.iter().any(|a| a == "--check") {
        println!("The settings are fine: {} on port {}, up to {} players, {} pinned creations.", cfg.name, cfg.port, cfg.max_players, cfg.creations.len());
        return;
    }
    // the market: its key checks passes, and pinned creations come from it
    let link = share::MarketLink::new(&cfg.market, &cfg.market_token);
    let market_key = match link.key() {
        Ok(k) => Some(k),
        Err(e) => {
            eprintln!("potluck-server: couldn't reach the market at {} ({e}); players come in without passes", cfg.market);
            None
        }
    };
    let mut pinned = vec![];
    for c in &cfg.creations {
        let (code, version) = match c.split_once('@') {
            Some((code, v)) => (code, v.parse().ok()),
            None => (c.as_str(), None),
        };
        match link.load(code, version) {
            Ok(p) => {
                eprintln!("potluck-server: runs {} by {} ({c})", p.manifest.name, p.manifest.by);
                pinned.push(p);
            }
            Err(e) => eprintln!("potluck-server: couldn't load {c}: {e}"),
        }
    }
    // save before going on Ctrl-C, SIGTERM or SIGHUP
    if let Err(e) = ctrlc::set_handler(|| STOP.store(true, Ordering::SeqCst)) {
        eprintln!("Can't catch stop signals ({e}); the world saves every minute");
    }

    let mut app = App::new();
    app.add_plugins((MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_secs_f64(1.0 / 30.0))), bevy::log::LogPlugin::default()));
    sim(&mut app, cfg.seed);
    // a world kept from before
    if let Ok(bytes) = std::fs::read(world_file()) {
        if bytes.len() > 13 && &bytes[..9] == b"POTLUCKW1" {
            let seed = u32::from_le_bytes([bytes[9], bytes[10], bytes[11], bytes[12]]);
            match world::Voxels::decode(seed, &bytes[13..]) {
                Some(v) => {
                    info!("Loaded the world from {}", world_file().display());
                    app.insert_resource(v);
                }
                None => warn!("{} is damaged; starting a new world", world_file().display()),
            }
        }
    }
    let mut s = settings::Settings::default();
    s.name = cfg.name.clone();
    {
        let scripts = app.world().resource::<script::Scripts>();
        let mut shared = scripts.shared.lock().unwrap();
        shared.boards = hosting::Boards::load(&boards_file());
        shared.links = cfg.links.keys().cloned().collect();
    }
    let listing = cfg.list.then(|| (cfg.clone(), Arc::new(link)));
    app.insert_resource(s)
        .insert_resource(Opts { cfg, market_key, pinned })
        .init_resource::<Saving>()
        .add_systems(Startup, (open, bring_saved, bring_pinned).chain().after(start_marker))
        .add_systems(Update, (clock, log_toasts, autosave).after(Sim));
    app.add_systems(Startup, start_marker);
    if let Some((cfg, link)) = listing {
        app.insert_resource(Listing { cfg, link, since: 55.0, busy: Arc::new(AtomicBool::new(false)) }).add_systems(Update, list_on_market.after(Sim));
    }
    app.run();
}

fn start_marker() {}

fn open(opts: Res<Opts>, mut net: ResMut<net::Net>, mut exit: MessageWriter<AppExit>) {
    let c = &opts.cfg;
    let tls = match (&c.cert, &c.key) {
        (Some(cert), Some(key)) => Some((PathBuf::from(cert), PathBuf::from(key), c.tls_port.unwrap_or(443))),
        _ => None,
    };
    let mut rules = c.rules();
    rules.market_key = opts.market_key.clone();
    let o = net::HostOptions { name: c.name.clone(), port: c.port, tls: tls.clone(), beacon: c.beacon, rules };
    match net.host_with(o) {
        Ok(addr) => {
            info!("{} is open at ws://{addr} (and http://{addr} for its status page)", c.name);
            if let Some((_, _, p)) = &tls {
                info!("TLS on port {p}: players join with wss://<your domain>:{p} or https://<your domain>");
            }
        }
        Err(e) => {
            error!("Couldn't open the world: {e}");
            exit.write(AppExit::error());
        }
    }
}

/// Creations players brought last time come back.
fn bring_saved(mut lib: ResMut<Library>, mut scripts: ResMut<script::Scripts>, mut herd: ResMut<creatures::Herd>, mut net: ResMut<net::Net>, mut toasts: ResMut<ui::Toasts>) {
    let Ok(text) = std::fs::read_to_string(creations_file()) else { return };
    let Ok(list) = serde_json::from_str::<Vec<Payload>>(&text) else { return };
    for p in list {
        let by = p.manifest.by.clone();
        if let Ok(c) = Creation::from_payload(p, Origin::Friend(by)) {
            let id = lib.add(c);
            game::bring(&id, &mut lib, &mut scripts, &mut herd, &mut net, &mut toasts);
        }
    }
}

/// The creations server.toml pins, fetched from the market at start.
fn bring_pinned(opts: Res<Opts>, mut lib: ResMut<Library>, mut scripts: ResMut<script::Scripts>, mut herd: ResMut<creatures::Herd>, mut net: ResMut<net::Net>, mut toasts: ResMut<ui::Toasts>) {
    for p in &opts.pinned {
        let code = p.code.clone().unwrap_or_default();
        if let Ok(c) = Creation::from_payload(p.clone(), Origin::Code(code)) {
            let id = lib.add(c);
            game::bring(&id, &mut lib, &mut scripts, &mut herd, &mut net, &mut toasts);
        }
    }
}

/// The market's server list: a heartbeat every minute with who's here and
/// what runs, and boards that changed. Off the main thread, so a slow
/// market never holds up the world.
#[derive(Resource)]
struct Listing {
    cfg: ServerConfig,
    link: Arc<share::MarketLink>,
    since: f32,
    busy: Arc<AtomicBool>,
}

fn list_on_market(time: Res<Time>, mut listing: ResMut<Listing>, net: Res<net::Net>, lib: Res<Library>, scripts: Res<script::Scripts>) {
    listing.since += time.delta_secs();
    if listing.since < 60.0 || listing.busy.load(Ordering::SeqCst) {
        return;
    }
    listing.since = 0.0;
    let players: Vec<String> = net.remote_players().iter().map(|p| p.name.clone()).collect();
    let creations: Vec<serde_json::Value> = lib
        .running
        .iter()
        .filter_map(|id| lib.list.get(id))
        .filter(|c| c.origin != Origin::Base)
        .map(|c| serde_json::json!({ "name": c.manifest.name, "code": c.code }))
        .collect();
    let c = &listing.cfg;
    let body = serde_json::json!({
        "name": c.name, "about": c.about, "address": c.address, "players": players.len(), "max_players": c.max_players,
        "join": c.join, "creations": creations, "version": share::version(), "protocol": net::PROTOCOL, "all_ages": c.all_ages,
    });
    // boards that changed since the market last heard, by creation code
    let boards: Vec<serde_json::Value> = {
        let mut shared = scripts.shared.lock().unwrap();
        let mut out = vec![];
        for (cid, bs) in shared.boards.boards.iter_mut() {
            // by market code, or the base game's own by name
            let which = match lib.list.get(cid) {
                Some(c) if c.origin == Origin::Base => serde_json::json!({ "base": cid }),
                Some(c) => match &c.code {
                    Some(code) => serde_json::json!({ "creation": code }),
                    None => continue,
                },
                None => continue,
            };
            for (name, b) in bs.iter_mut().filter(|(_, b)| b.dirty) {
                b.dirty = false;
                let mut post = which.clone();
                post["board"] = serde_json::json!(name);
                post["lower"] = serde_json::json!(b.lower);
                post["scores"] = serde_json::json!(b.scores.iter().take(20).collect::<Vec<_>>());
                out.push(post);
            }
        }
        out
    };
    let link = listing.link.clone();
    let busy = listing.busy.clone();
    busy.store(true, Ordering::SeqCst);
    std::thread::spawn(move || {
        if let Err(e) = link.heartbeat(&body) {
            eprintln!("potluck-server: the server list didn't take this server ({e})");
        }
        if !boards.is_empty() {
            if let Err(e) = link.boards(&serde_json::json!({ "boards": boards })) {
                eprintln!("potluck-server: boards didn't reach the market ({e})");
            }
        }
        busy.store(false, Ordering::SeqCst);
    });
}

fn clock(time: Res<Time>, mut clock: ResMut<render::Clock>) {
    clock.time = (clock.time + time.delta_secs() / (20.0 * 60.0)) % 1.0;
}

fn log_toasts(mut toasts: ResMut<ui::Toasts>) {
    for (t, _) in toasts.0.drain(..) {
        info!("{t}");
    }
}

/// Set by Ctrl-C, or by systemd (or snapd) stopping the service.
static STOP: AtomicBool = AtomicBool::new(false);

fn autosave(time: Res<Time>, mut saving: ResMut<Saving>, voxels: Res<world::Voxels>, lib: Res<Library>, scripts: Res<script::Scripts>, mut exit: MessageWriter<AppExit>) {
    saving.since += time.delta_secs();
    let stopping = STOP.load(Ordering::SeqCst);
    if saving.since < SAVE_EVERY && !stopping {
        return;
    }
    saving.since = 0.0;
    save(&voxels, &lib);
    scripts.shared.lock().unwrap().boards.save(&boards_file());
    if stopping {
        info!("Saved the world; stopping");
        exit.write(AppExit::Success);
    }
}

fn save(voxels: &world::Voxels, lib: &Library) {
    let mut bytes = b"POTLUCKW1".to_vec();
    bytes.extend_from_slice(&voxels.seed.to_le_bytes());
    bytes.extend_from_slice(&voxels.encode());
    let tmp = world_file().with_extension("tmp");
    if std::fs::write(&tmp, &bytes).and_then(|_| std::fs::rename(&tmp, world_file())).is_err() {
        warn!("Couldn't save the world to {}", world_file().display());
    }
    // pinned creations come from the market again at the next start
    let brought: Vec<Payload> = lib.running.iter().filter_map(|id| lib.list.get(id)).filter(|c| !matches!(c.origin, Origin::Base | Origin::Code(_))).map(|c| c.payload()).collect();
    let _ = std::fs::write(creations_file(), serde_json::to_string(&brought).unwrap_or_default());
}
