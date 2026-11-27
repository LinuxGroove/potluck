//! The dev bridge: an MCP server on this computer only, off unless the
//! player turns it on (Settings, or `--bridge`), so a coding helper can work
//! on creations with the game: list them, reload one from its folder, read
//! its log, run a few lines in its sandbox, turn its knobs, see what's near
//! the player and take a screenshot. It speaks MCP's Streamable HTTP at
//! `http://127.0.0.1:47130/mcp`, answering each message with plain JSON.
//!
//! It never reaches past what the player could do at the keyboard: it
//! listens on 127.0.0.1 only, answers only requests addressed to this
//! computer (no web page can reach it: its Host and Origin must be local),
//! and every tool goes through the game on its next frame, where creations
//! keep to their sandboxes as always.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bevy::prelude::*;
use bevy::render::view::screenshot::ScreenshotCaptured;
use serde_json::{json, Value};

use crate::creations::{self, Library, Origin};
use crate::creatures::Herd;
use crate::net::{Net, Role};
use crate::player::Me;
use crate::script::{KnobValue, Scripts};
use crate::testing;
use crate::settings::Settings;
use crate::ui::{Toasts, WorldInfo};
use crate::world::Voxels;
use crate::KnobEdits;

/// Where it listens unless `--bridge=PORT` or `POTLUCK_BRIDGE_PORT` says.
pub const PORT: u16 = 47130;
/// The MCP versions it speaks, newest first.
const VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];
const MAX_BODY: usize = 1 << 20;
/// How long a tool may take on the game's side (a screenshot waits for a
/// frame to be drawn).
const WAIT: Duration = Duration::from_secs(30);

/// What a tool gives back.
#[derive(Debug, Clone, PartialEq)]
pub enum Part {
    Text(String),
    Png(Vec<u8>),
}

/// A tool's answer, or what went wrong (shown to the helper as an error).
pub type Answer = Result<Vec<Part>, String>;

/// Something a helper asked of the game.
pub enum Call {
    /// A helper connected, and said its name.
    Hello(String),
    /// A tool to run on the game's next frame.
    Tool { name: String, args: Value, reply: Sender<Answer> },
}

/// The bridge while it's listening.
pub struct Open {
    pub port: u16,
    calls: Mutex<Receiver<Call>>,
    stop: Arc<AtomicBool>,
}

impl Open {
    /// What helpers asked since last time.
    pub fn calls(&self) -> Vec<Call> {
        self.calls.lock().unwrap().try_iter().collect()
    }
}

impl Drop for Open {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

#[derive(Resource, Default)]
pub struct Bridge {
    pub open: Option<Open>,
    /// Why it couldn't listen, for Settings.
    pub problem: Option<String>,
    /// `--bridge` on the command line: on for this run.
    asked: Option<u16>,
}

impl Bridge {
    /// Its address, for Settings, while it's on.
    pub fn address(&self) -> Option<String> {
        self.open.as_ref().map(|o| format!("http://127.0.0.1:{}/mcp", o.port))
    }
}

/// Adds the bridge to the game (never the server: it has no screen or player).
pub fn add(app: &mut App) {
    let args: Vec<String> = std::env::args().collect();
    let asked = args.iter().find_map(|a| match a.as_str() {
        "--bridge" => Some(port()),
        _ => a.strip_prefix("--bridge=").and_then(|p| p.parse().ok()),
    });
    app.insert_resource(Bridge { asked, ..default() }).add_systems(Update, serve.after(crate::Sim));
}

fn port() -> u16 {
    std::env::var("POTLUCK_BRIDGE_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(PORT)
}

/// Starts listening on 127.0.0.1 (port 0 picks a free one).
pub fn listen(port: u16) -> std::io::Result<Open> {
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    let port = listener.local_addr()?.port();
    listener.set_nonblocking(true)?;
    let (tx, rx) = channel();
    let stop = Arc::new(AtomicBool::new(false));
    let s = stop.clone();
    std::thread::Builder::new().name("dev bridge".into()).spawn(move || {
        while !s.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((stream, _)) => {
                    let (tx, s) = (tx.clone(), s.clone());
                    let _ = std::thread::Builder::new().name("dev bridge call".into()).spawn(move || connection(stream, port, &tx, &s));
                }
                Err(_) => std::thread::sleep(Duration::from_millis(40)),
            }
        }
    })?;
    Ok(Open { port, calls: Mutex::new(rx), stop })
}

// ---------- HTTP ----------

struct Request {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }
}

struct Response {
    status: &'static str,
    kind: &'static str,
    body: Vec<u8>,
}

