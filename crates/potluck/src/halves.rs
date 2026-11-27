//! Client halves: a second Luau file a creation names as `client` in its
//! potluck.toml, run on each player's own game for what only that player
//! sees: lines on their screen, sounds for them, clocks and countdowns that
//! need no messages while they run. A half gets only what its creation
//! sends it with `@potluck/net`, and answers the same way; it never sees
//! or changes the world, which stays the host's.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bevy::prelude::*;
use mlua::{Function, Lua, Table, Value, VmState};

use crate::creations::{Creation, Library};
use crate::noise::Rng;
use crate::script::{self, bad, Note, Piece, PlayerInfo, Waiting, MAX_HANDLERS, MAX_PIECES, NOTES_UP_A_SECOND, SOUNDS_A_SECOND};
use crate::sound::Play;

/// A half's memory: less than a creation gets on the host.
pub const MEMORY_LIMIT: usize = 16 << 20;
/// Each half's share of a frame.
pub const FRAME_BUDGET: Duration = Duration::from_millis(4);
const LOAD_BUDGET: Duration = Duration::from_millis(300);

/// What `@potluck/world` does only on the host, where the world is.
const HOST_ONLY: &[&str] = &[
    "creature", "block", "set_block", "top", "find_block", "find_light", "players", "nearest_player", "spawn", "creatures", "say", "on", "pockets", "has", "take",
    "give", "hurt", "heal", "healing", "held", "digging", "kind",
];

/// What a half asked for since it was last looked at.
#[derive(Default)]
struct Out {
    /// (id, piece, or None to hide it)
    pieces: Vec<(String, Option<Piece>)>,
    /// The ids it shows now.
    shown: BTreeSet<String>,
    sounds: Vec<Play>,
    sound_budget: f32,
    notes: Vec<(String, serde_json::Value)>,
    note_budget: f32,
    log: Vec<String>,
}

/// One creation's client half on this game.
pub struct Half {
    /// What it started from: a change to any file starts it again.
    files: BTreeMap<String, String>,
    world: u32,
    run: Option<Running>,
    pub log: Vec<String>,
    /// Why it stopped, when it did.
    pub error: Option<String>,
}

struct Running {
    lua: Lua,
    waiting: Arc<Mutex<Waiting>>,
    on: Arc<Mutex<Vec<(String, Function)>>>,
    out: Arc<Mutex<Out>>,
    me: Arc<Mutex<PlayerInfo>>,
    clock: Arc<Mutex<f64>>,
    /// The time of day on this game (`render::Clock::time`).
    day: Arc<Mutex<f32>>,
    deadline: Arc<Mutex<Instant>>,
}

/// What every half did since it was last asked, for the game to show,
/// play and send.
#[derive(Default)]
pub struct HalfOutput {
    /// (creation, id, piece or None): lines on this player's screen.
    pub pieces: Vec<(String, String, Option<Piece>)>,
    pub sounds: Vec<Play>,
    /// Messages to their creations on the host.
    pub notes: Vec<Note>,
}

/// The client halves running on this game, by creation.
#[derive(Resource, Default)]
pub struct Halves {
    pub list: BTreeMap<String, Half>,
    /// Pieces of halves that stopped, to take off the screen.
    hide: Vec<(String, String)>,
    /// The time of day halves see, from this game's clock.
    pub day: Option<f32>,
}

fn host_only(what: &str) -> mlua::Error {
    bad(format!("{what} works in the creation's main file, on the world's host: a client half can ask it with net.send"))
}

