//! Sharing creations. The market (potluck-share, in crates/share-service)
//! keeps them, lists the ones shared to Everyone, and carries remixes and
//! suggested changes; the LinuxGroove game server signs the player in (with
//! no email or password: a random id for this device) and still loads the
//! codes made before the market. Network calls run on their own thread and
//! report back by channel.

use std::collections::BTreeMap;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::creations::{FromRef, Manifest, Payload};

pub const DEFAULT_SERVER: &str = "https://play.linuxgroove.com";
pub const DEFAULT_MARKET: &str = "https://potluck.linuxgroove.com";
/// The game server's key; CI puts the real one in release builds.
const SERVER_KEY: &str = match option_env!("POTLUCK_SERVER_KEY") {
    Some(k) => k,
    None => "defaultkey",
};
/// The market's public signing key (its /api/key). Release builds carry it,
/// so a code only ever brings what the market signed. Without it the first
/// key a market shows is kept, and a different one later is refused.
const MARKET_KEY: Option<&str> = option_env!("POTLUCK_MARKET_KEY");
pub const GAME_ID: &str = "potluck";

pub fn version() -> &'static str {
    option_env!("POTLUCK_VERSION").unwrap_or(concat!("0.0.0+", env!("CARGO_PKG_VERSION")))
}

/// A change someone suggested to one of the player's creations, from their remix.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Suggestion {
    pub id: i64,
    pub title: String,
    #[serde(default)]
    pub by: String,
    #[serde(default)]
    pub from_code: String,
    #[serde(default)]
    pub from_name: String,
    #[serde(default)]
    pub from_version: u32,
}

pub enum Done {
    /// Shared: on the market (`version` 1 or more), or as a game server code (0).
    Shared { id: String, code: String, version: u32, everyone: bool, message: String },
    Loaded { code: String, version: u32, payload: Payload },
    /// A one-time sign-in link to the market, to show as a QR code.
    Link { url: String, title: String },
    Suggested { id: String, code: String },
    Suggestions { id: String, list: Vec<Suggestion> },
    /// A suggestion merged on the market: the creation's new version.
    Merged { id: String, suggestion: i64, code: String, version: u32, files: BTreeMap<String, String> },
    /// A pass for joining a server, or None when the market couldn't give one.
    Pass { address: String, pass: Option<String> },
    /// The market's server list.
    Servers(Vec<Listed>),
    /// Who this device plays as on the market.
    Account(Account),
    /// A code for another device to type, to play as this account too.
    LinkCode(String),
    /// This device plays as another device's account now; `recovery` when
    /// a recovery code did it (it's used up).
    Linked { name: String, handle: String, recovery: bool },
    /// This device left the account it was linked to.
    Unlinked,
    /// What the market offers the Green's arches, and the jam running now.
    Green { picks: Vec<Pick>, jam: Option<JamNow> },
    /// A creation went into a jam.
    Entered { theme: String },
    Said(String),
    Failed(String),
}

/// A creation the market offers the Green's arches, and why: a favourite
/// of a jam that ended lately, an entry in this week's, or trending.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct Pick {
    pub code: String,
    pub name: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub by: String,
    #[serde(default)]
    pub why: String,
}

/// The jam running now: a theme for a week.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct JamNow {
    pub slug: String,
    pub theme: String,
    #[serde(default)]
    pub ends: i64,
}

/// The market account this device plays as.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct Account {
    pub handle: String,
    pub name: String,
    /// Linked from another device, rather than started here.
    #[serde(default)]
    pub linked: bool,
}

/// What this device is, for the account's list of devices: never its name
/// or anything about who uses it.
pub fn device_label() -> &'static str {
    if std::env::var("SteamDeck").is_ok_and(|v| v == "1") {
        "Steam Deck"
    } else if cfg!(target_os = "windows") {
        "Windows"
    } else if cfg!(target_os = "macos") {
        "Mac"
    } else {
        "Linux"
    }
}

/// A server in the market's list.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct Listed {
    pub name: String,
    #[serde(default)]
    pub about: String,
    pub address: String,
    #[serde(default)]
    pub players: u32,
    #[serde(default)]
    pub max_players: u32,
    #[serde(default)]
    pub creations: Vec<String>,
    #[serde(default)]
    pub join: String,
    /// Open to players of every age, under 13 too.
    #[serde(default)]
    pub all_ages: bool,
}