fn text(status: &'static str, s: impl Into<String>) -> Response {
    Response { status, kind: "text/plain; charset=utf-8", body: s.into().into_bytes() }
}

fn connection(stream: TcpStream, port: u16, tx: &Sender<Call>, stop: &AtomicBool) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(120)));
    let Ok(read) = stream.try_clone() else { return };
    let mut reader = BufReader::new(read);
    let mut out = stream;
    while !stop.load(Ordering::Relaxed) {
        let req = match read_request(&mut reader) {
            Ok(Some(r)) => r,
            Ok(None) => return,
            Err(r) => {
                let _ = write_response(&mut out, &r, true);
                return;
            }
        };
        let close = req.header("connection").is_some_and(|c| c.eq_ignore_ascii_case("close"));
        let r = respond(&req, port, tx);
        if write_response(&mut out, &r, close).is_err() || close {
            return;
        }
    }
}

/// One request off the connection; None when it closed, or the answer to
/// give one that can't be read.
fn read_request(r: &mut impl BufRead) -> Result<Option<Request>, Response> {
    let mut line = String::new();
    match r.by_ref().take(8192).read_line(&mut line) {
        Ok(0) | Err(_) => return Ok(None),
        Ok(_) => {}
    }
    let mut parts = line.split_whitespace();
    let (Some(method), Some(path)) = (parts.next(), parts.next()) else { return Err(text("400 Bad Request", "That isn't HTTP.")) };
    let (method, path) = (method.to_string(), path.to_string());
    let mut headers = Vec::new();
    let mut size = line.len();
    loop {
        let mut h = String::new();
        match r.by_ref().take(8192).read_line(&mut h) {
            Ok(0) | Err(_) => return Ok(None),
            Ok(n) => size += n,
        }
        if size > 32 * 1024 {
            return Err(text("431 Request Header Fields Too Large", "Too many headers."));
        }
        let h = h.trim_end();
        if h.is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            headers.push((k.trim().to_ascii_lowercase(), v.trim().to_string()));
        }
    }
    let req = Request { method, path, headers, body: Vec::new() };
    if req.header("transfer-encoding").is_some() {
        return Err(text("411 Length Required", "Send a Content-Length."));
    }
    let len: usize = req.header("content-length").and_then(|l| l.parse().ok()).unwrap_or(0);
    if len > MAX_BODY {
        return Err(text("413 Content Too Large", "That's too big for the dev bridge."));
    }
    let mut body = vec![0; len];
    if r.read_exact(&mut body).is_err() {
        return Ok(None);
    }
    Ok(Some(Request { body, ..req }))
}

fn write_response(out: &mut TcpStream, r: &Response, close: bool) -> std::io::Result<()> {
    let mut head = format!("HTTP/1.1 {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\n", r.status, r.body.len());
    if !r.body.is_empty() {
        head.push_str(&format!("Content-Type: {}\r\n", r.kind));
    }
    if r.status.starts_with("405") {
        head.push_str("Allow: POST\r\n");
    }
    head.push_str(if close { "Connection: close\r\n\r\n" } else { "Connection: keep-alive\r\n\r\n" });
    out.write_all(head.as_bytes())?;
    out.write_all(&r.body)?;
    out.flush()
}

/// The host part of `host:port`, `[::1]:port` or a bare host.
fn host_of(s: &str) -> &str {
    if let Some(rest) = s.strip_prefix('[') {
        return rest.split(']').next().unwrap_or("");
    }
    s.split(':').next().unwrap_or("")
}

fn local(host: &str) -> bool {
    matches!(host.to_ascii_lowercase().as_str(), "127.0.0.1" | "localhost" | "::1")
}

/// Only requests meant for this computer: a web page somewhere else can't
/// point a name at 127.0.0.1 and talk to the bridge (its Host would be that
/// name), and pages' own requests carry their Origin.
fn allowed(req: &Request) -> bool {
    let host_ok = req.header("host").is_some_and(|h| local(host_of(h)));
    let origin_ok = req.header("origin").is_none_or(|o| {
        let rest = o.strip_prefix("http://").or_else(|| o.strip_prefix("https://"));
        rest.is_some_and(|r| local(host_of(r.split('/').next().unwrap_or(""))))
    });
    host_ok && origin_ok
}

