//! Worlds the player keeps. Each lives in its own folder under `worlds/`
//! in the data folder and saves as they play: every two minutes, when the
//! pause menu opens, on leaving and on quitting. A save holds the island
//! (`made::world_bytes`: every block, run-length encoded), what creations
//! keep (`@potluck/store`), their boards, the creations running besides the
//! base game's, and the player: what they carry, their health and where
//! they stood. Friends' worlds and servers aren't kept here: their host
//! keeps them. `--play` and `--shot` use the title's own world, which is
//! never saved, so pictures stay comparable.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::items;
use crate::creations::{Creation, Library, Origin, Payload};
use crate::creatures::Herd;
use crate::hosting::Boards;
use crate::net::{Net, Role};
use crate::player::Me;
use crate::script::{Scripts, Store};
use crate::settings::{self, Settings};
use crate::ui::{Act, Acts, Focus, Screen, Toasts, WorldInfo};
use crate::world::Voxels;

/// Seconds between saves while playing.
pub const SAVE_EVERY: f32 = 120.0;
/// The longest name a world can have.
const LONGEST_NAME: usize = 32;

/// A kept world, as the list shows it.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Kept {
    /// Its folder's name.
    #[serde(skip)]
    pub id: String,
    pub name: String,
    pub seed: u32,
    /// When it was made and last played, in seconds since 1970.
    pub created: u64,
    pub played: u64,
    /// How dangerous it is (`world.danger`); gentle for one from before.
    #[serde(default)]
    pub danger: crate::script::Danger,
}

/// Where the player stood and what they had, saved with the world. Things
/// go by name, so a save outlives a block's id changing.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Stood {
    pub pos: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub slot: usize,
    /// Every slot's thing, the hotbar's then the backpack's ("" for none),
    /// and how many of it.
    #[serde(alias = "hotbar")]
    pub pockets: Vec<String>,
    pub counts: Vec<u32>,
    pub health: f32,
    pub flying: bool,
}

impl Stood {
    pub fn of(me: &Me) -> Stood {
        Stood {
            pos: me.pos.to_array(),
            yaw: me.yaw,
            pitch: me.pitch,
            slot: me.slot,
            pockets: me.pockets.0.iter().map(|s| if s.thing == 0 { String::new() } else { items::name(s.thing).to_string() }).collect(),
            counts: me.pockets.0.iter().map(|s| s.count).collect(),
            health: me.health,
            flying: me.flying,
        }
    }

    /// Puts the player back as they were. A thing that's gone (its
    /// creation isn't here any more) leaves its slot empty.
    pub fn put(&self, me: &mut Me, voxels: &Voxels) {
        me.respawn(voxels, None);
        let pos = Vec3::from_array(self.pos);
        if pos.is_finite() {
            me.place(pos);
        }
        me.yaw = if self.yaw.is_finite() { self.yaw } else { 0.0 };
        me.pitch = if self.pitch.is_finite() { self.pitch.clamp(-1.55, 1.55) } else { 0.0 };
        me.slot = self.slot.min(8);
        for (i, slot) in me.pockets.0.iter_mut().enumerate() {
            match self.pockets.get(i).and_then(|n| items::by_name(n)) {
                Some(t) => {
                    slot.thing = t;
                    slot.count = self.counts.get(i).copied().unwrap_or(0).min(items::stack(t));
                }
                None => slot.count = 0,
            }
        }
        if self.health.is_finite() && self.health > 0.0 {
            me.health = self.health.min(crate::player::MAX_HEALTH);
        }
        me.flying = self.flying;
    }
}

/// A creation that was running, besides the base game's: the player's own
/// by its folder, anyone else's whole.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Ran {
    id: String,
    #[serde(default)]
    mine: bool,
    #[serde(default)]
    payload: Option<Payload>,
}

/// Everything a kept world's folder holds.
pub struct Loaded {
    pub kept: Kept,
    /// None for a world that's never been saved: it starts from its seed.
    pub voxels: Option<Voxels>,
    pub store: Store,
    pub boards: Boards,
    ran: Vec<Ran>,
    pub stood: Option<Stood>,
}

/// The kept world being played, if any.
#[derive(Resource, Default)]
pub struct Current {
    pub id: Option<String>,
    /// Seconds since it last saved.
    pub since: f32,
}

