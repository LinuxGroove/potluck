//! Creations' own tests (`@potluck/test`): a creation's `test.luau` gives
//! `test.case` a name and a function for each case, and the game runs each
//! case in a world of its own: a copy of the world with only this creation
//! in it, and players who are only there for the test. A case runs as a
//! coroutine in the creation's sandbox, so it sees everything the creation
//! sees; whenever it asks for something only the game can do (time passing,
//! a player putting a block down, using a creature, pressing a button) it
//! yields, the harness does it here, and the case carries on. Nothing a test
//! does reaches the world anyone plays in.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use bevy::prelude::*;
use mlua::thread::ThreadStatus;
use mlua::{Function, MultiValue, Table, Value};

use crate::blocks;
use crate::creations::Creation;
use crate::creatures::{self, Herd};
use crate::noise::Rng;
use crate::script::{self, Note, PlayerInfo, Scripts};
use crate::world::Voxels;

/// The file a creation keeps its tests in.
pub const FILE: &str = "test.luau";
/// One step of world time: the same as the game's ticks are cut into.
const STEP: f32 = 0.05;
/// The most world time one case may pass, and how long it may take.
const MOST_PLAY: f32 = 600.0;
const MOST_TIME: Duration = Duration::from_secs(30);
/// Test players' ids, out of the way of real players'.
const FIRST_PLAYER: u32 = 9000;

/// `@potluck/test` 0.1, in every creation's sandbox. Its functions that need
/// the game yield to the harness, so they work only while tests run.
pub const TEST_MODULE: &str = r#"
local test = {}
-- what the harness and the module share: the cases, and what the harness
-- says about the world before each part of a case runs
local h = { cases = {}, running = false, kinds = {}, bodies = {}, health = {}, shown = {}, asked = {}, heard = {}, sent = {} }
local function ask(op, ...)
	if not h.running or not coroutine.isyieldable() then
		error("test." .. op .. " works only in a case, while the creation's tests run", 3)
	end
	return coroutine.yield(op, ...)
end
local function seen(op)
	if not h.running then
		error("test." .. op .. " works only in a case, while the creation's tests run", 3)
	end
end
local function pid(player, op)
	local id = if type(player) == "table" then player.id else player
	if type(id) ~= "number" then
		error("test." .. op .. " needs a player from test.player", 3)
	end
	return id
end
function test.case(name, fn)
	if type(name) ~= "string" or type(fn) ~= "function" then
		error("test.case needs a name and a function: test.case(\"it hops\", function() ... end)", 2)
	end
	if #h.cases >= 64 then
		error("a creation has at most 64 cases", 2)
	end
	table.insert(h.cases, { name = name, fn = fn })
end
-- what a player does, and time passing: these hand over to the game
function test.wait(seconds) ask("wait", seconds) end
function test.player(name, opts) return ask("player", name, opts) end
function test.move(player, at) return ask("move", player, at) end
function test.place(player, at, block, facing) ask("place", player, at, block, facing) end
function test.dig(player, at) return ask("dig", player, at) end
function test.use_block(player, at) ask("use_block", player, at) end
function test.use_item(player, aim) ask("use_item", player, aim) end
function test.use(player, creature) ask("event", "used", player, creature) end
function test.hit(player, creature) ask("event", "hit", player, creature) end
function test.ride(player, creature) ask("ride", player, creature) end
function test.get_off(player) ask("get_off", player) end
function test.press(player, action) ask("press", player, action, true) end
function test.release(player, action) ask("press", player, action, false) end
function test.answer(player, choice) ask("answer", player, choice) end
function test.hold(player, block) ask("hold", player, block) end
function test.from_half(player, topic, value) ask("from_half", player, topic, value) end
function test.danger(level) ask("danger", level) end
-- looking: these only read what the harness said
function test.kind(name)
	seen("kind")
	local k = h.kinds[name]
	if k == nil then
		local all = {}
		for n in h.kinds do
			table.insert(all, n)
		end
		table.sort(all)
		error(string.format("test.kind: there's no kind called %q (there are: %s)", tostring(name), table.concat(all, ", ")), 2)
	end
	return k