impl Running {
    fn load(creation: &Creation, file: &str, me: PlayerInfo) -> Result<Running, String> {
        let lua = Lua::new();
        lua.set_memory_limit(MEMORY_LIMIT).map_err(|e| e.to_string())?;
        let deadline = Arc::new(Mutex::new(Instant::now() + LOAD_BUDGET));
        let d = deadline.clone();
        lua.set_interrupt(move |_| {
            if Instant::now() > *d.lock().unwrap() {
                return Err(bad("this client half took too long (is there a loop that never ends?)"));
            }
            Ok(VmState::Continue)
        });
        let waiting = Arc::new(Mutex::new(Waiting::default()));
        let on: Arc<Mutex<Vec<(String, Function)>>> = Arc::default();
        let out = Arc::new(Mutex::new(Out { sound_budget: SOUNDS_A_SECOND, note_budget: NOTES_UP_A_SECOND, ..default() }));
        let rng = Arc::new(Mutex::new(Rng::new(crate::noise::seed_of(&creation.id) ^ me.id.wrapping_mul(0x9e37_79b9))));
        let me = Arc::new(Mutex::new(me));
        let clock = Arc::new(Mutex::new(0.0f64));
        let day = Arc::new(Mutex::new(crate::render::Clock::default().time));
        let setup = || -> mlua::Result<()> {
            // world: its timers, its own clock and dice, and named spots
            let world = lua.create_table()?;
            let w = waiting.clone();
            world.set("after", lua.create_function(move |_, (seconds, f): (f64, Function)| script::add_timer(&w, seconds, false, f))?)?;
            let w = waiting.clone();
            world.set("every", lua.create_function(move |_, (seconds, f): (f64, Function)| script::add_timer(&w, seconds, true, f))?)?;
            let w = waiting.clone();
            world.set("cancel", lua.create_function(move |_, id: Option<u32>| Ok(script::cancel_timer(&w, id)))?)?;
            let c = clock.clone();
            world.set("time", lua.create_function(move |_, ()| Ok(*c.lock().unwrap()))?)?;
            let d = day.clone();
            world.set("daytime", lua.create_function(move |_, ()| Ok(*d.lock().unwrap()))?)?;
            let d = day.clone();
            world.set(
                "daylight",
                lua.create_function(move |_, ()| Ok(crate::render::Clock { time: *d.lock().unwrap(), running: true }.daylight()))?,
            )?;
            let r = rng.clone();
            world.set(
                "random",
                lua.create_function(move |_, (lo, hi): (Option<f64>, Option<f64>)| {
                    let x = r.lock().unwrap().next() as f64;
                    Ok(match (lo, hi) {
                        (Some(a), Some(b)) => a + x * (b - a),
                        (Some(b), None) => x * b,
                        _ => x,
                    })
                })?,
            )?;
            world.set("spot", lua.create_function(|_, name: String| Ok(script::spot(&name).map(script::vector)))?)?;
            for name in HOST_ONLY {
                let what = format!("world.{name}");
                world.set(*name, lua.create_function(move |_, _: mlua::Variadic<Value>| -> mlua::Result<()> { Err(host_only(&what)) })?)?;
            }
            // net: messages to and from its creation
            let net = lua.create_table()?;
            let o = out.clone();
            net.set(
                "send",
                lua.create_function(move |_, (topic, value): (String, Value)| {
                    let value = script::note_of(&topic, &value)?;
                    let mut o = o.lock().unwrap();
                    if o.note_budget < 1.0 {
                        return Ok(false);
                    }
                    o.note_budget -= 1.0;
                    o.notes.push((topic, value));
                    Ok(true)
                })?,
            )?;
            let h = on.clone();
            net.set(
                "on",
                lua.create_function(move |_, (topic, f): (String, Function)| {
                    script::note_of(&topic, &Value::Nil)?;
                    let mut h = h.lock().unwrap();
                    if h.len() >= MAX_HANDLERS {
                        return Err(bad(format!("a client half can give net.on at most {MAX_HANDLERS} handlers")));
                    }
                    h.push((topic, f));
                    Ok(())
                })?,
            )?;
            let m = me.clone();
            net.set("me", lua.create_function(move |lua, ()| script::player_table(lua, &m.lock().unwrap()))?)?;
            // ui: lines on this player's screen
            let ui = lua.create_table()?;
            let mine = |m: &Mutex<PlayerInfo>, v: &Value| -> mlua::Result<()> {
                if v.is_nil() || script::pid_of(v)? == m.lock().unwrap().id {
                    Ok(())
                } else {
                    Err(bad("a client half shows things only to its own player: pass nil, or net.me()"))
                }
            };
            let (o, m) = (out.clone(), me.clone());
            ui.set(
                "show",
                lua.create_function(move |_, (player, id, text, opts): (Value, String, String, Option<Table>)| {
                    mine(&m, &player)?;
                    let piece = script::piece_of(&id, &text, opts)?;
                    let mut o = o.lock().unwrap();
                    if piece.is_some() && !o.shown.contains(&id) && o.shown.len() >= MAX_PIECES {
                        return Err(bad(format!("a client half can show at most {MAX_PIECES} pieces at once: ui.hide the ones it's done with")));
                    }
                    match piece.is_some() {
                        true => o.shown.insert(id.clone()),
                        false => o.shown.remove(&id),
                    };
                    o.pieces.push((id, piece));
                    Ok(())
                })?,
            )?;
            let (o, m) = (out.clone(), me.clone());
            ui.set(
                "hide",
                lua.create_function(move |_, (player, id): (Value, String)| {
                    mine(&m, &player)?;
                    let mut o = o.lock().unwrap();
                    o.shown.remove(&id);
                    o.pieces.push((id, None));
                    Ok(())
                })?,
            )?;
            let (o, m) = (out.clone(), me.clone());
            ui.set(
                "meter",
                lua.create_function(move |_, (player, id, value, opts): (Value, String, Option<f64>, Option<Table>)| {
                    mine(&m, &player)?;
                    let piece = script::meter_of(&id, value, opts)?;
                    let mut o = o.lock().unwrap();
                    if piece.is_some() && !o.shown.contains(&id) && o.shown.len() >= MAX_PIECES {
                        return Err(bad(format!("a client half can show at most {MAX_PIECES} pieces at once: ui.hide the ones it's done with")));
                    }
                    match piece.is_some() {
                        true => o.shown.insert(id.clone()),
                        false => o.shown.remove(&id),
                    };
                    o.pieces.push((id, piece));
                    Ok(())
                })?,
            )?;
            for name in ["ask", "close"] {
                let what = format!("ui.{name}");
                ui.set(name, lua.create_function(move |_, _: mlua::Variadic<Value>| -> mlua::Result<()> { Err(host_only(&what)) })?)?;
            }
            // sound: heard by this player only
            let sound = lua.create_table()?;
            let play = {
                let o = out.clone();
                move |name: String, opts: Option<Table>| -> mlua::Result<bool> {
                    if crate::sound::caption_of(&name).is_none() {
                        return Err(bad(format!("there's no sound called \"{name}\" (sound.names() lists them)")));
                    }
                    let (at, volume, pitch, caption) = script::sound_options(opts)?;
                    let mut o = o.lock().unwrap();
                    if o.sound_budget < 1.0 {
                        return Ok(false);
                    }
                    o.sound_budget -= 1.0;
                    o.sounds.push(Play { name, at, volume, pitch, caption });
                    Ok(true)
                }
            };
            let p = play.clone();
            sound.set("play", lua.create_function(move |_, (name, opts): (String, Option<Table>)| p(name, opts))?)?;
            let m = me.clone();
            sound.set(
                "play_for",
                lua.create_function(move |_, (player, name, opts): (Value, String, Option<Table>)| {
                    mine(&m, &player)?;
                    play(name, opts)
                })?,
            )?;
            sound.set("names", lua.create_function(|_, ()| Ok(crate::sound::SOUNDS.iter().map(|s| s.0).collect::<Vec<_>>()))?)?;
            // require: these four, or another of the creation's files
            let files = creation.files.clone();
            let cache = lua.create_table()?;
            let req = lua.create_function(move |lua, name: String| {
                if let Some(module) = script::api_module(&name)? {
                    return match module {
                        "world" => Ok(Value::Table(world.clone())),
                        "net" => Ok(Value::Table(net.clone())),
                        "ui" => Ok(Value::Table(ui.clone())),
                        "sound" => Ok(Value::Table(sound.clone())),
                        m => Err(bad(format!("@potluck/{m} works in the creation's main file, on the world's host; a client half has net, ui, sound and world's timers"))),
                    };
                }
                script::own_file(lua, &files, &cache, &name)
            })?;
            lua.globals().set("require", req)?;
            let o = out.clone();
            script::print_to(&lua, move |line| {
                let mut o = o.lock().unwrap();
                if o.log.len() < 200 {
                    o.log.push(line);
                }
            })?;
            script::lock_down(&lua)
        };
        setup().map_err(|e| e.to_string())?;
        let src = creation.files.get(file).ok_or_else(|| format!("{} has no {file} for its client half", creation.manifest.name))?;
        lua.load(src.as_str()).set_name(format!("@{file}")).exec().map_err(|e| tidy(&e))?;
        Ok(Running { lua, waiting, on, out, me, clock, day, deadline })
    }