fn respond(req: &Request, port: u16, tx: &Sender<Call>) -> Response {
    if !allowed(req) {
        return text("403 Forbidden", "The dev bridge only answers programs on this computer.");
    }
    let path = req.path.split('?').next().unwrap_or("");
    match (req.method.as_str(), path) {
        ("POST", "/mcp") => {
            let Ok(msg) = serde_json::from_slice::<Value>(&req.body) else {
                return json_response("400 Bad Request", &error(Value::Null, -32700, "That isn't JSON."));
            };
            match handle(&msg, tx) {
                Some(answer) => json_response("200 OK", &answer),
                None => Response { status: "202 Accepted", kind: "", body: Vec::new() },
            }
        }
        (_, "/mcp") => text("405 Method Not Allowed", "The dev bridge takes POSTs of JSON-RPC, one message at a time."),
        ("GET", "/") => text("200 OK", format!("Potluck's dev bridge. Add http://127.0.0.1:{port}/mcp to your coding helper as an MCP server (HTTP).\n")),
        _ => text("404 Not Found", "The dev bridge is at /mcp."),
    }
}

fn json_response(status: &'static str, v: &Value) -> Response {
    Response { status, kind: "application/json", body: serde_json::to_vec(v).unwrap_or_default() }
}

// ---------- MCP ----------

fn ok(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

/// The answer to one JSON-RPC message; None for notifications and replies.
pub fn handle(msg: &Value, tx: &Sender<Call>) -> Option<Value> {
    if msg.is_array() {
        return Some(error(Value::Null, -32600, "Send one message at a time."));
    }
    let id = msg.get("id")?.clone();
    let Some(method) = msg.get("method").and_then(Value::as_str) else {
        // a reply to something we asked: we never ask
        return None;
    };
    let params = msg.get("params").cloned().unwrap_or(Value::Null);
    Some(match method {
        "initialize" => {
            let asked = params.get("protocolVersion").and_then(Value::as_str).unwrap_or("");
            let version = VERSIONS.iter().find(|v| **v == asked).unwrap_or(&VERSIONS[0]);
            let name: String = params.pointer("/clientInfo/name").and_then(Value::as_str).unwrap_or("A coding helper").chars().filter(|c| !c.is_control()).take(30).collect();
            let _ = tx.send(Call::Hello(name));
            ok(
                id,
                json!({
                    "protocolVersion": version,
                    "capabilities": { "tools": { "listChanged": false } },
                    "serverInfo": { "name": "potluck", "title": "Potluck", "version": env!("CARGO_PKG_VERSION") },
                    "instructions": instructions(),
                }),
            )
        }
        "ping" => ok(id, json!({})),
        "tools/list" => ok(id, json!({ "tools": tools() })),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            if !TOOLS.iter().any(|t| t.0 == name) {
                return Some(error(id, -32602, &format!("There's no tool called \"{name}\".")));
            }
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            let (reply, answer) = channel();
            if tx.send(Call::Tool { name: name.to_string(), args, reply }).is_err() {
                return Some(error(id, -32603, "The game has stopped."));
            }
            // a creation's tests may take a while to play out
            let wait = if name == "test" { WAIT * 10 } else { WAIT };
            let result = answer.recv_timeout(wait).unwrap_or_else(|_| Err("The game didn't answer in time: it may be loading, or stopped.".into()));
            ok(id, result_json(result))
        }
        _ => error(id, -32601, &format!("The dev bridge doesn't know {method}.")),
    })
}

fn result_json(a: Answer) -> Value {
    use base64::Engine;
    match a {
        Ok(parts) => {
            let content: Vec<Value> = parts
                .into_iter()
                .map(|p| match p {
                    Part::Text(t) => json!({ "type": "text", "text": t }),
                    Part::Png(png) => json!({ "type": "image", "data": base64::engine::general_purpose::STANDARD.encode(png), "mimeType": "image/png" }),
                })
                .collect();
            json!({ "content": content, "isError": false })
        }
        Err(e) => json!({ "content": [{ "type": "text", "text": e }], "isError": true }),
    }
}

fn instructions() -> String {
    format!(
        "This is Potluck, a block sandbox whose creatures, blocks and mini games are creations: folders of Luau with a potluck.toml. \
         The player's own creations are folders in {}; edit their files there (a new folder with potluck.toml and main.luau is a new creation), \
         then call reload, which starts or restarts it in the world and says what went wrong, with the line. guide gives the whole API (AGENTS.md); \
         log shows what a creation printed; run tries a few lines inside it; test runs its own tests (test.luau) in a copy of the world; \
         look_around and screenshot show the world near the player. \
         Creations only run on the world's host: in a friend's world, reload sends yours to them.",
        creations::my_folder().display()
    )
}