/// Where to and as whom: the game server, the market, this device's id and
/// the player's name.
#[derive(Clone, Debug)]
pub struct Who {
    pub server: String,
    pub market: String,
    pub device: String,
    pub name: String,
    /// Under 13, as the player (or a parent) said: the market keeps them to
    /// sharing by code, and their passes say so.
    pub young: bool,
}

impl Who {
    pub fn of(s: &crate::settings::Settings) -> Who {
        Who { server: s.server.clone(), market: s.market.clone(), device: s.device.clone(), name: s.name.clone(), young: s.young }
    }
}

#[derive(Resource)]
pub struct ShareJobs {
    tx: Sender<Done>,
    rx: Mutex<Receiver<Done>>,
    token: Arc<Mutex<Option<String>>>,
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

fn platform() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    }
}

fn agent(secs: u64) -> ureq::Agent {
    ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(secs))).http_status_as_error(false).build().into()
}

/// The game server: signing in, and the codes made before the market.
struct Server {
    agent: ureq::Agent,
    base: String,
}

impl Server {
    fn post(&self, path: &str, auth: &str, body: &str) -> Result<Value, String> {
        let mut r = self
            .agent
            .post(format!("{}{path}", self.base))
            .header("Authorization", auth)
            .header("Content-Type", "application/json")
            .send(body)
            .map_err(|e| format!("couldn't reach the game server ({e})"))?;
        let status = r.status().as_u16();
        let text = r.body_mut().read_to_string().map_err(|e| e.to_string())?;
        let v: Value = serde_json::from_str(&text).unwrap_or(Value::String(text.clone()));
        if status >= 400 {
            let msg = v.get("message").and_then(|m| m.as_str()).unwrap_or(&text).to_string();
            return Err(explain(&msg));
        }
        Ok(v)
    }

    fn sign_in(&self, device: &str) -> Result<String, String> {
        let body = json!({ "id": device, "vars": { "game": GAME_ID, "version": version(), "platform": platform() } });
        let basic = base64::engine::general_purpose::STANDARD.encode(format!("{SERVER_KEY}:"));
        let v = self.post("/v2/account/authenticate/device?create=true", &format!("Basic {basic}"), &body.to_string())?;
        v.get("token").and_then(|t| t.as_str()).map(|s| s.to_string()).ok_or_else(|| "the game server didn't sign us in".into())
    }