fn root() -> PathBuf {
    settings::data_dir().join("worlds")
}

/// A world's folder, if `id` is one (a plain name, never a path).
fn dir(id: &str) -> Option<PathBuf> {
    let plain = !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    plain.then(|| root().join(id))
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes).and_then(|_| std::fs::rename(&tmp, path)).map_err(|e| format!("couldn't save {}: {e}", path.display()))
}

fn read_json<T: for<'a> Deserialize<'a>>(path: &Path) -> Option<T> {
    std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok())
}

/// The kept worlds, last played first.
pub fn list() -> Vec<Kept> {
    let mut out = vec![];
    for entry in std::fs::read_dir(root()).into_iter().flatten().flatten() {
        let id = entry.file_name().to_string_lossy().to_string();
        if dir(&id).is_none() {
            continue;
        }
        if let Some(mut k) = read_json::<Kept>(&entry.path().join("meta.json")) {
            k.id = id;
            out.push(k);
        }
    }
    out.sort_by(|a, b| b.played.cmp(&a.played).then(a.name.cmp(&b.name)));
    out
}

/// Makes a new world's folder, named "World 1", "World 2" and so on, from
/// a seed of its own.
pub fn make(seed: Option<u32>) -> Result<Kept, String> {
    let have = list();
    let n = (1..).find(|n| !have.iter().any(|k| k.name == format!("World {n}")) && dir(&format!("world-{n}")).is_some_and(|d| !d.exists())).unwrap_or(1);
    let seed = seed.unwrap_or_else(|| {
        let t = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        (t as u32) ^ ((t >> 32) as u32).rotate_left(7) ^ std::process::id()
    });
    let kept = Kept { id: format!("world-{n}"), name: format!("World {n}"), seed, created: now(), played: now(), danger: Default::default() };
    let d = dir(&kept.id).ok_or("a world needs a plain name")?;
    std::fs::create_dir_all(&d).map_err(|e| format!("couldn't make {}: {e}", d.display()))?;
    write(&d.join("meta.json"), serde_json::to_string_pretty(&kept).unwrap_or_default().as_bytes())?;
    Ok(kept)
}

/// Renames a world (what the list shows; its folder stays).
pub fn rename(id: &str, name: &str) -> Result<(), String> {
    let d = dir(id).ok_or("no such world")?;
    let mut k: Kept = read_json(&d.join("meta.json")).ok_or("no such world")?;
    let name: String = name.trim().chars().filter(|c| !c.is_control()).take(LONGEST_NAME).collect();
    if name.is_empty() {
        return Err("a world needs a name".into());
    }
    k.name = name;
    write(&d.join("meta.json"), serde_json::to_string_pretty(&k).unwrap_or_default().as_bytes())
}

/// Sets how dangerous a world is, and says what it is now.
pub fn set_danger(id: &str, danger: crate::script::Danger) -> Result<(), String> {
    let d = dir(id).ok_or("no such world")?;
    let mut k: Kept = read_json(&d.join("meta.json")).ok_or("no such world")?;
    k.danger = danger;
    write(&d.join("meta.json"), serde_json::to_string_pretty(&k).unwrap_or_default().as_bytes())
}

/// Deletes a world for good.
pub fn forget(id: &str) -> Result<(), String> {
    let d = dir(id).ok_or("no such world")?;
    if !d.join("meta.json").exists() {
        return Err("no such world".into());
    }
    std::fs::remove_dir_all(&d).map_err(|e| format!("couldn't delete {}: {e}", d.display()))
}