/// name, title, what it does, its arguments' schema.
const TOOLS: &[(&str, &str, &str, &str)] = &[
    (
        "creations",
        "List creations",
        "The creations in this game: which run in the world, the folder each of the player's own is in, their errors, knobs, actions, creatures, memory and load time.",
        r#"{"type":"object","properties":{}}"#,
    ),
    (
        "guide",
        "The creation API",
        "How to make creations: AGENTS.md, the guide to every module of the API, its limits and examples. Read it before writing a creation.",
        r#"{"type":"object","properties":{"types":{"type":"boolean","description":"The API's Luau types (types/potluck.d.luau) instead"}}}"#,
    ),
    (
        "reload",
        "Reload a creation",
        "Reads one of the player's creations from its folder again (or finds a new folder) and starts or restarts it in the world, keeping its creatures and what they remember. Says what went wrong, with the line, if it can't start; the last version that started keeps running.",
        r#"{"type":"object","properties":{"creation":{"type":"string","description":"Its id: the folder's name"}},"required":["creation"]}"#,
    ),
    (
        "log",
        "Read a creation's log",
        "What a creation printed, its errors (newest last), whether it's paused, and what its client half printed on this game.",
        r#"{"type":"object","properties":{"creation":{"type":"string"},"lines":{"type":"integer","minimum":1,"maximum":200,"description":"How many of the last lines (50 unless asked)"}},"required":["creation"]}"#,
    ),
    (
        "run",
        "Run Luau in a creation",
        "Runs a few lines of Luau inside a running creation's sandbox, as if they were part of its code, and returns what they printed and returned. require the API as the creation does (local world = require(\"@potluck/world\")); its files' locals aren't reachable. One tick's time (15 ms). Good for checking the world: return #world.creatures().",
        r#"{"type":"object","properties":{"creation":{"type":"string"},"code":{"type":"string","description":"Luau; end with return to see values"}},"required":["creation","code"]}"#,
    ),
    (
        "test",
        "Test a creation",
        "Runs a creation's own tests: its test.luau, written with @potluck/test (see the guide). Each case plays in a copy of this world with only that creation and players made for the test in it, so nothing reaches the world or anyone in it. One of the player's creations is read from its folder as saved. Says which cases passed and, for the others, why they failed, with the line, and what the creation printed.",
        r#"{"type":"object","properties":{"creation":{"type":"string","description":"Its id: the folder's name for the player's own"}},"required":["creation"]}"#,
    ),
    (
        "tweak",
        "Turn a creation's knob",
        "Sets one of a running creation's Tweak knobs, as the player would in Tweak; it reloads with the new value a moment later. creations lists the knobs.",
        r##"{"type":"object","properties":{"creation":{"type":"string"},"knob":{"type":"string","description":"The knob's key"},"value":{"description":"A number, true or false, or a colour like \"#c24b41\""}},"required":["creation","knob","value"]}"##,
    ),
    (
        "look_around",
        "Look around the player",
        "Where the player is and what's near them: position, which way they face, health, what's in hand, the block they look at, creatures nearby (creation, kind, rider and the fields their code keeps), other players, the blocks about and the time of day.",
        r#"{"type":"object","properties":{"radius":{"type":"number","minimum":1,"maximum":64,"description":"How far to look for creatures and players, in blocks (24 unless asked)"}}}"#,
    ),
    (
        "screenshot",
        "Take a screenshot",
        "A picture of the game's screen as it is now, as PNG.",
        r#"{"type":"object","properties":{"width":{"type":"integer","minimum":256,"maximum":1920,"description":"Scaled down to at most this wide (1024 unless asked)"}}}"#,
    ),
];

fn tools() -> Vec<Value> {
    TOOLS
        .iter()
        .map(|(name, title, about, schema)| {
            let read_only = matches!(*name, "creations" | "guide" | "log" | "look_around" | "screenshot");
            json!({
                "name": name,
                "title": title,
                "description": about,
                "inputSchema": serde_json::from_str::<Value>(schema).unwrap_or(json!({"type":"object"})),
                "annotations": { "readOnlyHint": read_only, "openWorldHint": false },
            })
        })
        .collect()
}

// ---------- the game's side ----------

/// What the tools can reach, borrowed from the game for one frame.
pub struct Cx<'a> {
    pub lib: &'a mut Library,
    pub scripts: &'a mut Scripts,
    pub herd: &'a mut Herd,
    pub net: &'a mut Net,
    pub toasts: &'a mut Toasts,
    pub voxels: &'a mut Voxels,
    pub edits: &'a mut KnobEdits,
    pub me: Option<&'a Me>,
    pub halves: Option<&'a crate::halves::Halves>,
    pub in_world: bool,
    pub survival: bool,
    /// 0 is midnight, 0.5 noon.
    pub day: f32,
}

fn arg<'v>(args: &'v Value, key: &str) -> Result<&'v str, String> {
    args.get(key).and_then(Value::as_str).filter(|s| !s.is_empty()).ok_or_else(|| format!("say which {key}"))
}