    fn rpc(&self, token: &str, name: &str, payload: Value) -> Result<Value, String> {
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

/// Why a market call didn't work: it couldn't be reached, or it said no.
#[derive(Debug)]
enum Fail {
    Offline(String),
    Said(u16, String),
}

impl Fail {
    fn text(self) -> String {
        match self {
            Fail::Offline(e) => format!("couldn't reach the market ({e})"),
            Fail::Said(_, m) => m,
        }
    }
}

/// The market's API (crates/share-service/src/api.rs).
struct Market {
    agent: ureq::Agent,
    base: String,
    /// Says the player is under 13 on every signed-in call.
    young: bool,
}

impl Market {
    fn call(&self, method: &str, path: &str, token: Option<&str>, name: &str, body: Option<&Value>) -> Result<Value, Fail> {
        let url = format!("{}{path}", self.base);
        let res = if method == "GET" {
            let mut r = self.agent.get(&url).header("X-Potluck-Young", if self.young { "1" } else { "0" });
            if let Some(t) = token {
                r = r.header("Authorization", &format!("Bearer {t}")).header("X-Potluck-Name", name);
            }
            r.call()
        } else {
            let mut r = self.agent.post(&url).header("Content-Type", "application/json");
            if let Some(t) = token {
                r = r.header("Authorization", &format!("Bearer {t}")).header("X-Potluck-Name", name).header("X-Potluck-Young", if self.young { "1" } else { "0" });
            }
            r.send(body.map(|b| b.to_string()).unwrap_or_else(|| "{}".into()))
        };
        let mut r = res.map_err(|e| Fail::Offline(e.to_string()))?;
        let status = r.status().as_u16();
        let text = r.body_mut().read_to_string().map_err(|e| Fail::Offline(e.to_string()))?;
        // a proxy in front with the market behind it down
        if matches!(status, 502..=504) {
            return Err(Fail::Offline(format!("it answered {status}")));
        }
        let v: Value = serde_json::from_str(&text).map_err(|_| Fail::Offline("that address isn't a Potluck market".into()))?;
        if status >= 400 {
            return Err(Fail::Said(status, v.get("error").and_then(|e| e.as_str()).unwrap_or("the market said no").to_string()));
        }
        Ok(v)
    }

    /// A call as a server, with the token from the market's Your servers page.
    fn as_server(&self, path: &str, token: &str, body: &Value) -> Result<Value, Fail> {
        let mut r = self
            .agent
            .post(format!("{}{path}", self.base))
            .header("Content-Type", "application/json")
            .header("Authorization", &format!("Server {token}"))
            .send(body.to_string())
            .map_err(|e| Fail::Offline(e.to_string()))?;
        let status = r.status().as_u16();
        let text = r.body_mut().read_to_string().map_err(|e| Fail::Offline(e.to_string()))?;
        let v: Value = serde_json::from_str(&text).map_err(|_| Fail::Offline(format!("it answered {status} with something else")))?;
        if status >= 400 {
            return Err(Fail::Said(status, v.get("error").and_then(|e| e.as_str()).unwrap_or("the market said no").to_string()));
        }
        Ok(v)
    }

    fn file(&self, sha: &str) -> Result<Vec<u8>, Fail> {
        let mut r = self.agent.get(format!("{}/f/{sha}", self.base)).call().map_err(|e| Fail::Offline(e.to_string()))?;
        if r.status().as_u16() != 200 {
            return Err(Fail::Said(r.status().as_u16(), "a file of that creation is missing".into()));
        }
        r.body_mut().with_config().limit(crate::creations::MAX_SIZE as u64).read_to_vec().map_err(|e| Fail::Offline(e.to_string()))
    }
}

/// One job's connections and sign-in.
struct Ctx {
    server: Server,
    market: Market,
    who: Who,
    token: Arc<Mutex<Option<String>>>,
}

impl Ctx {
    fn token(&self, fresh: bool) -> Result<String, String> {
        if !fresh {
            if let Some(t) = self.token.lock().unwrap().clone() {
                return Ok(t);
            }
        }
        let t = self.server.sign_in(&self.who.device)?;
        *self.token.lock().unwrap() = Some(t.clone());
        Ok(t)
    }

    /// A market call as this player, signing in again once if the market
    /// says the session ran out.
    fn market(&self, method: &str, path: &str, body: Option<Value>) -> Result<Value, Fail> {
        let t = self.token(false).map_err(Fail::Offline)?;
        match self.market.call(method, path, Some(&t), &self.who.name, body.as_ref()) {
            Err(Fail::Said(401, _)) => {
                let t = self.token(true).map_err(Fail::Offline)?;
                self.market.call(method, path, Some(&t), &self.who.name, body.as_ref())
            }
            r => r,
        }
    }

    /// Signed in when the game server answers, and anonymous when it doesn't:
    /// anyone may load what's on the market.
    fn market_get(&self, path: &str) -> Result<Value, Fail> {
        match self.market("GET", path, None) {
            Err(Fail::Offline(_)) => self.market.call("GET", path, None, "", None),
            r => r,
        }
    }

    /// A creation from the market: its manifest must carry the market's
    /// signature, and every file must match the hash the manifest gives.
    fn load(&self, code: &str, version: Option<u32>) -> Result<(u32, Payload), Fail> {
        let path = match version {
            Some(n) => format!("/api/c/{code}?v={n}"),
            None => format!("/api/c/{code}"),
        };
        let r = self.market_get(&path)?;
        let text = r["manifest"].as_str().unwrap_or("");
        let key = trusted_key(&self.who.market, r["key"].as_str().unwrap_or("")).map_err(|e| Fail::Said(0, e))?;
        if !verify(&key, text, r["signature"].as_str().unwrap_or("")) {
            return Err(Fail::Said(0, "that creation isn't signed by the market, so it stays out".into()));
        }
        let m: MarketManifest = serde_json::from_str(text).map_err(|_| Fail::Said(0, "the market sent something that isn't a creation".into()))?;
        if m.code != code {
            return Err(Fail::Said(0, "the market sent a different creation".into()));
        }
        let mut files = BTreeMap::new();
        let mut total = 0;
        for (path, f) in &m.files {
            total += f.size;
            if total > crate::creations::MAX_SIZE || m.files.len() > 64 {
                return Err(Fail::Said(0, "that creation is too big".into()));
            }
            let bytes = cached_file(&f.sha256).map(Ok).unwrap_or_else(|| self.market.file(&f.sha256))?;
            if sha256_hex(&bytes) != f.sha256 {
                return Err(Fail::Said(0, format!("{path} isn't what the market signed, so it stays out")));
            }
            cache_file(&f.sha256, &bytes);
            files.insert(path.clone(), String::from_utf8_lossy(&bytes).to_string());
        }
        let manifest: Manifest = files.get("potluck.toml").and_then(|t| toml::from_str(t).ok()).unwrap_or_default();
        let id = crate::creations::slug(&manifest.name);
        Ok((m.version, Payload { format: 1, id, manifest, files, knobs: BTreeMap::new(), code: Some(code.to_string()) }))
    }
}

#[derive(Deserialize)]
struct MarketManifest {
    code: String,
    version: u32,
    files: BTreeMap<String, FileRef>,
}

#[derive(Deserialize)]
struct FileRef {
    sha256: String,
    size: usize,
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// Checks a manifest's Ed25519 signature against the market's public key.
pub fn verify(public: &str, text: &str, signature: &str) -> bool {
    let b64 = base64::engine::general_purpose::STANDARD;
    let (Ok(k), Ok(sig)) = (b64.decode(public), b64.decode(signature)) else { return false };
    let (Ok(k), Ok(sig)) = (<[u8; 32]>::try_from(k.as_slice()), <[u8; 64]>::try_from(sig.as_slice())) else { return false };
    let Ok(k) = ed25519_dalek::VerifyingKey::from_bytes(&k) else { return false };
    k.verify_strict(text.as_bytes(), &ed25519_dalek::Signature::from_bytes(&sig)).is_ok()
}

/// The key to check a market's signatures with: the one built in, or the
/// one this market showed the first time, kept in market-keys.json.
fn trusted_key(market: &str, shown: &str) -> Result<String, String> {
    if let Some(k) = MARKET_KEY.filter(|_| market.trim_end_matches('/') == DEFAULT_MARKET) {
        return Ok(k.to_string());
    }
    let path = crate::settings::data_dir().join("market-keys.json");
    let mut keys: BTreeMap<String, String> = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    match keys.get(market) {
        Some(k) if k == shown => Ok(k.clone()),
        Some(_) => Err("the market's signing key changed, so nothing from it loads. If that's expected, delete market-keys.json in Potluck's folder".into()),
        None if shown.len() == 44 => {
            keys.insert(market.to_string(), shown.to_string());
            let _ = std::fs::create_dir_all(crate::settings::data_dir());
            let _ = std::fs::write(&path, serde_json::to_string_pretty(&keys).unwrap_or_default());
            Ok(shown.to_string())
        }
        None => Err("the market didn't say how to check its signatures".into()),
    }
}

fn cache_dir() -> std::path::PathBuf {
    crate::settings::data_dir().join("cache").join("files")
}

fn cached_file(sha: &str) -> Option<Vec<u8>> {
    if sha.len() != 64 || !sha.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    std::fs::read(cache_dir().join(sha)).ok()
}

fn cache_file(sha: &str, bytes: &[u8]) {
    let _ = std::fs::create_dir_all(cache_dir());
    let _ = std::fs::write(cache_dir().join(sha), bytes);
}

/// The market says things in lower case for the game to put in a sentence.
fn sentence(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// A dedicated server's line to the market: its signing key (to check
/// players' passes), the creations the server pins, the server list and
/// boards. Every call blocks, so call from a thread or before starting.
pub struct MarketLink {
    base: String,
    token: String,
    ctx: Ctx,
}

impl MarketLink {
    pub fn new(market: &str, token: &str) -> MarketLink {
        let base = market.trim_end_matches('/').to_string();
        let who = Who { server: String::new(), market: base.clone(), device: String::new(), name: String::new(), young: false };
        let ctx = Ctx {
            // no game server: a server loads what anyone may load
            server: Server { agent: agent(1), base: "http://0.0.0.0:9".into() },
            market: Market { agent: agent(20), base: base.clone(), young: false },
            who,
            token: Default::default(),
        };
        MarketLink { base, token: token.to_string(), ctx }
    }

    /// The market's public key, checked as the game checks it.
    pub fn key(&self) -> Result<String, String> {
        let v = self.ctx.market.call("GET", "/api/key", None, "", None).map_err(Fail::text)?;
        trusted_key(&self.base, v["key"].as_str().unwrap_or(""))
    }

    /// A creation by its code, signed and checked.
    pub fn load(&self, code: &str, version: Option<u32>) -> Result<Payload, String> {
        let code = clean_code(code);
        self.ctx.load(&code, version).map(|(_, p)| p).map_err(Fail::text)
    }

    /// Tells the market this server is up, for its server list.
    pub fn heartbeat(&self, body: &Value) -> Result<Value, String> {
        self.ctx.market.as_server("/api/servers/heartbeat", &self.token, body).map_err(Fail::text)
    }

    /// Sends boards that changed.
    pub fn boards(&self, body: &Value) -> Result<(), String> {
        self.ctx.market.as_server("/api/servers/boards", &self.token, body).map(|_| ()).map_err(Fail::text)
    }
}

impl ShareJobs {
    fn run(&self, who: &Who, job: impl FnOnce(&Ctx) -> Done + Send + 'static) {
        let tx = self.tx.clone();
        let ctx = Ctx {
            server: Server { agent: agent(12), base: who.server.trim_end_matches('/').to_string() },
            market: Market { agent: agent(30), base: who.market.trim_end_matches('/').to_string(), young: who.young },
            who: who.clone(),
            token: self.token.clone(),
        };
        std::thread::spawn(move || {
            let _ = tx.send(job(&ctx));
        });
    }

    /// Shares a creation: to the market, for Everyone or by its code. The
    /// market keeps one code per creation, so `code` (the one it had) makes
    /// this the next version. With the market out of reach, sharing by code
    /// still works through the game server.
    pub fn share(&self, who: &Who, payload: Payload, everyone: bool, code: Option<String>, from: Option<FromRef>) {
        self.run(who, move |c| {
            let req = json!({ "code": code, "files": payload.files, "visibility": if everyone { "everyone" } else { "code" }, "from": from });
            match c.market("POST", "/api/share", Some(req)) {
                Ok(v) => Done::Shared {
                    id: payload.id.clone(),
                    code: v["code"].as_str().unwrap_or("").to_string(),
                    version: v["version"].as_u64().unwrap_or(0) as u32,
                    everyone,
                    message: sentence(v["message"].as_str().unwrap_or("")),
                },
                Err(Fail::Offline(e)) if !everyone => {
                    let t = match c.token(false) {
                        Ok(t) => t,
                        Err(e) => return Done::Failed(e),
                    };
                    let data = serde_json::to_string(&payload).unwrap_or_default();
                    let req = json!({ "kind": "creation", "title": payload.manifest.name, "data": data, "meta": { "by": payload.manifest.by, "kind": payload.manifest.kind, "id": payload.id } });
                    match c.server.rpc(&t, "core.share_create", req) {
                        Ok(v) => match v.get("code").and_then(|c| c.as_str()) {
                            Some(code) => Done::Shared {
                                id: payload.id.clone(),
                                code: code.to_string(),
                                version: 0,
                                everyone: false,
                                message: format!("The market isn't answering ({e}), so the game server keeps it by its code for now."),
                            },
                            None => Done::Failed("the game server gave no code back".into()),
                        },
                        Err(e) => Done::Failed(e),
                    }
                }
                Err(e) => Done::Failed(e.text()),
            }
        });
    }

    /// Loads a code: from the market, or from the game server for the codes
    /// made before it. `version` picks one version of it.
    pub fn load(&self, who: &Who, code: &str, version: Option<u32>) {
        let code = clean_code(code);
        self.run(who, move |c| match c.load(&code, version) {
            Ok((version, payload)) => Done::Loaded { code: code.clone(), version, payload },
            Err(Fail::Said(s, m)) if s != 404 => Done::Failed(m),
            Err(first) => {
                let t = match c.token(false) {
                    Ok(t) => t,
                    Err(e) => return Done::Failed(e),
                };
                match c.server.rpc(&t, "core.share_get", json!({ "code": code })) {
                    Ok(v) => {
                        let data = v.get("data").and_then(|d| d.as_str()).unwrap_or("");
                        match serde_json::from_str::<Payload>(data) {
                            Ok(p) => Done::Loaded { code: code.clone(), version: 0, payload: p },
                            Err(_) => Done::Failed("that code isn't a Potluck creation".into()),
                        }
                    }
                    Err(e) if e == "no creation has that code" => Done::Failed(match first {
                        Fail::Offline(m) => format!("couldn't reach the market ({m}), and the game server doesn't know that code"),
                        Fail::Said(_, m) => m,
                    }),
                    Err(e) => Done::Failed(e),
                }
            }
        });
    }

    /// A one-time link that signs the market in a browser in as this player
    /// and opens `next` there: the game shows it as a QR code for a phone.
    pub fn link(&self, who: &Who, next: &str, title: &str) {
        let next = next.to_string();
        let title = title.to_string();
        self.run(who, move |c| match c.market("POST", "/api/signin", Some(json!({ "next": next }))) {
            Ok(v) => Done::Link { url: v["url"].as_str().unwrap_or("").to_string(), title },
            Err(e) => Done::Failed(e.text()),
        });
    }

    /// Suggests a remix's change to the creation it was remixed from: shares
    /// it first if it changed, then opens the suggestion.
    pub fn suggest(&self, who: &Who, payload: Payload, everyone: bool, code: Option<String>, from: Option<FromRef>, title: String) {
        self.run(who, move |c| {
            let req = json!({ "code": code, "files": payload.files, "visibility": if everyone { "everyone" } else { "code" }, "from": from });
            let code = match c.market("POST", "/api/share", Some(req)) {
                Ok(v) => v["code"].as_str().unwrap_or("").to_string(),
                // already shared just as it is
                Err(Fail::Said(409, _)) if code.is_some() => code.clone().unwrap_or_default(),
                Err(e) => return Done::Failed(e.text()),
            };
            match c.market("POST", &format!("/api/c/{code}/suggest"), Some(json!({ "title": title }))) {
                Ok(_) => Done::Suggested { id: payload.id.clone(), code },
                Err(e) => Done::Failed(e.text()),
            }
        });
    }

    pub fn suggestions(&self, who: &Who, id: &str, code: &str) {
        let (id, code) = (id.to_string(), code.to_string());
        self.run(who, move |c| match c.market_get(&format!("/api/c/{code}/suggestions")) {
            Ok(v) => Done::Suggestions { id, list: serde_json::from_value(v["suggestions"].clone()).unwrap_or_default() },
            Err(e) => Done::Failed(e.text()),
        });
    }

    /// Merges a suggestion into the player's creation on the market, then
    /// brings back the new version to put in its folder.
    pub fn merge(&self, who: &Who, id: &str, suggestion: i64) {
        let id = id.to_string();
        self.run(who, move |c| {
            let v = match c.market("POST", &format!("/api/suggestions/{suggestion}/merge"), None) {
                Ok(v) => v,
                Err(e) => return Done::Failed(e.text()),
            };
            let code = v["code"].as_str().unwrap_or("").to_string();
            let version = v["version"].as_u64().unwrap_or(0) as u32;
            match c.load(&code, Some(version)) {
                Ok((version, p)) => Done::Merged { id, suggestion, code, version, files: p.files },
                Err(e) => Done::Failed(e.text()),
            }
        });
    }

    pub fn close_suggestion(&self, who: &Who, suggestion: i64) {
        self.run(who, move |c| match c.market("POST", &format!("/api/suggestions/{suggestion}/close"), None) {
            Ok(_) => Done::Said("Closed. It stays in their remix.".into()),
            Err(e) => Done::Failed(e.text()),
        });
    }

    pub fn report(&self, who: &Who, code: &str, reason: &str) {
        let (code, reason) = (code.to_string(), reason.to_string());
        self.run(who, move |c| match c.market("POST", &format!("/api/c/{code}/report"), Some(json!({ "reason": reason }))) {
            Ok(_) => Done::Said("Thanks. A moderator will look at it.".into()),
            Err(e) => Done::Failed(e.text()),
        });
    }

    /// A pass that tells a server who this player is. Quick, and nothing
    /// is lost without one: worlds that don't ask let anyone in.
    pub fn pass(&self, who: &Who, address: &str) {
        let address = address.to_string();
        let mut who = who.clone();
        if who.device.is_empty() {
            let tx = self.tx.clone();
            let _ = tx.send(Done::Pass { address, pass: None });
            return;
        }
        who.name = who.name.chars().take(32).collect();
        let tx = self.tx.clone();
        let ctx = Ctx {
            server: Server { agent: agent(5), base: who.server.trim_end_matches('/').to_string() },
            market: Market { agent: agent(5), base: who.market.trim_end_matches('/').to_string(), young: who.young },
            who,
            token: self.token.clone(),
        };
        std::thread::spawn(move || {
            let pass = ctx.market("POST", "/api/pass", Some(json!({ "server": address }))).ok().and_then(|v| v["pass"].as_str().map(|s| s.to_string()));
            let _ = tx.send(Done::Pass { address, pass });
        });
    }

    /// The servers listed on the market.
    pub fn servers(&self, who: &Who) {
        self.run(who, |c| match c.market.call("GET", "/api/servers", None, "", None) {
            Ok(v) => Done::Servers(serde_json::from_value(v["servers"].clone()).unwrap_or_default()),
            Err(e) => Done::Failed(e.text()),
        });
    }

    /// Who this device plays as on the market.
    pub fn account(&self, who: &Who) {
        self.run(who, |c| match c.market("GET", "/api/me", None) {
            Ok(v) => match serde_json::from_value(v) {
                Ok(a) => Done::Account(a),
                Err(e) => Done::Failed(format!("the market's answer didn't make sense ({e})")),
            },
            Err(e) => Done::Failed(e.text()),
        });
    }

    /// A code for another device to type, so it plays as this account too.
    pub fn link_code(&self, who: &Who) {
        self.run(who, |c| match c.market("POST", "/api/link/code", None) {
            Ok(v) => Done::LinkCode(v["code"].as_str().unwrap_or("").to_string()),
            Err(e) => Done::Failed(e.text()),
        });
    }

    /// This device plays as the account a link code or recovery code
    /// belongs to.
    pub fn link_device(&self, who: &Who, code: &str) {
        let code = clean_code(code);
        self.run(who, move |c| match c.market("POST", "/api/link", Some(json!({ "code": code, "device": device_label() }))) {
            Ok(v) => Done::Linked {
                name: v["name"].as_str().unwrap_or("").to_string(),
                handle: v["handle"].as_str().unwrap_or("").to_string(),
                recovery: v["recovery"].as_bool().unwrap_or(false),
            },
            Err(e) => Done::Failed(e.text()),
        });
    }

    /// This device stops playing as the account it was linked to.
    pub fn unlink(&self, who: &Who) {
        self.run(who, |c| match c.market("POST", "/api/link/leave", None) {
            Ok(_) => Done::Unlinked,
            Err(e) => Done::Failed(e.text()),
        });
    }

    /// What the market offers the Green's arches (only what's checked for
    /// all ages when `who` is young), and the jam running now. Nobody signs
    /// in for it, and an unreachable market offers nothing.
    pub fn green(&self, who: &Who) {
        self.run(who, |c| match c.market.call("GET", "/api/green", None, "", None) {
            Ok(v) => {
                let picks = serde_json::from_value(v["arches"].clone()).unwrap_or_default();
                let jam = c.market.call("GET", "/api/jams", None, "", None).ok().and_then(|v| serde_json::from_value(v["current"].clone()).ok());
                Done::Green { picks, jam }
            }
            // quietly: the arches show what players bring instead
            Err(_) => Done::Green { picks: vec![], jam: None },
        });
    }

    /// Puts a creation shared to Everyone in the jam running now.
    pub fn enter_jam(&self, who: &Who, jam: &JamNow, code: &str) {
        let (slug, theme, code) = (jam.slug.clone(), jam.theme.clone(), code.to_string());
        self.run(who, move |c| match c.market("POST", &format!("/api/jams/{slug}/enter"), Some(json!({ "code": code }))) {
            Ok(_) => Done::Entered { theme },
            Err(e) => Done::Failed(e.text()),
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
