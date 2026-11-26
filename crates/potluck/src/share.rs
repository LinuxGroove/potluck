//! Share codes through the LinuxGroove game server: a creation goes up as
//! one share of kind `creation` and comes back by its 8 character code.
//! Network calls run on their own thread and report back by channel.

use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Mutex;
use std::time::Duration;

use bevy::prelude::*;

use crate::creations::Payload;

pub const DEFAULT_SERVER: &str = "https://play.linuxgroove.com";
/// The game server's key; CI puts the real one in release builds.
const SERVER_KEY: &str = match option_env!("POTLUCK_SERVER_KEY") {
    Some(k) => k,
    None => "defaultkey",
};
pub const GAME_ID: &str = "potluck";

pub fn version() -> &'static str {
    option_env!("POTLUCK_VERSION").unwrap_or(concat!("0.0.0+", env!("CARGO_PKG_VERSION")))
}

pub enum Done {
    Shared { id: String, code: String },
    Loaded { code: String, payload: Payload },
    Failed(String),
}

#[derive(Resource)]
pub struct ShareJobs {
    tx: Sender<Done>,
    rx: Mutex<Receiver<Done>>,
    token: std::sync::Arc<Mutex<Option<String>>>,
}

impl Default for ShareJobs {
    fn default() -> Self {
        let (tx, rx) = channel();
        Self { tx, rx: Mutex::new(rx), token: Default::default() }
    }
}

/// Codes ignore case, dashes, spaces and the look-alike letters.
pub fn clean_code(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

fn b64(s: &str) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let b = s.as_bytes();
    let mut out = String::new();
    for chunk in b.chunks(3) {
        let n = (chunk[0] as u32) << 16 | (*chunk.get(1).unwrap_or(&0) as u32) << 8 | *chunk.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn platform() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    }
}

struct Client {
    agent: ureq::Agent,
    base: String,
}

impl Client {
    fn new(server: &str) -> Self {
        let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(12))).http_status_as_error(false).build().into();
        Self { agent, base: server.trim_end_matches('/').to_string() }
    }

    fn post(&self, path: &str, auth: &str, body: &str) -> Result<serde_json::Value, String> {
        let mut r = self
            .agent
            .post(format!("{}{path}", self.base))
            .header("Authorization", auth)
            .header("Content-Type", "application/json")
            .send(body)
            .map_err(|e| format!("couldn't reach the game server ({e})"))?;
        let status = r.status().as_u16();
        let text = r.body_mut().read_to_string().map_err(|e| e.to_string())?;
        let v: serde_json::Value = serde_json::from_str(&text).unwrap_or(serde_json::Value::String(text.clone()));
        if status >= 400 {
            let msg = v.get("message").and_then(|m| m.as_str()).unwrap_or(&text).to_string();
            return Err(explain(&msg));
        }
        Ok(v)
    }

    fn sign_in(&self, device: &str) -> Result<String, String> {
        let body = serde_json::json!({ "id": device, "vars": { "game": GAME_ID, "version": version(), "platform": platform() } });
        let v = self.post("/v2/account/authenticate/device?create=true", &format!("Basic {}", b64(&format!("{SERVER_KEY}:"))), &body.to_string())?;
        v.get("token").and_then(|t| t.as_str()).map(|s| s.to_string()).ok_or_else(|| "the game server didn't sign us in".into())
    }

    fn rpc(&self, token: &str, name: &str, payload: serde_json::Value) -> Result<serde_json::Value, String> {
        self.post(&format!("/v2/rpc/{name}?unwrap"), &format!("Bearer {token}"), &payload.to_string())
    }
}

fn explain(msg: &str) -> String {
    let m = msg.to_lowercase();
    if m.contains("unknown_game") || m.contains("game_disabled") || m.contains("unknown_kind") {
        "the game server doesn't know Potluck yet, so codes can't be made there. Friends' worlds still bring creations along".into()
    } else if m.contains("share_not_found") {
        "no creation has that code".into()
    } else if m.contains("share_hidden") {
        "that creation was hidden after reports".into()
    } else if m.contains("share_limit") {
        "you've shared as many creations as the server allows; delete some first".into()
    } else if m.contains("update_required") {
        "this version of Potluck is too old for the game server".into()
    } else {
        format!("the game server said: {msg}")
    }
}