fn known(cx: &Cx, id: &str) -> Result<(), String> {
    if cx.lib.list.contains_key(id) {
        Ok(())
    } else {
        Err(format!("there's no creation called \"{id}\" (creations lists them)"))
    }
}

/// Runs a tool, but for the screenshot, which needs a frame drawn.
pub fn answer(name: &str, args: &Value, cx: &mut Cx) -> Answer {
    let one = |s: String| Ok(vec![Part::Text(s)]);
    match name {
        "creations" => one(pretty(&list(cx))),
        "guide" => one(if args.get("types").and_then(Value::as_bool) == Some(true) { creations::TYPES.into() } else { creations::AGENTS_MD.into() }),
        "reload" => one(reload(arg(args, "creation")?, cx)?),
        "log" => one(log(arg(args, "creation")?, args.get("lines").and_then(Value::as_u64).unwrap_or(50).clamp(1, 200) as usize, cx)?),
        "run" => {
            let id = arg(args, "creation")?;
            let code = arg(args, "code")?;
            known(cx, id)?;
            if matches!(cx.net.role, Role::Client(_)) {
                return Err("creations run on the world's host: try this in a world of your own".into());
            }
            let (printed, values) = cx.scripts.run(cx.voxels, cx.herd, id, code)?;
            let mut s = String::new();
            if !printed.is_empty() {
                s.push_str(&format!("printed:\n{}\n", printed.join("\n")));
            }
            s.push_str(&format!("returned: {}", if values.is_empty() { "nothing".into() } else { values }));
            one(s)
        }
        "test" => tests_of(arg(args, "creation")?, cx.lib, cx.voxels)?(),
        "tweak" => one(tweak(arg(args, "creation")?, arg(args, "knob")?, args.get("value").unwrap_or(&Value::Null), cx)?),
        "look_around" => one(pretty(&look_around(args.get("radius").and_then(Value::as_f64).unwrap_or(24.0).clamp(1.0, 64.0) as f32, cx))),
        "screenshot" => Err("there's no screen here to take a picture of".into()),
        _ => Err(format!("there's no tool called {name}")),
    }
}

/// A creation's tests, ready to run away from the game's own thread.
fn tests_of(id: &str, lib: &Library, voxels: &Voxels) -> Result<impl FnOnce() -> Answer + Send + 'static, String> {
    let c = lib.to_test(id)?;
    if !c.files.contains_key(testing::FILE) {
        return Err(format!("{} has no {} yet: the guide's Testing part says how to write one", c.manifest.name, testing::FILE));
    }
    let (seed, bytes) = (voxels.seed, voxels.encode());
    Ok(move || Ok(vec![Part::Text(testing::run_in(&c, seed, &bytes).text())]))
}

/// A number to a tenth, as JSON shows it.
fn tenths(x: f32) -> f64 {
    (x as f64 * 10.0).round() / 10.0
}

fn pretty(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_default()
}

fn list(cx: &Cx) -> Value {
    let mut out = Vec::new();
    for (id, c) in &cx.lib.list {
        let vm = cx.scripts.vms.get(id);
        let origin = match &c.origin {
            Origin::Base => "the base game".to_string(),
            Origin::Mine => "the player's own".to_string(),
            Origin::Friend(who) => format!("from {who}'s world"),
            Origin::Code(code) => format!("loaded by code {code}"),
        };
        let mut e = json!({
            "id": id,
            "name": c.manifest.name,
            "by": c.manifest.by,
            "kind": c.manifest.kind,
            "origin": origin,
            "running": cx.lib.running.contains(id),
            "main": c.manifest.main,
        });
        if let Some(dir) = &c.dir {
            e["folder"] = json!(dir.display().to_string());
        }
        if let Some(client) = &c.manifest.client {
            e["client_half"] = json!(client);
        }
        if !c.manifest.actions.is_empty() {
            e["actions"] = c.manifest.actions.iter().map(|(k, a)| (k.clone(), json!(format!("{} ({}, {})", a.name, a.pad, a.keys)))).collect::<serde_json::Map<_, _>>().into();
        }
        if let Some(vm) = vm {
            e["creatures"] = json!(cx.herd.bodies.values().filter(|b| b.creation == *id).count());
            e["kinds"] = json!(vm.kinds.iter().map(|k| k.name.clone()).collect::<Vec<_>>());
            e["load_ms"] = json!(tenths(vm.load_ms));
            e["memory_kb"] = json!(vm.memory() / 1024);
            e["budgets"] = json!(vm.budget());
            e["knobs"] = vm
                .knobs
                .iter()
                .map(|k| {
                    let v = match &k.value {
                        KnobValue::Number(n) => json!(n),
                        KnobValue::Toggle(b) => json!(b),
                        KnobValue::Color(s) => json!(s),
                    };
                    (k.key.clone(), json!({ "label": k.label, "value": v }))
                })
                .collect::<serde_json::Map<_, _>>()
                .into();
            if let Some(err) = &vm.error {
                e["error"] = json!(err);
            }
            if vm.paused {
                e["paused"] = json!("after too many errors: reload it once it's fixed");
            }
        }
        out.push(e);
    }
    json!({ "folder": creations::my_folder().display().to_string(), "creations": out })
}