    /// One frame: what its creation sent, then its timers.
    fn frame(&self, dt: f32, me: &PlayerInfo, notes: Vec<Note>) -> Result<(), String> {
        *self.me.lock().unwrap() = me.clone();
        *self.clock.lock().unwrap() += dt as f64;
        {
            let mut o = self.out.lock().unwrap();
            o.sound_budget = (o.sound_budget + dt * SOUNDS_A_SECOND).min(SOUNDS_A_SECOND);
            o.note_budget = (o.note_budget + dt * NOTES_UP_A_SECOND).min(NOTES_UP_A_SECOND);
        }
        *self.deadline.lock().unwrap() = Instant::now() + FRAME_BUDGET;
        let r = (|| -> mlua::Result<()> {
            if !notes.is_empty() {
                let on = self.on.lock().unwrap().clone();
                for n in notes {
                    for (_, f) in on.iter().filter(|(t, _)| *t == n.topic) {
                        f.call::<()>(script::from_json(&self.lua, &n.value)?)?;
                    }
                }
            }
            script::run_timers(&self.waiting, dt as f64)
        })();
        r.map_err(|e| tidy(&e))
    }
}

fn tidy(e: &mlua::Error) -> String {
    let s = e.to_string();
    s.lines().next().unwrap_or("").trim_start_matches("runtime error: ").to_string()
}

