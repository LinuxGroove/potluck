//! Running creations: one Luau sandbox per creation, the API modules they
//! require, and the calls into them (each tick, and when players use things).
//!
//! The API is versioned by module, never by the game (`MODULES`), and
//! docs/api.md is the promise that goes with it.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bevy::prelude::*;
use mlua::{Function, Lua, Table, Value, Vector, VmState};

use crate::blocks;
use crate::creations::Creation;
use crate::creatures::{Body, Herd, Part};
use crate::noise::Rng;
use crate::world::{self, Voxels};

/// The modules and their versions this game provides: (name, major, minor).
pub const MODULES: &[(&str, u32, u32)] = &[("world", 0, 2), ("tweak", 0, 1), ("game", 0, 1), ("store", 0, 1)];

const MEMORY_LIMIT: usize = 32 << 20;
const TICK_BUDGET: Duration = Duration::from_millis(15);
const LOAD_BUDGET: Duration = Duration::from_millis(300);
/// Timers one creation may have waiting at once.
pub const MAX_TIMERS: usize = 64;
/// Handlers one creation may give `world.on`.
pub const MAX_HANDLERS: usize = 32;
/// What `world.on` can listen for.
pub const EVENTS: &[&str] = &["player_joined", "player_left"];
/// What one creation may keep with `@potluck/store`, counted as JSON.
pub const STORE_QUOTA: usize = 64 << 10;

#[derive(Clone, Debug, PartialEq)]
pub enum KnobValue {
    Number(f64),
    Color(String),
    Toggle(bool),
}

#[derive(Clone, Debug)]
pub struct Knob {
    pub key: String,
    pub label: String,
    pub value: KnobValue,
    pub min: f64,
    pub max: f64,
    pub order: i64,
}

#[derive(Clone, Debug)]
pub struct KindDef {
    pub name: String,
    pub parts: Vec<Part>,
    pub half: Vec3,
    pub speed: f32,
    pub rideable: bool,
    pub seat: f32,
    pub count: u32,
    pub home: Vec3,
}

#[derive(Clone, Debug)]
pub struct PlayerInfo {
    pub id: u32,
    pub name: String,
    pub pos: Vec3,
}

/// What a creation asked for that the game does outside the sandbox.
#[derive(Default)]
pub struct Outbox {
    pub toasts: Vec<String>,
    /// (player, velocity): riders thrown off.
    pub throws: Vec<(u32, Vec3)>,
    /// (creation, board, lower is better, player, value).
    pub scores: Vec<(String, String, bool, u32, f64)>,
    /// (player, link, note): players off to another server.
    pub sends: Vec<(u32, String, String)>,
}

/// What every creation in a world can see beyond the world itself: its
/// boards, the other servers it can send players to, and what it keeps.
#[derive(Default)]
pub struct GameShared {
    pub boards: crate::hosting::Boards,
    pub links: Vec<String>,
    pub store: Store,
}

/// What creations keep in a world with `@potluck/store`: each creation's
/// keys and their values, as JSON. A server saves it beside the world.
#[derive(serde::Serialize, serde::Deserialize, Default, Debug, Clone)]
pub struct Store {
    pub data: BTreeMap<String, BTreeMap<String, serde_json::Value>>,
    /// Bytes each creation uses, worked out when first needed.
    #[serde(skip)]
    used: HashMap<String, usize>,
    /// Changed since it was last saved.
    #[serde(skip)]
    pub dirty: bool,
}

fn entry_size(key: &str, value: &serde_json::Value) -> usize {
    key.len() + value.to_string().len()
}

impl Store {
    pub fn load(path: &std::path::Path) -> Store {
        std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }

    pub fn save(&mut self, path: &std::path::Path) {
        if !self.dirty {
            return;
        }
        let tmp = path.with_extension("tmp");
        if std::fs::write(&tmp, serde_json::to_string(self).unwrap_or_default()).is_ok() && std::fs::rename(tmp, path).is_ok() {
            self.dirty = false;
        }
    }

    /// The bytes a creation's keys and values take, as JSON.
    pub fn used(&mut self, creation: &str) -> usize {
        if let Some(n) = self.used.get(creation) {
            return *n;
        }
        let n = self.data.get(creation).map(|m| m.iter().map(|(k, v)| entry_size(k, v)).sum()).unwrap_or(0);
        self.used.insert(creation.to_string(), n);
        n
    }

    pub fn get(&self, creation: &str, key: &str) -> Option<&serde_json::Value> {
        self.data.get(creation)?.get(key)
    }

    /// Keeps a value, or forgets the key with None. Refused past the quota.
    pub fn set(&mut self, creation: &str, key: &str, value: Option<serde_json::Value>) -> Result<(), String> {
        let used = self.used(creation);
        let old = self.get(creation, key).map(|v| entry_size(key, v)).unwrap_or(0);
        let new = value.as_ref().map(|v| entry_size(key, v)).unwrap_or(0);
        if new > old && used - old + new > STORE_QUOTA {
            return Err(format!("this creation's store is full ({} KB): forget something with store.set(key, nil) first", STORE_QUOTA >> 10));
        }
        let keys = self.data.entry(creation.to_string()).or_default();
        match value {
            Some(v) => {
                keys.insert(key.to_string(), v);
            }
            None => {
                keys.remove(key);
            }
        }
        if keys.is_empty() {
            self.data.remove(creation);
        }
        self.used.insert(creation.to_string(), used - old + new);
        self.dirty = true;
        Ok(())
    }

    pub fn keys(&self, creation: &str) -> Vec<String> {
        self.data.get(creation).map(|m| m.keys().cloned().collect()).unwrap_or_default()
    }
}

/// A creation's timers and world event handlers, kept from tick to tick.
#[derive(Default)]
struct Waiting {
    next: u32,
    timers: Vec<Timer>,
    on: Vec<(String, Function)>,
}

struct Timer {
    id: u32,
    left: f64,
    every: Option<f64>,
    f: Function,
}

fn add_timer(w: &Mutex<Waiting>, seconds: f64, every: bool, f: Function) -> mlua::Result<u32> {
    if !seconds.is_finite() {
        return Err(bad("a timer needs a number of seconds"));
    }
    let mut w = w.lock().unwrap();
    if w.timers.len() >= MAX_TIMERS {
        return Err(bad(format!("a creation can have at most {MAX_TIMERS} timers waiting: world.cancel the ones it's done with")));
    }
    w.next += 1;
    let id = w.next;
    // a tick is a tenth of a second, so nothing comes sooner
    let s = seconds.clamp(0.1, 86400.0);
    w.timers.push(Timer { id, left: s, every: every.then_some(s), f });
    Ok(id)
}

