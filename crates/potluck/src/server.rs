//! potluck-server: a Potluck world that runs with nobody at the keyboard,
//! for friends to join at any time. It serves plain WebSockets on one port
//! and, given a certificate, TLS on another, so it can live at an https
//! address. A web browser pointed at either port sees who's playing.
//!
//!     potluck-server [--name="Riverbend"] [--port=47126] [--seed=26]
//!                    [--tls-port=443 --cert=fullchain.pem --key=privkey.pem]
//!                    [--data=/var/lib/potluck] [--no-beacon]
//!
//! Every option can also come from the environment as POTLUCK_NAME,
//! POTLUCK_PORT, POTLUCK_TLS_PORT, POTLUCK_CERT, POTLUCK_KEY, POTLUCK_SEED
//! and POTLUCK_HOME (where the world and creations are kept).

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use bevy::app::ScheduleRunnerPlugin;
use bevy::prelude::*;

use potluck::creations::{Creation, Library, Origin, Payload};
use potluck::*;

const SAVE_EVERY: f32 = 60.0;

#[derive(Resource)]
struct Opts {
    name: String,
    port: u16,
    tls: Option<(PathBuf, PathBuf, u16)>,
    beacon: bool,
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

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("potluck-server {}\n\nOptions (or the POTLUCK_* environment variables):\n  --name=NAME          what players see (POTLUCK_NAME)\n  --port=47126         plain WebSocket and status page (POTLUCK_PORT)\n  --tls-port=443       TLS: wss:// and https:// (POTLUCK_TLS_PORT)\n  --cert=FILE          TLS certificate chain, PEM (POTLUCK_CERT)\n  --key=FILE           TLS private key, PEM (POTLUCK_KEY)\n  --seed=26            the world's seed for a new world (POTLUCK_SEED)\n  --data=DIR           where the world is kept (POTLUCK_HOME)\n  --no-beacon          don't announce the world on the local network", share::version());
        return;
    }
    if let Some(d) = args.iter().find_map(|a| a.strip_prefix("--data=")) {
        // set before any thread starts
        unsafe { std::env::set_var("POTLUCK_HOME", d) };
    }
    let _ = std::fs::create_dir_all(settings::data_dir());
    let name = arg(&args, "name", "POTLUCK_NAME").unwrap_or_else(|| "Potluck server".into());
    let port: u16 = arg(&args, "port", "POTLUCK_PORT").and_then(|p| p.parse().ok()).unwrap_or(net::PORT);
    let seed: u32 = arg(&args, "seed", "POTLUCK_SEED").and_then(|p| p.parse().ok()).unwrap_or(SEED);
    let tls = match (arg(&args, "cert", "POTLUCK_CERT"), arg(&args, "key", "POTLUCK_KEY")) {
        (Some(c), Some(k)) => Some((PathBuf::from(c), PathBuf::from(k), arg(&args, "tls-port", "POTLUCK_TLS_PORT").and_then(|p| p.parse().ok()).unwrap_or(443))),
        (None, None) => None,
        _ => {
            eprintln!("TLS needs both --cert and --key");
            std::process::exit(2);
        }
    };
    let beacon = !args.iter().any(|a| a == "--no-beacon");
    // save before going on Ctrl-C, SIGTERM or SIGHUP
    if let Err(e) = ctrlc::set_handler(|| STOP.store(true, Ordering::SeqCst)) {
        eprintln!("Can't catch stop signals ({e}); the world saves every minute");
    }

    let mut app = App::new();
    app.add_plugins((MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_secs_f64(1.0 / 30.0))), bevy::log::LogPlugin::default()));
    sim(&mut app, seed);
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
    s.name = name.clone();
    app.insert_resource(s)
        .insert_resource(Opts { name, port, tls, beacon })
        .init_resource::<Saving>()
        .add_systems(Startup, (open, bring_saved).chain().after(start_marker))
        .add_systems(Update, (clock, log_toasts, autosave).after(Sim));
    app.add_systems(Startup, start_marker);
    app.run();
}

fn start_marker() {}

fn open(opts: Res<Opts>, mut net: ResMut<net::Net>, mut exit: MessageWriter<AppExit>) {
    let o = net::HostOptions { name: opts.name.clone(), port: opts.port, tls: opts.tls.clone(), beacon: opts.beacon };
    match net.host_with(o) {
        Ok(addr) => {
            info!("{} is open at ws://{addr} (and http://{addr} for its status page)", opts.name);
            if let Some((_, _, p)) = &opts.tls {
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

fn autosave(time: Res<Time>, mut saving: ResMut<Saving>, voxels: Res<world::Voxels>, lib: Res<Library>, mut exit: MessageWriter<AppExit>) {
    saving.since += time.delta_secs();
    let stopping = STOP.load(Ordering::SeqCst);
    if saving.since < SAVE_EVERY && !stopping {
        return;
    }
    saving.since = 0.0;
    save(&voxels, &lib);
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
    let brought: Vec<Payload> = lib.running.iter().filter_map(|id| lib.list.get(id)).filter(|c| c.origin != Origin::Base).map(|c| c.payload()).collect();
    let _ = std::fs::write(creations_file(), serde_json::to_string(&brought).unwrap_or_default());
}