impl ShareJobs {
    fn run(&self, server: &str, device: &str, job: impl FnOnce(&Client, &str) -> Done + Send + 'static) {
        let tx = self.tx.clone();
        let server = server.to_string();
        let device = device.to_string();
        let token = self.token.clone();
        std::thread::spawn(move || {
            let client = Client::new(&server);
            let have = token.lock().unwrap().clone();
            let t = match have {
                Some(t) => t,
                None => match client.sign_in(&device) {
                    Ok(t) => {
                        *token.lock().unwrap() = Some(t.clone());
                        t
                    }
                    Err(e) => {
                        let _ = tx.send(Done::Failed(e));
                        return;
                    }
                },
            };
            let _ = tx.send(job(&client, &t));
        });
    }

    pub fn share(&self, server: &str, device: &str, payload: Payload) {
        self.run(server, device, move |c, t| {
            let data = serde_json::to_string(&payload).unwrap_or_default();
            let req = serde_json::json!({
                "kind": "creation",
                "title": payload.manifest.name,
                "data": data,
                "meta": { "by": payload.manifest.by, "kind": payload.manifest.kind, "id": payload.id },
            });
            match c.rpc(t, "core.share_create", req) {
                Ok(v) => match v.get("code").and_then(|c| c.as_str()) {
                    Some(code) => Done::Shared { id: payload.id.clone(), code: code.to_string() },
                    None => Done::Failed("the game server gave no code back".into()),
                },
                Err(e) => Done::Failed(e),
            }
        });
    }

    pub fn load(&self, server: &str, device: &str, code: &str) {
        let code = clean_code(code);
        self.run(server, device, move |c, t| match c.rpc(t, "core.share_get", serde_json::json!({ "code": code })) {
            Ok(v) => {
                let data = v.get("data").and_then(|d| d.as_str()).unwrap_or("");
                match serde_json::from_str::<Payload>(data) {
                    Ok(p) => Done::Loaded { code: code.clone(), payload: p },
                    Err(_) => Done::Failed("that code isn't a Potluck creation".into()),
                }
            }
            Err(e) => Done::Failed(e),
        });
    }

    pub fn poll(&self) -> Vec<Done> {
        self.rx.lock().unwrap().try_iter().collect()
    }
}

// ---------- the launch ping ----------

/// Whether DO_NOT_TRACK says no: anything but empty, 0 or false.
pub fn opted_out(do_not_track: &str) -> bool {
    let v = do_not_track.trim().to_lowercase();
    !v.is_empty() && v != "0" && v != "false"
}

/// What a ping says: the game, a random install id, the version, OS and CPU.
pub fn ping_body(install: &str) -> serde_json::Value {
    let os = match std::env::consts::OS {
        "linux" => "Linux",
        "windows" => "Windows",
        "macos" => "macOS",
        o => o,
    };
    let arch = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        a => a,
    };
    let distro = std::fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|t| t.lines().find_map(|l| l.strip_prefix("NAME=").map(|v| v.trim_matches('"').chars().take(128).collect::<String>())))
        .unwrap_or_default();
    serde_json::json!({ "game": GAME_ID, "install": install, "version": version(), "os": os, "distro": distro, "os_version": "", "arch": arch })
}

/// This install's id: 32 random hex digits, made on the first run and kept.
fn install_id() -> String {
    let path = crate::settings::data_dir().join("install_id");
    let saved = std::fs::read_to_string(&path).unwrap_or_default().trim().to_string();
    if saved.len() == 32 && saved.chars().all(|c| c.is_ascii_hexdigit()) {
        return saved;
    }
    let made: String = crate::settings::random_hex(32).chars().take(32).collect();
    let _ = std::fs::write(&path, &made);
    made
}

/// One anonymous request when the game starts, so the game server can count
/// players of every game, online or not (game-server docs/game-api.md,
/// "Launch pings"). It never signs in, never waits and never retries. Builds
/// from source (no POTLUCK_VERSION) and DO_NOT_TRACK skip it.
pub fn launch_ping(server: &str) {
    if option_env!("POTLUCK_VERSION").is_none() || opted_out(&std::env::var("DO_NOT_TRACK").unwrap_or_default()) {
        return;
    }
    let url = format!("{}/launch", server.trim_end_matches('/'));
    let body = ping_body(&install_id());
    std::thread::spawn(move || {
        let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(5))).build().into();
        let _ = agent.post(&url).header("Content-Type", "application/json").send(body.to_string());
    });
}