fn reload(id: &str, cx: &mut Cx) -> Result<String, String> {
    let mine = creations::my_folder().join(id).join("potluck.toml").is_file();
    let mut note = String::new();
    if mine {
        if let Some(base) = cx.lib.list.get(id).filter(|c| c.origin == Origin::Base) {
            note = format!(" The folder has the same name as the base game's {}, so it takes its place; give the folder another name to keep both.", base.manifest.name);
        }
        cx.lib.read_mine(id)?;
    }
    known(cx, id)?;
    let name = cx.lib.list[id].manifest.name.clone();
    if !cx.in_world {
        return Err(format!("the player isn't in a world yet, so {name} has nowhere to run"));
    }
    let started = Instant::now();
    crate::game::bring_checked(id, cx.lib, cx.scripts, cx.herd, cx.net, cx.toasts).map_err(|e| {
        let kept = if cx.scripts.vms.contains_key(id) { " The last version that started is still running." } else { "" };
        format!("{name} didn't start: {e}.{kept}")
    })?;
    if let Role::Client(_) = cx.net.role {
        return Ok(format!("Sent {name} to the world's host, who runs it."));
    }
    let ms = started.elapsed().as_secs_f32() * 1000.0;
    cx.toasts.push(format!("{name} reloaded in {} ms", ms.round().max(1.0)));
    let vm = &cx.scripts.vms[id];
    let creatures = cx.herd.bodies.values().filter(|b| b.creation == id).count();
    Ok(format!(
        "{name} is running ({ms:.0} ms). Kinds: {}. Creatures now: {creatures} (kinds with a count appear on its next tick).{note}",
        if vm.kinds.is_empty() { "none".to_string() } else { vm.kinds.iter().map(|k| k.name.clone()).collect::<Vec<_>>().join(", ") }
    ))
}

fn log(id: &str, n: usize, cx: &Cx) -> Result<String, String> {
    known(cx, id)?;
    let mut s = String::new();
    match cx.scripts.vms.get(id) {
        Some(vm) => {
            let from = vm.log.len().saturating_sub(n);
            if vm.log.is_empty() {
                s.push_str("It hasn't printed anything.\n");
            } else {
                s.push_str(&vm.log[from..].join("\n"));
                s.push('\n');
            }
            if let Some(e) = &vm.error {
                s.push_str(&format!("Its last error: {e}\n"));
            }
            if vm.paused {
                s.push_str("It's paused after too many errors: reload it once it's fixed.\n");
            }
        }
        None if matches!(cx.net.role, Role::Client(_)) => s.push_str("It runs on the world's host, so its log is there.\n"),
        None => s.push_str("It isn't running in this world: reload starts it.\n"),
    }
    if let Some(h) = cx.halves.and_then(|h| h.list.get(id)) {
        let from = h.log.len().saturating_sub(n);
        for l in &h.log[from..] {
            s.push_str(&format!("client: {l}\n"));
        }
        if let Some(e) = &h.error {
            s.push_str(&format!("Its client half stopped: {e}\n"));
        }
    }
    Ok(s.trim_end().to_string())
}

fn tweak(id: &str, key: &str, value: &Value, cx: &mut Cx) -> Result<String, String> {
    known(cx, id)?;
    let vm = cx.scripts.vms.get(id).ok_or_else(|| format!("{id} isn't running, so its knobs aren't known: reload starts it"))?;
    let k = vm.knobs.iter().find(|k| k.key == key).ok_or_else(|| {
        let keys: Vec<&str> = vm.knobs.iter().map(|k| k.key.as_str()).collect();
        format!("{id} has no knob called {key} (it has: {})", if keys.is_empty() { "none".into() } else { keys.join(", ") })
    })?;
    let v = match (&k.value, value) {
        (KnobValue::Number(_), Value::Number(n)) => KnobValue::Number(n.as_f64().unwrap_or(0.0)),
        (KnobValue::Toggle(_), Value::Bool(b)) => KnobValue::Toggle(*b),
        (KnobValue::Color(_), Value::String(s)) if crate::script::parse_color(s).is_some() => KnobValue::Color(s.clone()),
        (KnobValue::Number(_), _) => return Err(format!("{key} is a number")),
        (KnobValue::Toggle(_), _) => return Err(format!("{key} is true or false")),
        (KnobValue::Color(_), _) => return Err(format!("{key} is a colour, like \"#c24b41\"")),
    };
    let label = k.label.clone();
    cx.edits.0.push((id.to_string(), key.to_string(), v));
    Ok(format!("{label} is set; {id} reloads with it in a moment."))
}