/// Everything the API's functions can reach during one call into a creation.
struct Ctx<'a> {
    voxels: &'a mut Voxels,
    herd: &'a mut Herd,
    players: &'a [PlayerInfo],
    out: &'a mut Outbox,
    creation: &'a str,
    kinds: &'a [KindDef],
    time: f64,
    rng: &'a mut Rng,
    log: &'a mut Vec<String>,
}

#[derive(Default)]
struct LoadState {
    kinds: Vec<(KindDef, Option<Table>)>,
    knobs: Vec<Knob>,
    saved: BTreeMap<String, KnobValue>,
    log: Vec<String>,
}

pub struct Vm {
    lua: Lua,
    world: Table,
    base: Table,
    game: Table,
    shared: Arc<Mutex<GameShared>>,
    waiting: Arc<Mutex<Waiting>>,
    /// Where `print` writes, read after every call into the creation.
    state: Arc<Mutex<LoadState>>,
    pub kinds: Vec<KindDef>,
    kind_tables: HashMap<String, Table>,
    instances: HashMap<u64, Table>,
    pub knobs: Vec<Knob>,
    pub log: Vec<String>,
    pub error: Option<String>,
    errors: u32,
    pub paused: bool,
    deadline: Arc<Mutex<Instant>>,
    rng: Rng,
    pub load_ms: f32,
}

#[derive(Resource, Default)]
pub struct Scripts {
    pub vms: BTreeMap<String, Vm>,
    pub shared: Arc<Mutex<GameShared>>,
    pub players: Vec<PlayerInfo>,
    pub out: Outbox,
    time: f64,
    tick_acc: f32,
    /// Who was here last tick, to tell creations who came and went.
    seen: Vec<PlayerInfo>,
}

fn vec_of(v: &Value) -> Option<Vec3> {
    match v {
        Value::Vector(v) => Some(Vec3::new(v.x(), v.y(), v.z())),
        Value::Table(t) => {
            let a: Option<f32> = t.get(1).ok();
            if let Some(x) = a {
                Some(Vec3::new(x, t.get(2).unwrap_or(0.0), t.get(3).unwrap_or(0.0)))
            } else {
                Some(Vec3::new(t.get("x").ok()?, t.get("y").ok()?, t.get("z").ok()?))
            }
        }
        _ => None,
    }
}

fn vector(p: Vec3) -> Value {
    Value::Vector(Vector::new(p.x, p.y, p.z))
}

fn bad(msg: impl Into<String>) -> mlua::Error {
    mlua::Error::RuntimeError(msg.into())
}

pub fn parse_color(s: &str) -> Option<Color> {
    let h = s.trim().trim_start_matches('#');
    if h.len() != 6 {
        return None;
    }
    let n = u32::from_str_radix(h, 16).ok()?;
    Some(Color::srgb_u8((n >> 16) as u8, (n >> 8) as u8, n as u8))
}

fn spot(name: &str) -> Option<Vec3> {
    match name {
        "green" => Some(Vec3::new(world::GREEN.x + 0.5, world::GREEN_Y as f32 + 1.0, world::GREEN.z + 7.5)),
        "paddock" => Some(world::paddock()),
        "creek" => Some(world::creek()),
        "start" | "hill" => Some(Vec3::new(world::START.x + 0.5, 0.0, world::START.z + 0.5)),
        _ => None,
    }
}

fn player_table(lua: &Lua, p: &PlayerInfo) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    t.set("id", p.id)?;
    t.set("name", p.name.clone())?;
    t.set("position", vector(p.pos))?;
    Ok(t)
}

/// Copies a creature's own fields out of the sandbox, to keep across a reload.
fn to_json(v: &Value, depth: u32) -> Option<serde_json::Value> {
    use serde_json::Value as J;
    if depth > 6 {
        return None;
    }
    Some(match v {
        Value::Nil => J::Null,
        Value::Boolean(b) => J::Bool(*b),
        Value::Integer(i) => J::from(*i),
        Value::Number(n) => serde_json::Number::from_f64(*n).map(J::Number)?,
        Value::String(s) => J::String(s.to_str().ok()?.to_string()),
        Value::Vector(v) => serde_json::json!({ "__vector": [v.x(), v.y(), v.z()] }),
        Value::Table(t) => {
            let mut m = serde_json::Map::new();
            for (k, val) in t.pairs::<Value, Value>().flatten() {
                let key = match &k {
                    Value::String(s) => s.to_str().ok()?.to_string(),
                    Value::Integer(i) => format!("#{i}"),
                    Value::Number(n) if n.fract() == 0.0 && n.abs() < 1e15 => format!("#{}", *n as i64),
                    _ => continue,
                };
                if let Some(j) = to_json(&val, depth + 1) {
                    m.insert(key, j);
                }
            }
            J::Object(m)
        }
        _ => return None,
    })
}

fn from_json(lua: &Lua, j: &serde_json::Value) -> mlua::Result<Value> {
    use serde_json::Value as J;
    Ok(match j {
        J::Null => Value::Nil,
        J::Bool(b) => Value::Boolean(*b),
        J::Number(n) => Value::Number(n.as_f64().unwrap_or(0.0)),
        J::String(s) => Value::String(lua.create_string(s)?),
        J::Array(_) => Value::Nil,
        J::Object(m) => {
            if let Some(J::Array(a)) = m.get("__vector") {
                let f = |i: usize| a.get(i).and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                return Ok(vector(Vec3::new(f(0), f(1), f(2))));
            }
            let t = lua.create_table()?;
            for (k, v) in m {
                if let Some(i) = k.strip_prefix('#').and_then(|i| i.parse::<i64>().ok()) {
                    t.set(i, from_json(lua, v)?)?;
                } else {
                    t.set(k.as_str(), from_json(lua, v)?)?;
                }
            }
            Value::Table(t)
        }
    })
}

const RESERVED: &[&str] = &["id", "position", "rider", "kind"];