end
function test.creature(id)
	seen("creature")
	return h.bodies[if type(id) == "table" then id.id else id]
end
function test.health(player)
	seen("health")
	return h.health[pid(player, "health")]
end
function test.shown(player, id)
	seen("shown")
	local s = h.shown[pid(player, "shown")]
	return s and s[id]
end
function test.asked(player)
	seen("asked")
	return h.asked[pid(player, "asked")]
end
function test.heard(name)
	seen("heard")
	if type(name) ~= "string" then
		error("test.heard needs a sound's name", 2)
	end
	return h.heard[name] or 0
end
function test.sent(player, topic)
	seen("sent")
	if type(topic) ~= "string" then
		error("test.sent needs a player and a topic", 2)
	end
	local s = h.sent[pid(player, "sent")]
	local m = s and s[topic]
	if m then
		return m.value, m.times
	end
	return nil, 0
end
function test.check(ok, message)
	if not ok then
		error(message or "a check failed", 2)
	end
	return ok
end
function test.same(got, want, message)
	if got ~= want then
		error(string.format("%s: got %s, wanted %s", message or "not the same", tostring(got), tostring(want)), 2)
	end
end
return test, h
"#;

/// How one case went.
#[derive(Debug, Clone)]
pub struct Case {
    pub name: String,
    pub passed: bool,
    /// Why it failed, with the line, when it did.
    pub problem: Option<String>,
    /// World time it passed, in seconds.
    pub played: f32,
    /// The last lines the creation printed during it.
    pub log: Vec<String>,
}

/// How a creation's tests went.
#[derive(Debug, Clone)]
pub struct Report {
    pub name: String,
    /// Why no case could run (no test.luau, or it didn't load).
    pub problem: Option<String>,
    pub cases: Vec<Case>,
}

impl Report {
    pub fn passed(&self) -> bool {
        self.problem.is_none() && self.cases.iter().all(|c| c.passed)
    }

    /// Plain text, as the dev bridge and `potluck-server --test` show it.
    pub fn text(&self) -> String {
        if let Some(p) = &self.problem {
            return format!("{}: {p}", self.name);
        }
        let passed = self.cases.iter().filter(|c| c.passed).count();
        let mut s = format!("{}: {passed} of {} cases passed\n", self.name, self.cases.len());
        for c in &self.cases {
            if c.passed {
                s.push_str(&format!("  pass  {} ({:.1} s of play)\n", c.name, c.played));
            } else {
                s.push_str(&format!("  FAIL  {}: {}\n", c.name, c.problem.as_deref().unwrap_or("it failed")));
                for l in &c.log {
                    s.push_str(&format!("        printed: {l}\n"));
                }
            }
        }
        s.trim_end().to_string()
    }
}

/// Runs a creation's tests, each case in its own copy of `world`.
pub fn run(creation: &Creation, world: &Voxels) -> Report {
    run_in(creation, world.seed, &world.encode())
}

/// The same, with the world as `Voxels::encode` keeps it.
pub fn run_in(creation: &Creation, seed: u32, bytes: &[u8]) -> Report {
    let name = creation.manifest.name.clone();
    let Some(src) = creation.files.get(FILE) else {
        return Report { name, problem: Some(format!("it has no {FILE}")), cases: vec![] };
    };
    let fresh = || Voxels::decode(seed, bytes).unwrap_or_else(|| Voxels::empty(seed));
    // learn its cases' names
    let names = match Harness::new(creation, fresh(), src) {
        Ok(h) => h.names(),
        Err(e) => return Report { name, problem: Some(e), cases: vec![] },
    };
    if names.is_empty() {
        return Report { name, problem: Some(format!("{FILE} has no cases: give test.case a name and a function")), cases: vec![] };
    }
    let cases = names
        .iter()
        .enumerate()
        .map(|(i, case)| match Harness::new(creation, fresh(), src) {
            Ok(mut h) => h.case(i, case),
            Err(e) => Case { name: case.clone(), passed: false, problem: Some(e), played: 0.0, log: vec![] },
        })
        .collect();
    Report { name, problem: None, cases }
}