fn look_around(radius: f32, cx: &Cx) -> Value {
    let Some(me) = cx.me.filter(|_| cx.in_world) else {
        return json!({ "in_world": false, "note": "the player is on the title screen: nothing to see yet" });
    };
    let at = |p: Vec3| json!([tenths(p.x), tenths(p.y), tenths(p.z)]);
    let facing = Vec3::new(-me.yaw.sin() * me.pitch.cos(), me.pitch.sin(), -me.yaw.cos() * me.pitch.cos());
    let name = |b| crate::blocks::block(b).name;
    let feet = me.pos.floor().as_ivec3();
    let mut you = json!({
        "at": at(me.pos),
        "facing": at(facing),
        "mode": if cx.survival { "survival" } else { "build" },
        "flying": me.flying,
        "standing_on": name(cx.voxels.at(feet - IVec3::Y)),
        "in_hand": name(me.hotbar[me.slot.min(8)]),
        "hotbar": me.hotbar.iter().map(|b| name(*b)).collect::<Vec<_>>(),
    });
    if cx.survival {
        you["health"] = json!(me.health);
        you["counts"] = json!(me.counts);
    }
    if let Some(r) = me.riding {
        you["riding"] = json!(r);
    }
    let world = match &cx.net.role {
        Role::Solo => "alone",
        Role::Host(_) => "hosting friends",
        Role::Client(_) => "in a friend's world",
    };
    let mut out = json!({ "in_world": true, "world": world, "you": you, "time_of_day": (cx.day as f64 * 100.0).round() / 100.0, "time_note": "0 is midnight, 0.25 sunrise, 0.5 noon, 0.75 sunset" });
    if let Some(h) = &me.target {
        out["looking_at"] = json!({ "block": name(h.id), "at": [h.block.x, h.block.y, h.block.z] });
    }
    if let Some(b) = me.aim_body.and_then(|id| cx.herd.bodies.get(&id)) {
        out["looking_at_creature"] = json!(b.id);
    }
    // creatures, nearest first, with what their code keeps on them
    let mut near: Vec<_> = cx.herd.bodies.values().map(|b| (b.pos.distance(me.pos), b)).filter(|(d, _)| *d <= radius).collect();
    near.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut fields: std::collections::HashMap<String, std::collections::HashMap<u64, Value>> = Default::default();
    let creatures: Vec<Value> = near
        .iter()
        .take(40)
        .map(|(d, b)| {
            let mem = fields.entry(b.creation.clone()).or_insert_with(|| cx.scripts.vms.get(&b.creation).map(|v| v.memories()).unwrap_or_default());
            let mut c = json!({ "id": b.id, "creation": b.creation, "kind": b.kind, "at": at(b.pos), "distance": tenths(*d), "on_ground": b.on_ground });
            if let Some(r) = b.rider {
                let who = cx.scripts.players.iter().find(|p| p.id == r).map(|p| p.name.clone()).unwrap_or_else(|| format!("player {r}"));
                c["rider"] = json!(who);
            }
            if let Some(f) = mem.get(&b.id).filter(|f| f.as_object().is_some_and(|o| !o.is_empty())) {
                let s = f.to_string();
                c["fields"] = if s.len() <= 600 { f.clone() } else { json!(format!("{}...", s.chars().take(600).collect::<String>())) };
            }
            c
        })
        .collect();
    out["creatures"] = json!(creatures);
    if near.len() > 40 {
        out["more_creatures"] = json!(near.len() - 40);
    }
    let players: Vec<Value> = cx
        .scripts
        .players
        .iter()
        .filter(|p| p.id != cx.net.my_id && p.pos.distance(me.pos) <= radius)
        .map(|p| json!({ "name": p.name, "at": at(p.pos), "distance": tenths(p.pos.distance(me.pos)) }))
        .collect();
    out["players"] = json!(players);
    // the blocks within 8, most first
    let mut counts: std::collections::BTreeMap<&str, u32> = Default::default();
    for y in -8..=8 {
        for z in -8..=8 {
            for x in -8..=8 {
                let b = cx.voxels.at(feet + IVec3::new(x, y, z));
                if b != crate::blocks::AIR {
                    *counts.entry(name(b)).or_default() += 1;
                }
            }
        }
    }
    let mut counts: Vec<_> = counts.into_iter().collect();
    counts.sort_by(|a, b| b.1.cmp(&a.1));
    out["blocks_within_8"] = counts.into_iter().take(24).map(|(k, v)| (k.to_string(), json!(v))).collect::<serde_json::Map<_, _>>().into();
    out
}