fn parse_kind(def: &Table) -> mlua::Result<KindDef> {
    let name: String = def.get::<Option<String>>("name")?.ok_or_else(|| bad("a creature needs a name"))?;
    let mut parts = Vec::new();
    if let Some(body) = def.get::<Option<Table>>("body")? {
        for part in body.sequence_values::<Table>() {
            let part = part?;
            let pname: String = part.get::<Option<String>>(1)?.or(part.get::<Option<String>>("name")?).unwrap_or_else(|| "part".into());
            let size = vec_of(&part.get::<Value>("size")?).unwrap_or(Vec3::splat(0.5)).clamp(Vec3::splat(0.02), Vec3::splat(6.0));
            let at = vec_of(&part.get::<Value>("at")?).unwrap_or(Vec3::ZERO).clamp(Vec3::splat(-8.0), Vec3::splat(8.0));
            let color = part.get::<Option<String>>("color")?.and_then(|c| parse_color(&c)).unwrap_or(Color::srgb(0.6, 0.6, 0.6));
            let glow = part.get::<Option<bool>>("glow")?.unwrap_or(false);
            parts.push(Part {
                name: pname,
                size,
                at,
                color: color.to_srgba().to_f32_array_no_alpha(),
                leg: part.get::<Option<bool>>("leg")?.unwrap_or(false),
                head: part.get::<Option<bool>>("head")?.unwrap_or(false),
                glow,
            });
            if parts.len() >= 64 {
                break;
            }
        }
    }
    // the box it collides with: given, or round the body
    let half = match vec_of(&def.get::<Value>("size")?) {
        Some(s) => Vec3::new(s.x / 2.0, s.y, s.z / 2.0),
        None => {
            let mut lo = Vec3::splat(f32::MAX);
            let mut hi = Vec3::splat(f32::MIN);
            for p in &parts {
                lo = lo.min(p.at - p.size / 2.0);
                hi = hi.max(p.at + p.size / 2.0);
            }
            if parts.is_empty() {
                Vec3::new(0.4, 1.0, 0.4)
            } else {
                let w = (hi.x - lo.x).max(hi.z - lo.z).min(2.8) / 2.0 * 0.85;
                Vec3::new(w, hi.y.clamp(0.3, 4.0), w)
            }
        }
    };
    let half = half.clamp(Vec3::new(0.1, 0.2, 0.1), Vec3::new(1.5, 4.0, 1.5));
    let home = match def.get::<Value>("home")? {
        Value::String(s) => spot(&s.to_str()?).ok_or_else(|| bad(format!("no spot called \"{}\" (try \"green\", \"paddock\", \"creek\" or \"start\")", s.to_str().map(|s| s.to_string()).unwrap_or_default())))?,
        v => vec_of(&v).unwrap_or_else(|| spot("green").unwrap()),
    };
    Ok(KindDef {
        name,
        half,
        speed: def.get::<Option<f32>>("speed")?.unwrap_or(2.0).clamp(0.0, 20.0),
        rideable: def.get::<Option<bool>>("rideable")?.unwrap_or(false),
        seat: def.get::<Option<f32>>("seat")?.unwrap_or(half.y).clamp(0.2, 5.0),
        count: def.get::<Option<u32>>("count")?.unwrap_or(1).min(32),
        home,
        parts,
    })
}

fn parse_knobs(lua: &Lua, spec: &Table, saved: &BTreeMap<String, KnobValue>, out: &mut Vec<Knob>) -> mlua::Result<Table> {
    let values = lua.create_table()?;
    for pair in spec.pairs::<String, Table>() {
        let (key, t) = pair?;
        let first: Value = t.get(1)?;
        let label = t.get::<Option<String>>("label")?.unwrap_or_else(|| key.clone());
        let order = t.get::<Option<i64>>("order")?.unwrap_or(0);
        let (mut value, min, max) = match &first {
            Value::Integer(i) => (KnobValue::Number(*i as f64), t.get::<Option<f64>>("min")?.unwrap_or(0.0), t.get::<Option<f64>>("max")?.unwrap_or(10.0)),
            Value::Number(n) => (KnobValue::Number(*n), t.get::<Option<f64>>("min")?.unwrap_or(0.0), t.get::<Option<f64>>("max")?.unwrap_or(10.0)),
            Value::Boolean(b) => (KnobValue::Toggle(*b), 0.0, 1.0),
            Value::String(s) if parse_color(&s.to_str()?).is_some() => (KnobValue::Color(s.to_str()?.to_string()), 0.0, 0.0),
            _ => return Err(bad(format!("knob \"{key}\" needs a number, true or false, or a colour like \"#4f7a3a\" first"))),
        };
        if let Some(s) = saved.get(&key) {
            if std::mem::discriminant(s) == std::mem::discriminant(&value) {
                value = s.clone();
            }
        }
        if let KnobValue::Number(n) = &mut value {
            *n = n.clamp(min.min(max), max.max(min));
        }
        match &value {
            KnobValue::Number(n) => values.set(key.as_str(), *n)?,
            KnobValue::Toggle(b) => values.set(key.as_str(), *b)?,
            KnobValue::Color(c) => values.set(key.as_str(), c.as_str())?,
        }
        out.push(Knob { key, label, value, min, max, order });
    }
    out.sort_by(|a, b| a.order.cmp(&b.order).then(a.key.cmp(&b.key)));
    Ok(values)
}