/// One case's world: the creation running alone, with its test file loaded.
struct Harness {
    id: String,
    voxels: Voxels,
    herd: Herd,
    scripts: Scripts,
    rng: Rng,
    played: f32,
    /// Test players' health, which survival players' games keep in play.
    health: BTreeMap<u32, f32>,
    /// The sounds the creation played since the case began, by the names it
    /// plays them by (`test.heard`).
    heard: BTreeMap<String, u32>,
    /// The last thing the creation sent each player's client half on each
    /// topic since the case began (`test.sent`, test 0.3).
    sent: BTreeMap<u32, BTreeMap<String, (serde_json::Value, u32)>>,
}

/// Why a case stops early.
enum Stop {
    /// A mistake in the test, said plainly.
    Problem(String),
}

impl Harness {
    fn new(creation: &Creation, voxels: Voxels, src: &str) -> Result<Harness, String> {
        let mut h = Harness { id: creation.id.clone(), voxels, herd: Herd::default(), scripts: Scripts::default(), rng: Rng::new(7), played: 0.0, health: BTreeMap::new(), heard: BTreeMap::new(), sent: BTreeMap::new() };
        h.scripts.start(creation, &mut h.herd).map_err(|e| format!("it didn't start: {e}"))?;
        let Harness { scripts, voxels, herd, id, .. } = &mut h;
        let vm = scripts.vms.get_mut(id.as_str()).ok_or("it didn't start")?;
        let mut log = Vec::new();
        // the top level runs with a load's time, as main.luau's does
        vm.session(voxels, herd, &[], &mut scripts.out, id, 0.0, &mut log, |lua, _| lua.load(src).set_name(format!("@{FILE}")).exec()).map_err(|e| format!("{FILE} didn't load: {e}"))?;
        Ok(h)
    }

    fn names(&self) -> Vec<String> {
        let vm = &self.scripts.vms[&self.id];
        vm.test_harness.get::<Table>("cases").into_iter().flat_map(|t| t.sequence_values::<Table>().collect::<Vec<_>>()).flatten().map(|c| c.get::<String>("name").unwrap_or_default()).collect()
    }