/// Saves the world as it is into its folder.
pub fn save(id: &str, voxels: &Voxels, lib: &Library, scripts: &Scripts, me: &Me) -> Result<(), String> {
    let d = dir(id).ok_or("no such world")?;
    let mut kept: Kept = read_json(&d.join("meta.json")).ok_or("no such world")?;
    write(&d.join("world.potluck"), &crate::made::world_bytes(voxels))?;
    {
        let sh = scripts.shared.lock().unwrap();
        write(&d.join("store.json"), serde_json::to_string(&sh.store).unwrap_or_default().as_bytes())?;
        write(&d.join("boards.json"), serde_json::to_string(&sh.boards).unwrap_or_default().as_bytes())?;
    }
    let ran: Vec<Ran> = lib
        .running
        .iter()
        .filter_map(|id| lib.list.get(id))
        .filter(|c| c.origin != Origin::Base)
        .map(|c| match c.origin {
            Origin::Mine => Ran { id: c.id.clone(), mine: true, payload: None },
            _ => Ran { id: c.id.clone(), mine: false, payload: Some(c.payload()) },
        })
        .collect();
    write(&d.join("creations.json"), serde_json::to_string(&ran).unwrap_or_default().as_bytes())?;
    write(&d.join("player.json"), serde_json::to_string_pretty(&Stood::of(me)).unwrap_or_default().as_bytes())?;
    kept.played = now();
    write(&d.join("meta.json"), serde_json::to_string_pretty(&kept).unwrap_or_default().as_bytes())
}

/// Reads a kept world's folder.
pub fn load(id: &str) -> Result<Loaded, String> {
    let d = dir(id).ok_or("no such world")?;
    let mut kept: Kept = read_json(&d.join("meta.json")).ok_or("no such world")?;
    kept.id = id.to_string();
    let voxels = match std::fs::read(d.join("world.potluck")) {
        Ok(bytes) => Some(crate::made::read_world(&bytes).ok_or_else(|| format!("{} is damaged", kept.name))?),
        Err(_) => None,
    };
    Ok(Loaded {
        voxels,
        store: Store::load(&d.join("store.json")),
        boards: Boards::load(&d.join("boards.json")),
        ran: read_json(&d.join("creations.json")).unwrap_or_default(),
        stood: read_json(&d.join("player.json")),
        kept,
    })
}

/// How long ago, in words.
pub fn ago(then: u64) -> String {
    let s = now().saturating_sub(then);
    match s {
        0..=89 => "just now".into(),
        90..=3599 => format!("{} minutes ago", (s + 30) / 60),
        3600..=7199 => "an hour ago".into(),
        7200..=86_399 => format!("{} hours ago", s / 3600),
        86_400..=172_799 => "yesterday".into(),
        _ => format!("{} days ago", s / 86_400),
    }
}

/// Whether this game is playing someone else's world.
fn guest(net: &Net) -> bool {
    matches!(net.role, Role::Client(_))
}

/// Plays a kept world: its island, what creations kept, its creations and
/// the player as they were.
#[allow(clippy::too_many_arguments)]
fn enter(loaded: Loaded, lib: &mut Library, scripts: &mut Scripts, herd: &mut Herd, net: &mut Net, toasts: &mut Toasts, me: &mut Me, voxels: &mut Voxels) {
    *voxels = loaded.voxels.unwrap_or_else(|| crate::world::generate(loaded.kept.seed));
    herd.bodies.clear();
    {
        let mut sh = scripts.shared.lock().unwrap();
        sh.store = loaded.store;
        sh.boards = loaded.boards;
        sh.danger = loaded.kept.danger;
    }
    crate::game::start_world(lib, scripts, herd, net, toasts);
    for r in loaded.ran {
        if r.mine {
            if lib.list.get(&r.id).is_some_and(|c| c.origin == Origin::Mine) {
                crate::game::bring(&r.id, lib, scripts, herd, net, toasts);
            }
        } else if let Some(p) = r.payload {
            let by = p.manifest.by.clone();
            if let Ok(c) = Creation::from_payload(p, Origin::Friend(by)) {
                let id = lib.add(c);
                crate::game::bring(&id, lib, scripts, herd, net, toasts);
            }
        }
    }
    match &loaded.stood {
        Some(s) => s.put(me, voxels),
        None => me.respawn(voxels, None),
    }
}