impl Vm {
    /// Starts a creation in a new sandbox. An error leaves no VM, and the
    /// caller keeps the last good one running.
    pub fn load(creation: &Creation, saved: &BTreeMap<String, KnobValue>, shared: Arc<Mutex<GameShared>>) -> Result<Vm, String> {
        let started = Instant::now();
        let lua = Lua::new();
        lua.set_memory_limit(MEMORY_LIMIT).map_err(|e| e.to_string())?;
        let deadline = Arc::new(Mutex::new(Instant::now() + LOAD_BUDGET));
        let d = deadline.clone();
        lua.set_interrupt(move |_| {
            if Instant::now() > *d.lock().unwrap() {
                return Err(bad("this creation took too long (is there a loop that never ends?)"));
            }
            Ok(VmState::Continue)
        });
        let state = Arc::new(Mutex::new(LoadState { saved: saved.clone(), ..default() }));
        let waiting = Arc::new(Mutex::new(Waiting::default()));
        let setup = || -> mlua::Result<(Table, Table, Table)> {
            let world = lua.create_table()?;
            let game = lua.create_table()?;
            let base = lua.create_table()?;
            let base_mt = lua.create_table()?;
            base_mt.set("__index", base.clone())?;
            // world.creature: define a kind of creature
            {
                let st = state.clone();
                let base_mt = base_mt.clone();
                world.set(
                    "creature",
                    lua.create_function(move |_, def: Table| {
                        let kind = parse_kind(&def)?;
                        def.set_metatable(Some(base_mt.clone()))?;
                        let mut s = st.lock().unwrap();
                        if s.kinds.iter().any(|(k, _)| k.name == kind.name) {
                            return Err(bad(format!("there's already a creature called \"{}\"", kind.name)));
                        }
                        if s.kinds.len() >= 16 {
                            return Err(bad("a creation can define at most 16 creatures"));
                        }
                        s.kinds.push((kind, Some(def.clone())));
                        Ok(def)
                    })?,
                )?;
            }
            world.set("spot", lua.create_function(|_, name: String| Ok(spot(&name).map(vector)))?)?;
            // world 0.2: timers and world events, kept between ticks
            {
                let w = waiting.clone();
                world.set("after", lua.create_function(move |_, (seconds, f): (f64, Function)| add_timer(&w, seconds, false, f))?)?;
                let w = waiting.clone();
                world.set("every", lua.create_function(move |_, (seconds, f): (f64, Function)| add_timer(&w, seconds, true, f))?)?;
                let w = waiting.clone();
                world.set(
                    "cancel",
                    lua.create_function(move |_, id: Option<u32>| {
                        let mut w = w.lock().unwrap();
                        let n = w.timers.len();
                        w.timers.retain(|t| Some(t.id) != id);
                        Ok(w.timers.len() < n)
                    })?,
                )?;
                let w = waiting.clone();
                world.set(
                    "on",
                    lua.create_function(move |_, (event, f): (String, Function)| {
                        if !EVENTS.contains(&event.as_str()) {
                            return Err(bad(format!("world.on can't listen for \"{event}\" (it can for: {})", EVENTS.join(", "))));
                        }
                        let mut w = w.lock().unwrap();
                        if w.on.len() >= MAX_HANDLERS {
                            return Err(bad(format!("a creation can give world.on at most {MAX_HANDLERS} handlers")));
                        }
                        w.on.push((event, f));
                        Ok(())
                    })?,
                )?;
            }
            // store 0.1: what this creation keeps in this world
            let store = lua.create_table()?;
            {
                let (sh, me) = (shared.clone(), creation.id.clone());
                store.set(
                    "get",
                    lua.create_function(move |lua, key: String| match sh.lock().unwrap().store.get(&me, &key) {
                        Some(j) => from_json(lua, j),
                        None => Ok(Value::Nil),
                    })?,
                )?;
                let (sh, me) = (shared.clone(), creation.id.clone());
                store.set(
                    "set",
                    lua.create_function(move |_, (key, value): (String, Value)| {
                        if key.is_empty() || key.chars().count() > 64 {
                            return Err(bad("a store key is 1 to 64 letters"));
                        }
                        let j = match &value {
                            Value::Nil => None,
                            v => Some(to_json(v, 0).ok_or_else(|| bad("the store keeps numbers, strings, true or false, vectors, and tables of those"))?),
                        };
                        sh.lock().unwrap().store.set(&me, &key, j).map_err(bad)
                    })?,
                )?;
                let (sh, me) = (shared.clone(), creation.id.clone());
                store.set("keys", lua.create_function(move |_, ()| Ok(sh.lock().unwrap().store.keys(&me)))?)?;
                let (sh, me) = (shared.clone(), creation.id.clone());
                store.set("used", lua.create_function(move |_, ()| Ok(sh.lock().unwrap().store.used(&me)))?)?;
            }
            // tweak { key = { value, min =, max =, label = } }
            let tweak = {
                let st = state.clone();
                lua.create_function(move |lua, spec: Table| {
                    let mut s = st.lock().unwrap();
                    let saved = s.saved.clone();
                    let mut knobs = Vec::new();
                    let t = parse_knobs(lua, &spec, &saved, &mut knobs)?;
                    s.knobs.extend(knobs);
                    Ok(t)
                })?
            };
            let tweak_mod = lua.create_table()?;
            let tweak_mt = lua.create_table()?;
            tweak_mt.set("__call", lua.create_function(move |_, (_, spec): (Value, Table)| tweak.call::<Table>(spec))?)?;
            tweak_mod.set_metatable(Some(tweak_mt))?;
            // require: the API's modules, or another file in the creation
            let files = creation.files.clone();
            let cache = lua.create_table()?;
            let w2 = world.clone();
            let g2 = game.clone();
            let req = lua.create_function(move |lua, name: String| {
                if let Some(m) = name.strip_prefix("@potluck/") {
                    let (module, version) = m.split_once('@').unwrap_or((m, ""));
                    let Some((_, major, minor)) = MODULES.iter().find(|(n, _, _)| *n == module) else {
                        let all: Vec<&str> = MODULES.iter().map(|m| m.0).collect();
                        return Err(bad(format!("there's no module called @potluck/{module} (there are: {})", all.join(", "))));
                    };
                    if !version.is_empty() {
                        let mut it = version.split('.');
                        let want_major: u32 = it.next().and_then(|v| v.parse().ok()).ok_or_else(|| bad(format!("can't read the version in \"{name}\"")))?;
                        let want_minor: u32 = it.next().and_then(|v| v.parse().ok()).unwrap_or(0);
                        if want_major != *major || want_minor > *minor {
                            return Err(bad(format!("this creation needs @potluck/{module} {version}, and this game has {major}.{minor}: update Potluck")));
                        }
                    }
                    return Ok(match module {
                        "world" => Value::Table(w2.clone()),
                        "game" => Value::Table(g2.clone()),
                        "store" => Value::Table(store.clone()),
                        _ => Value::Table(tweak_mod.clone()),
                    });
                }
                let file = name.trim_start_matches("./");
                let file = if file.ends_with(".luau") { file.to_string() } else { format!("{file}.luau") };
                if let Some(v) = cache.get::<Option<Value>>(file.as_str())? {
                    return Ok(v);
                }
                let src = files.get(&file).ok_or_else(|| bad(format!("there's no file called {file} in this creation")))?;
                let v: Value = lua.load(src.as_str()).set_name(format!("@{file}")).eval()?;
                let v = if v.is_nil() { Value::Boolean(true) } else { v };
                cache.set(file.as_str(), v.clone())?;
                Ok(v)
            })?;
            lua.globals().set("require", req)?;
            let st = state.clone();
            lua.globals().set(
                "print",
                lua.create_function(move |lua, args: mlua::Variadic<Value>| {
                    let line: Vec<String> = args.iter().map(|v| lua.coerce_string(v.clone()).ok().flatten().map(|s| s.to_string_lossy()).unwrap_or_else(|| format!("{v:?}"))).collect();
                    let mut s = st.lock().unwrap();
                    s.log.push(line.join(" "));
                    if s.log.len() > 200 {
                        s.log.remove(0);
                    }
                    Ok(())
                })?,
            )?;
            // Creations load code only through require, from their own
            // files, and keep to their own environment.
            for name in ["loadstring", "getfenv", "setfenv", "collectgarbage", "newproxy"] {
                lua.globals().set(name, Value::Nil)?;
            }
            lua.sandbox(true)?;
            Ok((world, base, game))
        };
        let (world, base, game) = setup().map_err(|e| e.to_string())?;
        let main = creation.manifest.main.clone();
        let src = creation.files.get(&main).ok_or_else(|| format!("{} has no {main}", creation.manifest.name))?;
        let mut vm = Vm {
            lua,
            world,
            base,
            game,
            shared,
            waiting,
            state: state.clone(),
            kinds: vec![],
            kind_tables: HashMap::new(),
            instances: HashMap::new(),
            knobs: vec![],
            log: vec![],
            error: None,
            errors: 0,
            paused: false,
            deadline,
            rng: Rng::new(crate::noise::seed_of(&creation.id)),
            load_ms: 0.0,
        };
        // the top level runs with the read-only parts of the world reachable
        let mut scratch_vox = Voxels::empty(0);
        let mut scratch_herd = Herd::default();
        let mut out = Outbox::default();
        let mut log = Vec::new();
        let r = vm.session(&mut scratch_vox, &mut scratch_herd, &[], &mut out, &creation.id, 0.0, &mut log, |lua, _| {
            lua.load(src.as_str()).set_name(format!("@{main}")).exec()
        });
        r?;
        let mut s = state.lock().unwrap();
        for (k, t) in s.kinds.drain(..) {
            if let Some(t) = t {
                vm.kind_tables.insert(k.name.clone(), t);
            }
            vm.kinds.push(k);
        }
        vm.knobs = std::mem::take(&mut s.knobs);
        vm.log.extend(std::mem::take(&mut s.log));
        vm.log.extend(log);
        vm.load_ms = started.elapsed().as_secs_f32() * 1000.0;
        Ok(vm)
    }