    fn case(&mut self, index: usize, name: &str) -> Case {
        let started = Instant::now();
        let mut case = Case { name: name.to_string(), passed: false, problem: None, played: 0.0, log: vec![] };
        let co = {
            let vm = &self.scripts.vms[&self.id];
            let f = vm.test_harness.get::<Table>("cases").and_then(|t| t.get::<Table>(index + 1)).and_then(|c| c.get::<Function>("fn"));
            match f.and_then(|f| vm.lua.create_thread(f)) {
                Ok(co) => co,
                Err(e) => {
                    case.problem = Some(e.to_string());
                    return case;
                }
            }
        };
        // the world has been running a tick, so creatures that come by
        // themselves are there
        self.pass(0.1);
        self.played = 0.0;
        self.heard.clear();
        self.sent.clear();
        let log_from = self.scripts.vms[&self.id].log.len();
        self.scripts.vms.get_mut(&self.id).expect("the creation runs").error = None;
        let mut reply = MultiValue::new();
        let outcome: Result<(), String> = loop {
            if started.elapsed() > MOST_TIME {
                break Err(format!("it took longer than {} s to run", MOST_TIME.as_secs()));
            }
            if self.played > MOST_PLAY {
                break Err(format!("it passed more than {MOST_PLAY:.0} s of play"));
            }
            if let Err(e) = self.tell() {
                break Err(e.to_string());
            }
            // resumed in a session, so the world's functions work
            let time = self.scripts.time().max(0.001);
            let players = self.scripts.players.clone();
            let Harness { scripts, voxels, herd, id, .. } = self;
            let vm = scripts.vms.get_mut(id.as_str()).expect("the creation runs");
            let mut log = Vec::new();
            let arg = std::mem::take(&mut reply);
            let r = vm.session(voxels, herd, &players, &mut scripts.out, id, time, &mut log, |_, _| co.resume::<MultiValue>(arg));
            vm.log.extend(log);
            let asked = match r {
                Err(e) => break Err(e),
                Ok(v) => v,
            };
            if co.status() != ThreadStatus::Resumable {
                break self.stopped();
            }
            match self.answer(asked) {
                Ok(v) => reply = v,
                Err(Stop::Problem(e)) => break Err(e),
            }
            // the creation's own code going wrong fails the case too
            if let Err(e) = self.stopped() {
                break Err(e);
            }
        };
        case.played = self.played;
        case.passed = outcome.is_ok();
        case.problem = outcome.err().map(|e| {
            if e.contains("destructed callback") {
                // a scoped world function held over a hand-over to the game
                format!("{e} (a test action ran in the middle of a call to world or a creature, as in world.spawn(kind, test.player().position): give test actions lines of their own)")
            } else {
                e
            }
        });
        if !case.passed {
            let log = &self.scripts.vms[&self.id].log;
            let from = log_from.min(log.len()).max(log.len().saturating_sub(8));
            case.log = log[from..].to_vec();
        }
        case
    }

    /// Tells the module what the world's like now, for `test.kind`,
    /// `test.creature`, `test.shown` and the rest, which only read it.
    /// Counts the sounds played since it last looked, as the game would
    /// send them to players.
    fn listen(&mut self) {
        let mine = format!("{}/", self.id);
        for (_, p) in std::mem::take(&mut self.scripts.shared.lock().unwrap().sounds) {
            let name = p.name.strip_prefix(&mine).unwrap_or(&p.name).to_string();
            *self.heard.entry(name).or_default() += 1;
        }
        let notes = std::mem::take(&mut self.scripts.shared.lock().unwrap().notes);
        for (who, n) in notes.into_iter().filter(|(_, n)| n.creation == self.id) {
            for p in self.scripts.players.iter().filter(|p| who.is_none_or(|w| w == p.id)) {
                let m = self.sent.entry(p.id).or_default().entry(n.topic.clone()).or_insert((serde_json::Value::Null, 0));
                *m = (n.value.clone(), m.1 + 1);
            }
        }
    }