impl Halves {
    /// Starts, starts again and stops halves to match what runs in this
    /// world: a half starts afresh when its creation's files change or a
    /// new world begins. `None` is no world at all.
    pub fn sync(&mut self, lib: Option<&Library>, me: &PlayerInfo) {
        let want: BTreeMap<&str, (&Creation, &str)> = match lib {
            Some(lib) => lib.running.iter().filter_map(|id| lib.list.get(id)).filter_map(|c| Some((c.id.as_str(), (c, c.manifest.client.as_deref()?)))).collect(),
            None => BTreeMap::new(),
        };
        let world = lib.map_or(0, |l| l.world);
        let stale: Vec<String> = self.list.iter().filter(|(id, h)| want.get(id.as_str()).is_none_or(|(c, _)| c.files != h.files || h.world != world)).map(|(id, _)| id.clone()).collect();
        for id in stale {
            self.stop(&id);
        }
        for (id, (c, file)) in want {
            if self.list.contains_key(id) {
                continue;
            }
            let (run, error) = match Running::load(c, file, me.clone()) {
                Ok(r) => (Some(r), None),
                Err(e) => (None, Some(e)),
            };
            if let Some(e) = &error {
                warn!("{}'s client half: {e}", c.manifest.name);
            }
            self.list.insert(id.to_string(), Half { files: c.files.clone(), world, run, log: vec![], error });
        }
    }

    fn stop(&mut self, id: &str) {
        if let Some(h) = self.list.remove(id) {
            if let Some(r) = h.run {
                let o = r.out.lock().unwrap();
                self.hide.extend(o.shown.iter().map(|p| (id.to_string(), p.clone())));
            }
        }
    }

    /// Gives each half what its creation sent this frame, and runs its
    /// timers. A half that fails stops, and says why in Look inside.
    pub fn frame(&mut self, dt: f32, me: &PlayerInfo, notes: Vec<Note>) {
        let mut by: BTreeMap<String, Vec<Note>> = BTreeMap::new();
        for n in notes {
            by.entry(n.creation.clone()).or_default().push(n);
        }
        for (id, h) in self.list.iter_mut() {
            let Some(r) = &h.run else { continue };
            if let Some(d) = self.day {
                *r.day.lock().unwrap() = d;
            }
            if let Err(e) = r.frame(dt, me, by.remove(id).unwrap_or_default()) {
                warn!("{id}'s client half stopped: {e}");
                h.error = Some(e);
                let r = h.run.take().unwrap();
                let mut o = r.out.lock().unwrap();
                h.log.append(&mut o.log);
                self.hide.extend(o.shown.iter().map(|p| (id.clone(), p.clone())));
            }
        }
    }

    /// What the halves showed, played and sent since last asked.
    pub fn take(&mut self) -> HalfOutput {
        let mut out = HalfOutput { pieces: self.hide.drain(..).map(|(c, id)| (c, id, None)).collect(), ..default() };
        for (id, h) in self.list.iter_mut() {
            let Some(r) = &h.run else { continue };
            let mut o = r.out.lock().unwrap();
            out.pieces.extend(o.pieces.drain(..).map(|(p, piece)| (id.clone(), p, piece)));
            out.sounds.append(&mut o.sounds);
            out.notes.extend(o.notes.drain(..).map(|(topic, value)| Note { creation: id.clone(), topic, value }));
            h.log.append(&mut o.log);
            if h.log.len() > 200 {
                let n = h.log.len() - 200;
                h.log.drain(..n);
            }
        }
        out
    }
}

/// Runs the client halves on the game with a player at the screen: what
/// creations sent this player in, what the halves show, play and send out.
#[allow(clippy::too_many_arguments)]
pub fn run(
    time: Res<Time>,
    lib: Res<Library>,
    info: Res<crate::ui::WorldInfo>,
    net: Res<crate::net::Net>,
    settings: Option<Res<crate::settings::Settings>>,
    mut halves: ResMut<Halves>,
    me: Option<ResMut<crate::player::Me>>,
    mut actions: ResMut<crate::game::Actions>,
    mut heard: ResMut<crate::sound::Heard>,
    clock: Res<crate::render::Clock>,
) {
    let Some(mut me) = me else { return };
    halves.day = Some(clock.time);
    let mine = PlayerInfo { id: net.my_id, name: settings.map(|s| s.name.clone()).unwrap_or_default(), pos: me.pos };
    halves.sync(info.in_world.then_some(&*lib), &mine);
    let notes = std::mem::take(&mut me.notes);
    halves.frame(time.delta_secs().min(0.25), &mine, notes);
    let out = halves.take();
    for (creation, id, piece) in out.pieces {
        me.show(script::UiChange::Piece { creation, id, piece });
    }
    heard.0.extend(out.sounds);
    actions.0.extend(out.notes.into_iter().map(|n| (net.my_id, crate::game::Action::Net(n))));
}