    /// Runs `f` with the API's world functions bound to this tick's state.
    #[allow(clippy::too_many_arguments)]
    fn session<R>(
        &mut self,
        voxels: &mut Voxels,
        herd: &mut Herd,
        players: &[PlayerInfo],
        out: &mut Outbox,
        creation: &str,
        time: f64,
        log: &mut Vec<String>,
        f: impl FnOnce(&Lua, &Self) -> mlua::Result<R>,
    ) -> Result<R, String> {
        let budget = if time == 0.0 { LOAD_BUDGET } else { TICK_BUDGET };
        // generous while the API is set up; the creation's own time starts below
        *self.deadline.lock().unwrap() = Instant::now() + LOAD_BUDGET;
        let kinds = self.kinds.clone();
        let mut rng = self.rng.clone();
        let ctx = RefCell::new(Ctx { voxels, herd, players, out, creation, kinds: &kinds, time, rng: &mut rng, log });
        let world = self.world.clone();
        let base = self.base.clone();
        let game = self.game.clone();
        let shared = self.shared.clone();
        let kind_tables = self.kind_tables.clone();
        let r = self.lua.scope(|s| {
            let c = &ctx;
            let id_of = |t: &Table| -> mlua::Result<u64> { t.raw_get::<Option<u64>>("id")?.ok_or_else(|| bad("call this with a colon: self:walk_to(...)")) };
            // ---- world ----
            world.set("block", s.create_function(move |_, p: Value| {
                let p = vec_of(&p).ok_or_else(|| bad("world.block needs a position"))?;
                Ok(blocks::block(c.borrow().voxels.at(p.floor().as_ivec3())).name)
            })?)?;
            world.set("set_block", s.create_function(move |_, (p, name): (Value, String)| {
                let p = vec_of(&p).ok_or_else(|| bad("world.set_block needs a position"))?;
                let id = blocks::by_name(&name).ok_or_else(|| bad(format!("there's no block called \"{name}\"")))?;
                Ok(c.borrow_mut().voxels.set(p.floor().as_ivec3(), id))
            })?)?;
            world.set("top", s.create_function(move |_, (x, z): (f32, f32)| Ok(c.borrow().voxels.top(x.floor() as i32, z.floor() as i32)))?)?;
            world.set("find_block", s.create_function(move |_, (p, radius, name): (Value, f32, String)| {
                let p = vec_of(&p).ok_or_else(|| bad("world.find_block needs a position"))?.floor().as_ivec3();
                let id = blocks::by_name(&name).ok_or_else(|| bad(format!("there's no block called \"{name}\"")))?;
                let r = radius.clamp(0.0, 16.0) as i32;
                let ctx = c.borrow();
                let mut best: Option<(i32, IVec3)> = None;
                for y in -r..=r {
                    for z in -r..=r {
                        for x in -r..=r {
                            let q = p + IVec3::new(x, y, z);
                            if ctx.voxels.at(q) == id {
                                let d = x * x + y * y + z * z;
                                if best.is_none_or(|(b, _)| d < b) {
                                    best = Some((d, q));
                                }
                            }
                        }
                    }
                }
                Ok(best.map(|(_, q)| vector(q.as_vec3() + Vec3::splat(0.5))))
            })?)?;
            world.set("players", s.create_function(move |lua, ()| {
                let t = lua.create_table()?;
                for (i, p) in c.borrow().players.iter().enumerate() {
                    t.raw_set(i + 1, player_table(lua, p)?)?;
                }
                Ok(t)
            })?)?;
            world.set("nearest_player", s.create_function(move |lua, (p, radius): (Value, Option<f32>)| {
                let p = vec_of(&p).ok_or_else(|| bad("world.nearest_player needs a position"))?;
                let r = radius.unwrap_or(1e9);
                let ctx = c.borrow();
                let best = ctx.players.iter().map(|q| (q.pos.distance(p), q)).filter(|(d, _)| *d <= r).min_by(|a, b| a.0.total_cmp(&b.0));
                match best {
                    Some((_, q)) => Ok(Value::Table(player_table(lua, q)?)),
                    None => Ok(Value::Nil),
                }
            })?)?;
            world.set("say", s.create_function(move |_, text: String| {
                c.borrow_mut().out.toasts.push(text.chars().take(120).collect());
                Ok(())
            })?)?;
            world.set("time", s.create_function(move |_, ()| Ok(c.borrow().time))?)?;
            world.set("random", s.create_function(move |_, (lo, hi): (Option<f64>, Option<f64>)| {
                let r = c.borrow_mut().rng.next() as f64;
                Ok(match (lo, hi) {
                    (Some(a), Some(b)) => a + r * (b - a),
                    (Some(b), None) => r * b,
                    _ => r,
                })
            })?)?;
            let kt = kind_tables.clone();
            world.set("spawn", s.create_function(move |_, (kind, p): (Table, Value)| {
                let name: String = kind.get("name")?;
                let p = vec_of(&p).ok_or_else(|| bad("world.spawn needs a position"))?;
                let mut ctx = c.borrow_mut();
                let def = ctx.kinds.iter().find(|k| k.name == name).cloned().ok_or_else(|| bad("world.spawn needs a creature made with world.creature"))?;
                let creation = ctx.creation.to_string();
                if ctx.herd.bodies.values().filter(|b| b.creation == creation).count() >= 64 {
                    return Err(bad("a creation can have at most 64 creatures at once"));
                }
                let _ = &kt;
                let mut p = p;
                if ctx.voxels.at(p.floor().as_ivec3()) != crate::blocks::AIR && ctx.voxels.at(p.floor().as_ivec3()) != crate::blocks::WATER {
                    p.y = ctx.voxels.top(p.x.floor() as i32, p.z.floor() as i32) as f32 + 1.2;
                }
                Ok(ctx.herd.spawn(&creation, &def, p))
            })?)?;
            world.set("creatures", s.create_function(move |_, kind: Option<Table>| {
                let name: Option<String> = match kind { Some(k) => k.get("name")?, None => None };
                let ctx = c.borrow();
                let ids: Vec<u64> = ctx.herd.bodies.values().filter(|b| b.creation == ctx.creation && name.as_ref().is_none_or(|n| *n == b.kind)).map(|b| b.id).collect();
                Ok(ids)
            })?)?;
            // ---- a creature's own methods (self:...) ----
            base.set("walk_to", s.create_function(move |_, (me, p, speed): (Table, Value, Option<f32>)| {
                let id = id_of(&me)?;
                let p = vec_of(&p).ok_or_else(|| bad("walk_to needs a position"))?;
                if let Some(b) = c.borrow_mut().herd.bodies.get_mut(&id) {
                    b.goal = Some(p);
                    b.wander = false;
                    if let Some(s) = speed {
                        b.speed = s.clamp(0.0, 20.0);
                    }
                }
                Ok(())
            })?)?;
            base.set("wander", s.create_function(move |_, (me, speed): (Table, Option<f32>)| {
                let id = id_of(&me)?;
                if let Some(b) = c.borrow_mut().herd.bodies.get_mut(&id) {
                    b.wander = true;
                    if let Some(s) = speed {
                        b.speed = s.clamp(0.0, 20.0);
                    }
                }
                Ok(())
            })?)?;
            base.set("stop", s.create_function(move |_, me: Table| {
                let id = id_of(&me)?;
                if let Some(b) = c.borrow_mut().herd.bodies.get_mut(&id) {
                    b.goal = None;
                    b.wander = false;
                }
                Ok(())
            })?)?;
            base.set("hop", s.create_function(move |_, (me, power): (Table, Option<f32>)| {
                let id = id_of(&me)?;
                if let Some(b) = c.borrow_mut().herd.bodies.get_mut(&id) {
                    if b.on_ground {
                        b.vel.y = 7.0 * power.unwrap_or(1.0).clamp(0.0, 3.0);
                    }
                }
                Ok(())
            })?)?;
            base.set("face", s.create_function(move |_, (me, p): (Table, Value)| {
                let id = id_of(&me)?;
                let p = vec_of(&p).ok_or_else(|| bad("face needs a position"))?;
                if let Some(b) = c.borrow_mut().herd.bodies.get_mut(&id) {
                    let d = p - b.pos;
                    if d.x.abs() + d.z.abs() > 0.01 {
                        b.face = Some(d.x.atan2(d.z) + std::f32::consts::PI);
                    }
                }
                Ok(())
            })?)?;
            base.set("say", s.create_function(move |_, (me, text): (Table, String)| {
                let id = id_of(&me)?;
                if let Some(b) = c.borrow_mut().herd.bodies.get_mut(&id) {
                    b.says = Some((text.chars().take(80).collect(), 3.0));
                }
                Ok(())
            })?)?;
            base.set("distance_to", s.create_function(move |_, (me, p): (Table, Value)| {
                let id = id_of(&me)?;
                let p = vec_of(&p).ok_or_else(|| bad("distance_to needs a position"))?;
                Ok(c.borrow().herd.bodies.get(&id).map(|b| b.pos.distance(p)).unwrap_or(f32::MAX))
            })?)?;
            base.set("throw_rider", s.create_function(move |_, (me, force): (Table, Option<f32>)| {
                let id = id_of(&me)?;
                let mut ctx = c.borrow_mut();
                let Some(b) = ctx.herd.bodies.get_mut(&id) else { return Ok(false) };
                let Some(rider) = b.rider.take() else { return Ok(false) };
                let f = force.unwrap_or(5.0).clamp(0.0, 30.0);
                let back = Vec3::new(b.yaw.sin(), 0.0, b.yaw.cos());
                let v = Vec3::Y * (4.0 + f * 0.55) + back * (1.0 + f * 0.45);
                ctx.out.throws.push((rider, v));
                Ok(true)
            })?)?;
            base.set("remove", s.create_function(move |_, me: Table| {
                let id = id_of(&me)?;
                c.borrow_mut().herd.bodies.remove(&id);
                Ok(())
            })?)?;
            // ---- game ----
            let player_id = |p: &Value| -> mlua::Result<u32> {
                match p {
                    Value::Table(t) => t.get::<Option<u32>>("id")?.ok_or_else(|| bad("that isn't a player")),
                    Value::Integer(i) => Ok(*i as u32),
                    Value::Number(n) => Ok(*n as u32),
                    _ => Err(bad("that isn't a player")),
                }
            };
            game.set("score", s.create_function(move |_, (board, player, value, opts): (String, Value, f64, Option<Table>)| {
                let pid = player_id(&player)?;
                if board.trim().is_empty() || board.len() > 40 {
                    return Err(bad("a board's name is 1 to 40 letters"));
                }
                if !value.is_finite() {
                    return Err(bad("a score is a number"));
                }
                let lower = match opts {
                    Some(o) => o.get::<Option<bool>>("lower")?.unwrap_or(false),
                    None => false,
                };
                let mut ctx = c.borrow_mut();
                if ctx.out.scores.len() >= 256 {
                    return Err(bad("too many scores at once"));
                }
                let creation = ctx.creation.to_string();
                ctx.out.scores.push((creation, board, lower, pid, value));
                Ok(())
            })?)?;
            let sh = shared.clone();
            game.set("board", s.create_function(move |lua, (board, n): (String, Option<usize>)| {
                let creation = c.borrow().creation.to_string();
                let top = sh.lock().unwrap().boards.top(&creation, &board, n.unwrap_or(10).clamp(1, 100));
                let t = lua.create_table()?;
                for (i, sc) in top.iter().enumerate() {
                    let row = lua.create_table()?;
                    row.set("name", sc.name.clone())?;
                    row.set("value", sc.value)?;
                    t.raw_set(i + 1, row)?;
                }
                Ok(t)
            })?)?;
            let sh = shared.clone();
            game.set("links", s.create_function(move |lua, ()| {
                let t = lua.create_table()?;
                for (i, l) in sh.lock().unwrap().links.iter().enumerate() {
                    t.raw_set(i + 1, l.clone())?;
                }
                Ok(t)
            })?)?;
            let sh = shared.clone();
            game.set("send", s.create_function(move |_, (player, link, note): (Value, String, Option<String>)| {
                let pid = player_id(&player)?;
                if !sh.lock().unwrap().links.contains(&link) {
                    return Err(bad(format!("this world has no link called \"{link}\"")));
                }
                let mut ctx = c.borrow_mut();
                if ctx.out.sends.len() >= 64 {
                    return Err(bad("too many players sent at once"));
                }
                ctx.out.sends.push((pid, link, note.unwrap_or_default().chars().take(120).collect()));
                Ok(())
            })?)?;
            let _ = &kind_tables;
            *self.deadline.lock().unwrap() = Instant::now() + budget;
            f(&self.lua, self)
        });
        self.rng = rng;
        // what it printed, in Look inside
        let printed = std::mem::take(&mut self.state.lock().unwrap().log);
        self.log.extend(printed);
        if self.log.len() > 200 {
            let n = self.log.len() - 200;
            self.log.drain(..n);
        }
        r.map_err(|e| tidy_error(&e))
    }