    fn tell(&mut self) -> mlua::Result<()> {
        self.listen();
        let players = self.scripts.players.clone();
        let (shown, asked) = {
            let sh = self.scripts.shared.lock().unwrap();
            let mut shown: BTreeMap<u32, BTreeMap<String, String>> = BTreeMap::new();
            // pieces for everyone first, so a player's own replace them
            let mut pieces: Vec<_> = sh.ui.pieces.iter().filter(|((_, c, _), _)| *c == self.id).collect();
            pieces.sort_by_key(|((who, _, _), _)| who.is_some());
            for ((who, _, id), piece) in pieces {
                for p in players.iter().filter(|p| who.is_none_or(|w| w == p.id)) {
                    shown.entry(p.id).or_default().insert(id.clone(), piece.text.clone());
                }
            }
            let asked: Vec<_> = players.iter().filter_map(|p| sh.ui.panels.get(&p.id).and_then(|q| q.front().cloned()).map(|f| (p.id, f))).collect();
            (shown, asked)
        };
        let Harness { scripts, herd, id, health, heard, sent, .. } = self;
        let vm = scripts.vms.get_mut(id.as_str()).expect("the creation runs");
        let lua = &vm.lua;
        let h = vm.test_harness.clone();
        h.set("running", true)?;
        let kinds = lua.create_table()?;
        for (name, t) in &vm.kind_tables {
            kinds.set(name.as_str(), t.clone())?;
        }
        h.set("kinds", kinds)?;
        let bodies = lua.create_table()?;
        for b in herd.bodies.values().filter(|b| b.creation == *id) {
            // only creatures the creation has met: making one here would
            // keep its `spawned` from being called
            if let Some(t) = vm.known(b.id) {
                vm.fill(&t, b, &players)?;
                bodies.set(b.id, t)?;
            }
        }
        h.set("bodies", bodies)?;
        let healths = lua.create_table()?;
        for p in &players {
            healths.set(p.id, health.get(&p.id).copied().unwrap_or(20.0))?;
        }
        h.set("health", healths)?;
        let s = lua.create_table()?;
        for (pid, pieces) in shown {
            s.set(pid, lua.create_table_from(pieces)?)?;
        }
        h.set("shown", s)?;
        let a = lua.create_table()?;
        for (pid, panel) in asked {
            let t = lua.create_table()?;
            t.set("title", panel.title)?;
            t.set("text", panel.text)?;
            t.set("choices", panel.choices)?;
            a.set(pid, t)?;
        }
        h.set("asked", a)?;
        h.set("heard", lua.create_table_from(heard.iter().map(|(k, v)| (k.as_str(), *v)))?)?;
        let s = lua.create_table()?;
        for (pid, topics) in sent.iter() {
            let t = lua.create_table()?;
            for (topic, (value, times)) in topics {
                // a message with no value still says it was sent
                let m = lua.create_table()?;
                m.set("value", if value.is_null() { Value::Boolean(true) } else { script::from_json(lua, value)? })?;
                m.set("times", *times)?;
                t.set(topic.as_str(), m)?;
            }
            s.set(*pid, t)?;
        }
        h.set("sent", s)
    }

    /// Whether the creation's own code went wrong (a tick, an event).
    fn stopped(&mut self) -> Result<(), String> {
        match self.scripts.vms.get_mut(&self.id).and_then(|vm| vm.error.take()) {
            Some(e) => Err(format!("the creation went wrong: {e}")),
            None => Ok(()),
        }
    }

    /// World time passing, as the game passes it.
    fn pass(&mut self, secs: f32) {
        let steps = (secs.max(0.0) / STEP).round() as usize;
        for _ in 0..steps {
            creatures::step(&mut self.herd, &self.voxels, STEP, &mut self.rng);
            self.scripts.tick(&mut self.voxels, &mut self.herd, STEP);
            // what play would send to players' games, done here
            let _ = self.scripts.pocket_changes();
            for (pid, d, to) in self.scripts.health_changes() {
                let h = self.health.entry(pid).or_insert(20.0);
                *h = if d < 0.0 { (*h + d).max(to.min(*h)) } else { (*h + d).min(20.0) };
                // hurt too badly: the game tells the host where they fell
                // (`player_fell`) and they're whole again (on the hill in
                // play; a test leaves them where they stood)
                if *h <= 0.0 {
                    *h = 20.0;
                    if let Some(p) = self.scripts.players.iter().find(|p| p.id == pid) {
                        let at = p.pos;
                        self.scripts.fell(pid, at);
                    }
                }
            }
            let ids: Vec<u32> = self.scripts.players.iter().map(|p| p.id).collect();
            let _ = self.scripts.ui_changes(&ids);
            self.listen();
            let _ = self.scripts.net_changes(&ids);
            let _ = self.scripts.focus_changes();
            let _ = self.scripts.healing_changes();
            let _ = self.scripts.digging_changes();
            let _ = self.scripts.drops_changes();
            // shots wait in the outbox for the next tick to fly them
            let shots = std::mem::take(&mut self.scripts.out.shots);
            self.scripts.out = Default::default();
            self.scripts.out.shots = shots;
            self.played += STEP;
        }
    }