/// The worlds screens' acts, before `game::do_acts`; saving before leaving
/// or quitting, which `do_acts` then does.
#[allow(clippy::too_many_arguments)]
pub fn acts(
    mut acts: ResMut<Acts>,
    mut focus: ResMut<Focus>,
    mut lib: ResMut<Library>,
    mut scripts: ResMut<Scripts>,
    mut herd: ResMut<Herd>,
    mut net: ResMut<Net>,
    mut toasts: ResMut<Toasts>,
    settings: Res<Settings>,
    mut me: ResMut<Me>,
    mut voxels: ResMut<Voxels>,
    mut info: ResMut<WorldInfo>,
    mut current: ResMut<Current>,
) {
    let mut rest = vec![];
    for act in std::mem::take(&mut acts.0) {
        match act {
            Act::OpenWorlds(host) => {
                focus.worlds = list();
                focus.status.clear();
                focus.go(Screen::Worlds(host));
            }
            Act::NewWorld(host) => match make(None) {
                Ok(k) => rest.push(Act::PlayWorld(k.id, host)),
                Err(e) => focus.status = format!("Couldn't make a world: {e}"),
            },
            Act::PlayWorld(id, host) => match load(&id) {
                Ok(loaded) => {
                    let name = loaded.kept.name.clone();
                    enter(loaded, &mut lib, &mut scripts, &mut herd, &mut net, &mut toasts, &mut me, &mut voxels);
                    current.id = Some(id.clone());
                    current.since = 0.0;
                    // played now, so it's first in the list
                    let _ = save(&id, &voxels, &lib, &scripts, &me);
                    if host {
                        match net.host(&settings.name) {
                            Ok(addr) => toasts.push(format!("{name} is open. Friends on this network can join; or give them {addr}")),
                            Err(e) => toasts.push(format!("Couldn't open {name} to friends: {e}")),
                        }
                    }
                    info.in_world = true;
                    focus.go(Screen::Playing);
                }
                Err(e) => focus.status = format!("Couldn't open that world: {e}"),
            },
            Act::OpenForget(id) => {
                focus.status.clear();
                focus.fields.remove("world_name");
                focus.go(Screen::Forget(id));
            }
            Act::ForgetWorld(id) => {
                if current.id.as_deref() == Some(id.as_str()) {
                    focus.status = "That world is open. Leave it first.".into();
                } else {
                    match forget(&id) {
                        Ok(()) => toasts.push("Deleted the world"),
                        Err(e) => toasts.push(format!("Couldn't delete the world: {e}")),
                    }
                    focus.worlds = list();
                    focus.go(Screen::Worlds(false));
                }
            }
            Act::WorldDanger(id) => {
                let next = focus.worlds.iter().find(|k| k.id == id).map(|k| k.danger.next()).unwrap_or_default();
                match set_danger(&id, next) {
                    Ok(()) => {
                        // the open world changes straight away
                        if current.id.as_deref() == Some(id.as_str()) {
                            scripts.set_danger(next);
                        }
                        focus.worlds = list();
                    }
                    Err(e) => focus.status = e,
                }
            }
            Act::RenameWorld(id) => {
                let name = focus.fields.get("world_name").cloned().unwrap_or_default();
                match rename(&id, &name) {
                    Ok(()) => {
                        focus.worlds = list();
                        focus.go(Screen::Worlds(false));
                    }
                    Err(e) => focus.status = e,
                }
            }
            Act::Leave | Act::Quit => {
                if let Some(id) = current.id.take() {
                    if info.in_world && !guest(&net) {
                        if let Err(e) = save(&id, &voxels, &lib, &scripts, &me) {
                            warn!("{e}");
                        }
                    }
                }
                rest.push(act);
            }
            other => rest.push(other),
        }
    }
    acts.0 = rest;
}

/// Saves the kept world every `SAVE_EVERY` seconds, and when the pause
/// menu opens (where players go before closing the window).
pub fn autosave(time: Res<Time>, mut current: ResMut<Current>, focus: Res<Focus>, info: Res<WorldInfo>, net: Res<Net>, voxels: Res<Voxels>, lib: Res<Library>, scripts: Res<Scripts>, me: Res<Me>, mut paused: Local<bool>) {
    let Some(id) = current.id.clone() else { return };
    if !info.in_world || guest(&net) {
        return;
    }
    current.since += time.delta_secs();
    let pause = focus.screen == Screen::Pause;
    let opened = pause && !*paused;
    *paused = pause;
    if current.since >= SAVE_EVERY || opened {
        current.since = 0.0;
        if let Err(e) = save(&id, &voxels, &lib, &scripts, &me) {
            warn!("{e}");
        }
    }
}