    /// The Lua table for a creature, made the first time it's needed.
    fn instance(&mut self, b: &Body) -> mlua::Result<(Table, bool)> {
        if let Some(t) = self.instances.get(&b.id) {
            return Ok((t.clone(), false));
        }
        let kind = self.kind_tables.get(&b.kind).ok_or_else(|| bad("no such kind"))?;
        let t = self.lua.create_table()?;
        let mt = self.lua.create_table()?;
        mt.set("__index", kind.clone())?;
        t.set_metatable(Some(mt))?;
        t.raw_set("id", b.id)?;
        self.instances.insert(b.id, t.clone());
        Ok((t, true))
    }

    fn fill(&self, t: &Table, b: &Body, players: &[PlayerInfo]) -> mlua::Result<()> {
        t.raw_set("position", vector(b.pos))?;
        t.raw_set("kind", b.kind.as_str())?;
        let rider = match b.rider.and_then(|r| players.iter().find(|p| p.id == r)) {
            Some(p) => Value::Table(player_table(&self.lua, p)?),
            None => Value::Nil,
        };
        t.raw_set("rider", rider)?;
        Ok(())
    }

    fn failed(&mut self, name: &str, e: String, out: &mut Outbox) {
        if self.error.as_deref() != Some(e.as_str()) {
            out.toasts.push(format!("{name} has a problem: {e}"));
        }
        self.log.push(format!("error: {e}"));
        self.error = Some(e);
        self.errors += 1;
        if self.errors > 30 {
            self.paused = true;
            out.toasts.push(format!("{name} is paused after too many errors. Look inside to see why."));
        }
    }