    fn player(&self, v: Option<&Value>, op: &str) -> Result<PlayerInfo, Stop> {
        let id = match v {
            Some(Value::Table(t)) => t.get::<Option<u32>>("id").ok().flatten(),
            Some(Value::Integer(i)) => Some(*i as u32),
            Some(Value::Number(n)) => Some(*n as u32),
            _ => None,
        };
        id.and_then(|id| self.scripts.players.iter().find(|p| p.id == id).cloned()).ok_or_else(|| Stop::Problem(format!("test.{op} needs a player from test.player")))
    }

    fn body(&self, v: Option<&Value>, op: &str) -> Result<u64, Stop> {
        let id = match v {
            Some(Value::Table(t)) => t.get::<Option<u64>>("id").ok().flatten(),
            Some(Value::Integer(i)) => Some(*i as u64),
            Some(Value::Number(n)) => Some(*n as u64),
            _ => None,
        };
        id.filter(|id| self.herd.bodies.contains_key(id)).ok_or_else(|| Stop::Problem(format!("test.{op} needs one of the creation's creatures, by its id")))
    }

    fn at(v: Option<&Value>, op: &str) -> Result<IVec3, Stop> {
        v.and_then(script::vec_of).map(|p| p.floor().as_ivec3()).ok_or_else(|| Stop::Problem(format!("test.{op} needs a position")))
    }

    fn lua(&self) -> &mlua::Lua {
        &self.scripts.vms[&self.id].lua
    }