/// A screenshot as PNG, scaled down to `width` at most.
fn png_of(img: &Image, width: u32) -> Result<Vec<u8>, String> {
    let dynamic = img.clone().try_into_dynamic().map_err(|e| format!("couldn't read the screen: {e}"))?;
    let mut rgb = dynamic.to_rgb8();
    if rgb.width() > width {
        let h = ((rgb.height() as u64 * width as u64) / rgb.width() as u64).max(1) as u32;
        rgb = image::imageops::resize(&rgb, width, h, image::imageops::FilterType::Triangle);
    }
    let mut out = Vec::new();
    image::DynamicImage::ImageRgb8(rgb).write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).map_err(|e| format!("couldn't make the picture: {e}"))?;
    Ok(out)
}

/// Turns the bridge on and off with the setting, and answers what helpers ask.
#[allow(clippy::too_many_arguments)]
fn serve(
    mut bridge: ResMut<Bridge>,
    settings: Option<Res<Settings>>,
    info: Res<WorldInfo>,
    mut lib: ResMut<Library>,
    mut scripts: ResMut<Scripts>,
    mut herd: ResMut<Herd>,
    mut net: ResMut<Net>,
    mut toasts: ResMut<Toasts>,
    mut voxels: ResMut<Voxels>,
    mut edits: ResMut<KnobEdits>,
    me: Option<Res<Me>>,
    halves: Option<Res<crate::halves::Halves>>,
    clock: Res<crate::render::Clock>,
    off: Option<Res<crate::render::Offscreen>>,
    mut focus: ResMut<crate::ui::Focus>,
    mut commands: Commands,
) {
    let on = bridge.asked.is_some() || settings.as_ref().is_some_and(|s| s.dev_bridge);
    if !on {
        bridge.open = None;
        bridge.problem = None;
        if focus.bridge.is_some() {
            focus.bridge = None;
        }
        return;
    }
    if bridge.open.is_none() && bridge.problem.is_none() {
        let port = bridge.asked.unwrap_or_else(port);
        match listen(port) {
            Ok(o) => {
                info!("Dev bridge: http://127.0.0.1:{}/mcp", o.port);
                bridge.open = Some(o);
            }
            Err(e) => bridge.problem = Some(format!("The dev bridge couldn't use port {port}: {e}")),
        }
    }
    let says = match (&bridge.open, &bridge.problem) {
        (Some(o), _) => Some(format!("Add http://127.0.0.1:{}/mcp to your coding helper as an MCP server", o.port)),
        (None, p) => p.clone(),
    };
    if focus.bridge != says {
        focus.bridge = says;
    }
    let Some(open) = &bridge.open else { return };
    for call in open.calls() {
        match call {
            Call::Hello(who) => toasts.push(format!("{who} connected through the dev bridge")),
            Call::Tool { name, args, reply } if name == "screenshot" => {
                let width = args.get("width").and_then(Value::as_u64).unwrap_or(1024).clamp(256, 1920) as u32;
                let mut reply = Some(reply);
                commands.spawn(crate::render::screenshot(off.as_deref())).observe(move |shot: On<ScreenshotCaptured>| {
                    if let Some(r) = reply.take() {
                        let _ = r.send(png_of(&shot.image, width).map(|png| vec![Part::Png(png)]));
                    }
                });
            }
            Call::Tool { name, args, reply } if name == "test" => {
                // tests play out on a thread of their own, so the game doesn't stop
                match arg(&args, "creation").and_then(|id| tests_of(id, &lib, &voxels)) {
                    Ok(job) => {
                        std::thread::spawn(move || {
                            let _ = reply.send(job());
                        });
                    }
                    Err(e) => {
                        let _ = reply.send(Err(e));
                    }
                }
            }
            Call::Tool { name, args, reply } => {
                let mut cx = Cx {
                    lib: &mut lib,
                    scripts: &mut scripts,
                    herd: &mut herd,
                    net: &mut net,
                    toasts: &mut toasts,
                    voxels: &mut voxels,
                    edits: &mut edits,
                    me: me.as_deref(),
                    halves: halves.as_deref(),
                    in_world: info.in_world,
                    survival: settings.as_ref().is_some_and(|s| s.survival),
                    day: clock.time,
                };
                let _ = reply.send(answer(&name, &args, &mut cx));
            }
        }
    }
}