    /// The creatures' own fields, to carry over a reload.
    pub fn memories(&self) -> HashMap<u64, serde_json::Value> {
        let mut m = HashMap::new();
        for (id, t) in &self.instances {
            let mut o = serde_json::Map::new();
            for (k, v) in t.pairs::<String, Value>().flatten() {
                if RESERVED.contains(&k.as_str()) {
                    continue;
                }
                if let Some(j) = to_json(&v, 0) {
                    o.insert(k, j);
                }
            }
            m.insert(*id, serde_json::Value::Object(o));
        }
        m
    }

    pub fn restore(&mut self, herd: &Herd, creation: &str, mem: &HashMap<u64, serde_json::Value>) {
        for b in herd.bodies.values().filter(|b| b.creation == creation) {
            let Some(j) = mem.get(&b.id) else { continue };
            let Ok((t, _)) = self.instance(b) else { continue };
            if let serde_json::Value::Object(o) = j {
                for (k, v) in o {
                    if let Ok(v) = from_json(&self.lua, v) {
                        let _ = t.raw_set(k.as_str(), v);
                    }
                }
            }
        }
    }
}

fn tidy_error(e: &mlua::Error) -> String {
    let s = match e {
        mlua::Error::CallbackError { cause, .. } => return tidy_error(cause),
        mlua::Error::RuntimeError(s) | mlua::Error::SyntaxError { message: s, .. } => s.clone(),
        mlua::Error::MemoryError(_) => "this creation used too much memory".into(),
        other => other.to_string(),
    };
    // keep the first line, which says where it went wrong
    s.lines().next().unwrap_or("").trim().to_string()
}