    /// Does what a case asked, and says what to hand back to it.
    fn answer(&mut self, asked: MultiValue) -> Result<MultiValue, Stop> {
        let a: Vec<Value> = asked.into_iter().collect();
        let op = a.first().and_then(|v| v.as_string().map(|s| s.to_string_lossy())).unwrap_or_default();
        let arg = |i: usize| a.get(i);
        let fail = |e: mlua::Error| Stop::Problem(e.to_string());
        let nothing = Ok(MultiValue::new());
        let one = |v: Value| Ok(MultiValue::from_vec(vec![v]));
        match op.as_str() {
            "wait" => {
                let secs = match arg(1) {
                    Some(Value::Integer(i)) => *i as f32,
                    Some(Value::Number(n)) => *n as f32,
                    _ => return Err(Stop::Problem("test.wait needs a number of seconds".into())),
                };
                self.pass(secs);
                nothing
            }
            "player" => {
                let n = self.scripts.players.len() as u32;
                let name = arg(1).and_then(|v| v.as_string().map(|s| s.to_string_lossy())).unwrap_or_else(|| format!("Tester {}", n + 1));
                let opts = match arg(2) {
                    Some(Value::Table(t)) => Some(t.clone()),
                    _ => None,
                };
                let at = opts.as_ref().and_then(|o| o.get::<Value>("at").ok()).and_then(|v| script::vec_of(&v)).unwrap_or_else(|| {
                    let g = crate::world::GREEN;
                    Vec3::new(g.x, self.voxels.top(g.x as i32, g.z as i32) as f32 + 1.0, g.z)
                });
                let survival = opts.as_ref().and_then(|o| o.get::<Option<bool>>("survival").ok().flatten()).unwrap_or(false);
                // what they carry, put in their pockets as the game would
                let mut pockets = crate::items::Slots::default();
                if let Some(Ok(carry)) = opts.as_ref().map(|o| o.get::<Option<Table>>("carrying")) {
                    for (thing, count) in carry.into_iter().flat_map(|t| t.pairs::<String, u32>().flatten().collect::<Vec<_>>()) {
                        let t = crate::items::by_name(&thing).ok_or_else(|| Stop::Problem(format!("test.player: there's nothing called \"{thing}\" to carry")))?;
                        pockets.give(t, count);
                    }
                }
                let p = PlayerInfo { id: FIRST_PLAYER + n, name, pos: at };
                self.scripts.players.push(p.clone());
                self.scripts.set_pockets(p.id, survival, &pockets.filled(), 0);
                self.health.insert(p.id, 20.0);
                one(Value::Table(script::player_table(self.lua(), &p).map_err(fail)?))
            }
            "move" => {
                let p = self.player(arg(1), "move")?;
                let to = arg(2).and_then(script::vec_of).ok_or_else(|| Stop::Problem("test.move needs a position".into()))?;
                let moved = PlayerInfo { pos: to, ..p };
                if let Some(q) = self.scripts.players.iter_mut().find(|q| q.id == moved.id) {
                    *q = moved.clone();
                }
                // the table the case holds moves too
                if let Some(Value::Table(t)) = arg(1) {
                    t.set("position", script::vector(to)).map_err(fail)?;
                }
                one(Value::Table(script::player_table(self.lua(), &moved).map_err(fail)?))
            }
            "place" | "dig" | "use_block" => {
                let p = self.player(arg(1), &op)?;
                let at = Self::at(arg(2), &op)?;
                let old = self.voxels.at(at);
                match op.as_str() {
                    "place" => {
                        let name = arg(3).and_then(|v| v.as_string().map(|s| s.to_string_lossy())).unwrap_or_default();
                        if crate::items::by_name(&name).is_some_and(crate::items::is_item) {
                            return Err(Stop::Problem(format!("test.place: {name} is something to hold, not to put down")));
                        }
                        let b = blocks::by_name(&name).ok_or_else(|| Stop::Problem(format!("test.place: there's no block called \"{name}\"")))?;
                        // a survival player puts down what they carry
                        {
                            let mut sh = self.scripts.shared.lock().unwrap();
                            let pockets = sh.pockets.entry(p.id).or_default();
                            if pockets.survival && !pockets.slots.take(crate::items::of_block(b), 1) {
                                return Err(Stop::Problem(format!("test.place: {} isn't carrying any {name}", p.name)));
                            }
                        }
                        // a block that turns faces the way it's told (test 0.5), or north
                        let q = match arg(4).and_then(|v| v.as_string().map(|s| s.to_string_lossy())) {
                            Some(f) => blocks::QUARTERS.iter().position(|q| *q == f.as_str()).ok_or_else(|| Stop::Problem(format!("test.place: facing is one of {}", blocks::QUARTERS.join(", "))))? as u8,
                            None => 0,
                        };
                        let b = blocks::turned(b, q);
                        self.voxels.set(at, b);
                        self.scripts.player_block(p.id, at, old, b);
                    }
                    "dig" => {
                        if old != blocks::AIR {
                            // a survival player breaks it with what's in hand, as
                            // the game does: not at all if it needs better, and
                            // getting what creations say it gives
                            let rules = self.scripts.rules();
                            let mut sh = self.scripts.shared.lock().unwrap();
                            let held = sh.held.get(&p.id).copied().unwrap_or(0);
                            let pockets = sh.pockets.entry(p.id).or_default();
                            if pockets.survival {
                                if !rules.can_break(held, old) {
                                    return one(Value::Boolean(false));
                                }
                                let roll = crate::noise::hash2(at.x ^ at.y.wrapping_mul(7919), at.z, self.played as i32);
                                if let Some((t, n)) = rules.gives(old, roll) {
                                    pockets.slots.give(t, n);
                                }
                            }
                        }
                        self.voxels.set(at, blocks::AIR);
                        self.scripts.player_block(p.id, at, old, blocks::AIR);
                        self.pass(0.1);
                        return one(Value::Boolean(true));
                    }
                    _ => self.scripts.use_block(p.id, at, old),
                }
                // the creation hears it on the next tick, as in play
                self.pass(0.1);
                nothing
            }
            "use_item" => {
                // Interact on nothing in reach, looking `aim` (north if not said)
                let p = self.player(arg(1), "use_item")?;
                let aim = arg(2).and_then(script::vec_of).unwrap_or(Vec3::NEG_Z);
                self.scripts.use_item(p.id, aim);
                self.pass(0.1);
                nothing
            }
            "event" => {
                let event = arg(1).and_then(|v| v.as_string().map(|s| s.to_string_lossy())).unwrap_or_default();
                let op = if event == "used" { "use" } else { "hit" };
                let p = self.player(arg(2), op)?;
                let b = self.body(arg(3), op)?;
                self.scripts.event(&mut self.voxels, &mut self.herd, b, &event, &p);
                nothing
            }
            "ride" => {
                let p = self.player(arg(1), "ride")?;
                let b = self.body(arg(2), "ride")?;
                if !self.herd.bodies[&b].rideable {
                    return Err(Stop::Problem("test.ride: that creature can't be ridden".into()));
                }
                for o in self.herd.bodies.values_mut().filter(|o| o.rider == Some(p.id)) {
                    o.rider = None;
                }
                self.herd.bodies.get_mut(&b).unwrap().rider = Some(p.id);
                self.scripts.event(&mut self.voxels, &mut self.herd, b, "ridden", &p);
                nothing
            }
            "hold" => {
                let p = self.player(arg(1), "hold")?;
                let b = match arg(2).and_then(|v| v.as_string().map(|s| s.to_string_lossy())) {
                    None => 0,
                    Some(name) => crate::items::by_name(&name).ok_or_else(|| Stop::Problem(format!("test.hold: there's nothing called \"{name}\"")))?,
                };
                self.scripts.set_held(p.id, b);
                nothing
            }
            "danger" => {
                let level = arg(1).and_then(|v| v.as_string().map(|s| s.to_string_lossy())).unwrap_or_default();
                let d = script::Danger::parse(&level).ok_or_else(|| Stop::Problem(format!("test.danger: \"{level}\" isn't a danger (peaceful, gentle, normal or hard)")))?;
                self.scripts.set_danger(d);
                nothing
            }
            "get_off" => {
                let p = self.player(arg(1), "get_off")?;
                for o in self.herd.bodies.values_mut().filter(|o| o.rider == Some(p.id)) {
                    o.rider = None;
                }
                nothing
            }
            "press" => {
                let p = self.player(arg(1), "press")?;
                let action = arg(2).and_then(|v| v.as_string().map(|s| s.to_string_lossy())).unwrap_or_default();
                let known = self.scripts.vms[&self.id].actions.contains_key(&action);
                if !known {
                    return Err(Stop::Problem(format!("test.press: this creation has no action called \"{action}\" in its potluck.toml")));
                }
                let down = matches!(arg(3), Some(Value::Boolean(true)));
                self.scripts.press(p.id, &self.id.clone(), &action, down);
                self.pass(0.1);
                nothing
            }
            "answer" => {
                let p = self.player(arg(1), "answer")?;
                let choice = match arg(2) {
                    Some(Value::Integer(i)) if *i >= 1 => Some(*i as u32 - 1),
                    Some(Value::Number(n)) if *n >= 1.0 => Some(*n as u32 - 1),
                    _ => None,
                };
                let ask = self.scripts.shared.lock().unwrap().ui.panels.get(&p.id).and_then(|q| q.front().map(|f| f.ask));
                let ask = ask.ok_or_else(|| Stop::Problem("test.answer: nobody has asked that player anything".into()))?;
                self.scripts.answer(p.id, ask, choice);
                self.pass(0.1);
                nothing
            }
            "from_half" => {
                let p = self.player(arg(1), "from_half")?;
                let topic = arg(2).and_then(|v| v.as_string().map(|s| s.to_string_lossy())).unwrap_or_default();
                let value = match script::note_of(&topic, arg(3).unwrap_or(&Value::Nil)) {
                    Ok(v) => v,
                    Err(mlua::Error::RuntimeError(e)) => return Err(Stop::Problem(format!("test.from_half: {e}"))),
                    Err(e) => return Err(fail(e)),
                };
                self.scripts.hear(p.id, Note { creation: self.id.clone(), topic, value });
                self.pass(0.1);
                nothing
            }
            other => Err(Stop::Problem(format!("@potluck/test can't {other}"))),
        }
    }
}