impl Scripts {
    pub fn start(&mut self, creation: &Creation, herd: &mut Herd) -> Result<(), String> {
        let saved = creation.knobs.clone();
        let mut vm = Vm::load(creation, &saved, self.shared.clone())?;
        // keep what the creatures were doing across a reload
        if let Some(old) = self.vms.remove(&creation.id) {
            let mem = old.memories();
            vm.restore(herd, &creation.id, &mem);
        }
        // creatures of kinds that are gone go too; the rest take the new look
        herd.bodies.retain(|_, b| b.creation != creation.id || vm.kinds.iter().any(|k| k.name == b.kind));
        for b in herd.bodies.values_mut().filter(|b| b.creation == creation.id) {
            if let Some(k) = vm.kinds.iter().find(|k| k.name == b.kind) {
                b.restyle(k);
            }
        }
        self.vms.insert(creation.id.clone(), vm);
        Ok(())
    }

    pub fn stop(&mut self, id: &str, herd: &mut Herd) {
        self.vms.remove(id);
        herd.bodies.retain(|_, b| b.creation != id);
    }

    /// Calls a creature's handler for something a player did to it.
    pub fn event(&mut self, voxels: &mut Voxels, herd: &mut Herd, body: u64, event: &str, player: &PlayerInfo) {
        let Some(b) = herd.bodies.get(&body).cloned() else { return };
        let players = self.players.clone();
        let time = self.time;
        let Some(vm) = self.vms.get_mut(&b.creation) else { return };
        if vm.paused {
            return;
        }
        let Ok((t, _)) = vm.instance(&b) else { return };
        let _ = vm.fill(&t, &b, &players);
        let mut log = Vec::new();
        let p = player.clone();
        let creation = b.creation.clone();
        let r = vm.session(voxels, herd, &players, &mut self.out, &creation, time.max(0.001), &mut log, |lua, _| {
            if let Some(f) = t.get::<Option<Function>>(event)? {
                f.call::<()>((t.clone(), player_table(lua, &p)?))?;
            }
            Ok(())
        });
        vm.log.extend(log);
        if let Err(e) = r {
            let name = b.kind.clone();
            vm.failed(&name, e, &mut self.out);
        }
    }

    /// Ticks every creation's creatures, ten times a second.
    pub fn tick(&mut self, voxels: &mut Voxels, herd: &mut Herd, dt: f32) {
        self.time += dt as f64;
        self.tick_acc += dt;
        if self.tick_acc < 0.1 {
            return;
        }
        let step = self.tick_acc;
        self.tick_acc = 0.0;
        let players = self.players.clone();
        let time = self.time;
        let joined: Vec<PlayerInfo> = players.iter().filter(|p| !self.seen.iter().any(|q| q.id == p.id)).cloned().collect();
        let left: Vec<PlayerInfo> = self.seen.iter().filter(|q| !players.iter().any(|p| p.id == q.id)).cloned().collect();
        self.seen = players.clone();
        for (id, vm) in self.vms.iter_mut() {
            if vm.paused {
                continue;
            }
            // keep each kind's numbers up
            for k in vm.kinds.clone() {
                let have = herd.bodies.values().filter(|b| b.creation == *id && b.kind == k.name).count() as u32;
                for i in have..k.count {
                    let a = i as f32 * 2.4;
                    let mut at = k.home + Vec3::new(a.cos() * 2.5, 2.0, a.sin() * 2.5);
                    // never inside the ground
                    at.y = at.y.max(voxels.top(at.x.floor() as i32, at.z.floor() as i32) as f32 + 1.2);
                    herd.spawn(id, &k, at);
                }
            }
            vm.instances.retain(|i, _| herd.bodies.contains_key(i));
            let bodies: Vec<Body> = herd.bodies.values().filter(|b| b.creation == *id).cloned().collect();
            let mut calls = Vec::new();
            for b in &bodies {
                match vm.instance(b) {
                    Ok((t, new)) => {
                        let _ = vm.fill(&t, b, &players);
                        calls.push((t, new));
                    }
                    Err(_) => continue,
                }
            }
            let mut log = Vec::new();
            let waiting = vm.waiting.clone();
            let r = vm.session(voxels, herd, &players, &mut self.out, id, time, &mut log, |lua, _| {
                // who came and went
                if !joined.is_empty() || !left.is_empty() {
                    let on = waiting.lock().unwrap().on.clone();
                    for (event, list) in [("player_joined", &joined), ("player_left", &left)] {
                        for p in list.iter() {
                            for (_, f) in on.iter().filter(|(e, _)| e == event) {
                                f.call::<()>(player_table(lua, p)?)?;
                            }
                        }
                    }
                }
                // timers that are due, soonest first
                let due: Vec<u32> = {
                    let mut w = waiting.lock().unwrap();
                    for t in w.timers.iter_mut() {
                        t.left -= step as f64;
                    }
                    let mut due: Vec<(f64, u32)> = w.timers.iter().filter(|t| t.left <= 1e-6).map(|t| (t.left, t.id)).collect();
                    due.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
                    due.into_iter().map(|(_, id)| id).collect()
                };
                for timer in due {
                    let f = {
                        let mut w = waiting.lock().unwrap();
                        // a timer before it may have cancelled it
                        let Some(i) = w.timers.iter().position(|t| t.id == timer) else { continue };
                        let t = &mut w.timers[i];
                        let f = t.f.clone();
                        match t.every {
                            Some(every) => {
                                t.left += every;
                                if t.left <= 1e-6 {
                                    t.left = every;
                                }
                            }
                            None => {
                                w.timers.remove(i);
                            }
                        }
                        f
                    };
                    f.call::<()>(())?;
                }
                for (t, new) in &calls {
                    if *new {
                        if let Some(f) = t.get::<Option<Function>>("spawned")? {
                            f.call::<()>(t.clone())?;
                        }
                    }
                    if let Some(f) = t.get::<Option<Function>>("tick")? {
                        f.call::<()>((t.clone(), step))?;
                    }
                }
                Ok(())
            });
            vm.log.extend(log);
            if vm.log.len() > 200 {
                let n = vm.log.len() - 200;
                vm.log.drain(..n);
            }
            match r {
                Ok(()) => {
                    if vm.errors > 0 {
                        vm.errors -= 1;
                    }
                }
                Err(e) => {
                    let name = bodies.first().map(|b| b.kind.clone()).unwrap_or_else(|| id.clone());
                    vm.failed(&name, e, &mut self.out);
                }
            }
        }
    }
}
