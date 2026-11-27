//! Waypoints: places a player keeps on the map, for each world, on this
//! device; and going to them, with a short fade and never a swoop of the
//! camera.
//!
//! The rules for going somewhere (`may_go`): building, you go anywhere on
//! the map, to any waypoint or to a friend, at once. In survival you go
//! only to your own waypoints at places you've stood, after standing still
//! for a few seconds, and then rest a minute before the next. A host can
//! turn it off for everyone (`teleport` in server.toml).

use std::collections::BTreeMap;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::blocks::{self, WATER};
use crate::player::Me;
use crate::world::{Voxels, WX, WY, WZ};

/// Waypoint colours, by name, as the map and the world show them.
pub const COLORS: &[(&str, [u8; 3])] = &[
    ("Mint", [94, 224, 199]),
    ("Gold", [255, 196, 92]),
    ("Red", [255, 115, 107]),
    ("Violet", [177, 140, 255]),
    ("Blue", [96, 160, 255]),
    ("Pink", [255, 127, 175]),
    ("Green", [140, 214, 96]),
    ("White", [240, 240, 240]),
];

/// Waypoints a world keeps on one device.
pub const MAX: usize = 100;
/// Seconds to stand still before going, in survival.
pub const WAIT: f32 = 3.0;
/// Seconds between trips, in survival.
pub const REST: f32 = 60.0;
/// How near a waypoint counts as standing at it.
const STOOD: f32 = 3.0;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub enum Kind {
    /// One the player made.
    #[default]
    Mine,
    /// Where the player last fell in survival.
    Fell,
    /// A place someone shared in chat, kept: who shared it.
    Shared(String),
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Waypoint {
    pub id: u32,
    pub name: String,
    /// The block the player stood in (feet).
    pub at: [i32; 3],
    /// An index into `COLORS`.
    #[serde(default)]
    pub color: usize,
    /// Shown in the world, over where it is.
    #[serde(default = "yes")]
    pub shown: bool,
    /// The player has stood here, so survival lets them come back.
    #[serde(default)]
    pub stood: bool,
    #[serde(default)]
    pub kind: Kind,
}

fn yes() -> bool {
    true
}

impl Waypoint {
    /// The middle of its block, at the feet.
    pub fn pos(&self) -> Vec3 {
        Vec3::new(self.at[0] as f32 + 0.5, self.at[1] as f32, self.at[2] as f32 + 0.5)
    }

    pub fn rgb(&self) -> [u8; 3] {
        COLORS[self.color % COLORS.len()].1
    }

    /// What its pin on the map says: a number it ends with, or its first letter.
    pub fn letter(&self) -> String {
        if self.kind == Kind::Fell {
            return "X".into();
        }
        let last = self.name.split_whitespace().last().unwrap_or("");
        if !last.is_empty() && last.len() <= 3 && last.chars().all(|c| c.is_ascii_digit()) {
            return last.to_string();
        }
        self.name.chars().find(|c| c.is_alphanumeric()).map(|c| c.to_uppercase().to_string()).unwrap_or_else(|| "?".into())
    }
}

/// Every world's waypoints on this device, and which world is this one.
#[derive(Resource, Default, Serialize, Deserialize)]
pub struct Waypoints {
    worlds: BTreeMap<String, Vec<Waypoint>>,
    /// This world, as `world_key` names it ("" outside a world).
    #[serde(skip)]
    pub here: String,
    /// Changed since the last save.
    #[serde(skip)]
    dirty: bool,
    /// Where they're kept; none in tests that don't save.
    #[serde(skip)]
    file: Option<std::path::PathBuf>,
}

impl Waypoints {
    pub fn load() -> Self {
        let file = crate::settings::data_dir().join("waypoints.json");
        let mut w: Waypoints = std::fs::read_to_string(&file).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        w.file = Some(file);
        w
    }

    pub fn save(&mut self) {
        if !self.dirty {
            return;
        }
        self.dirty = false;
        let Some(file) = &self.file else { return };
        let _ = std::fs::create_dir_all(crate::settings::data_dir());
        let tmp = file.with_extension("tmp");
        if std::fs::write(&tmp, serde_json::to_string_pretty(self).unwrap_or_default()).is_ok() {
            let _ = std::fs::rename(tmp, file);
        }
    }

    /// This world's waypoints.
    pub fn list(&self) -> &[Waypoint] {
        self.worlds.get(&self.here).map(|v| v.as_slice()).unwrap_or(&[])
    }

    pub fn get(&self, id: u32) -> Option<&Waypoint> {
        self.list().iter().find(|w| w.id == id)
    }

    /// Changes one of this world's waypoints (and saves it later).
    pub fn edit(&mut self, id: u32, f: impl FnOnce(&mut Waypoint)) {
        if let Some(w) = self.worlds.get_mut(&self.here).and_then(|l| l.iter_mut().find(|w| w.id == id)) {
            f(w);
            self.dirty = true;
        }
    }

    /// A new waypoint in this world. None when there's no world or it's full.
    pub fn add(&mut self, name: &str, at: IVec3, stood: bool, kind: Kind) -> Option<u32> {
        if self.here.is_empty() {
            return None;
        }
        let list = self.worlds.entry(self.here.clone()).or_default();
        if list.len() >= MAX {
            return None;
        }
        let id = list.iter().map(|w| w.id).max().unwrap_or(0) + 1;
        // colours go round, so neighbours differ
        let color = list.iter().filter(|w| w.kind != Kind::Fell).count() % COLORS.len();
        let name: String = name.trim().chars().take(32).collect();
        list.push(Waypoint { id, name: if name.is_empty() { format!("Place {id}") } else { name }, at: at.to_array(), color, shown: true, stood, kind });
        self.dirty = true;
        Some(id)
    }

    pub fn remove(&mut self, id: u32) {
        if let Some(list) = self.worlds.get_mut(&self.here) {
            list.retain(|w| w.id != id);
            self.dirty = true;
        }
    }

    /// A name for the next one: Place 1, Place 2...
    pub fn next_name(&self) -> String {
        let used = |n: usize| self.list().iter().any(|w| w.name == format!("Place {n}"));
        let n = (1..).find(|n| !used(*n)).unwrap_or(1);
        format!("Place {n}")
    }

    /// Moves the one "Where you fell" mark (making it the first time).
    pub fn fell(&mut self, at: IVec3) {
        let old = self.list().iter().find(|w| w.kind == Kind::Fell).map(|w| w.id);
        match old {
            Some(id) => self.edit(id, |w| {
                w.at = at.to_array();
                w.stood = false;
            }),
            None => {
                if let Some(id) = self.add("Where you fell", at, false, Kind::Fell) {
                    let red = COLORS.iter().position(|c| c.0 == "Red").unwrap_or(0);
                    self.edit(id, |w| w.color = red);
                }
            }
        }
    }

    /// Marks the waypoints the player is standing at as stood at. True when
    /// one changed.
    pub fn stand(&mut self, feet: Vec3) -> bool {
        let Some(list) = self.worlds.get_mut(&self.here) else { return false };
        let mut changed = false;
        for w in list.iter_mut().filter(|w| !w.stood) {
            let d = w.pos() - feet;
            // marked on the map, it has no height until someone stands there
            let ground = w.at[1] <= 0;
            if Vec2::new(d.x, d.z).length() <= STOOD && (ground || d.y.abs() <= STOOD) {
                w.stood = true;
                if ground {
                    w.at[1] = feet.y.floor() as i32;
                }
                changed = true;
            }
        }
        self.dirty |= changed;
        changed
    }

    /// For tests: a set with no file.
    pub fn in_memory(here: &str) -> Self {
        Waypoints { here: here.into(), ..Default::default() }
    }
}

/// Which world a player is in, as waypoints are kept: a world made here
/// by its seed (solo worlds come back the same from it), a joined one by
/// its address and seed.
pub fn world_key(in_world: bool, joined: Option<&str>, seed: u32) -> String {
    match (in_world, joined) {
        (false, _) => String::new(),
        (true, Some(addr)) => format!("{}#{seed}", addr.trim().trim_end_matches('/').to_lowercase()),
        (true, None) => format!("seed {seed}"),
    }
}

/// What a trip is to.
#[derive(Clone, Debug, PartialEq)]
pub enum Goal {
    /// One of this player's waypoints, and whether they've stood there.
    Waypoint { stood: bool },
    /// A spot picked on the map.
    Spot,
    /// Another player.
    Player,
}

/// Whether a player may go somewhere now, and if so how many seconds they
/// stand still first.
pub fn may_go(goal: &Goal, survival: bool, world_allows: bool, riding: bool, rest: f32) -> Result<f32, String> {
    if !world_allows {
        return Err("This world doesn't let players teleport.".into());
    }
    if riding {
        return Err("Get off first, then go.".into());
    }
    if !survival {
        return Ok(0.0);
    }
    match goal {
        Goal::Waypoint { stood: true } if rest > 0.0 => Err(format!("You can go again in {} s.", rest.ceil() as i32)),
        Goal::Waypoint { stood: true } => Ok(WAIT),
        Goal::Waypoint { stood: false } => Err("In survival you can go back to places you've stood at. Walk there once first.".into()),
        Goal::Spot | Goal::Player => Err("In survival you can go only to your own waypoints, at places you've stood.".into()),
    }
}

/// Where someone going to `to` lands: there if it's clear with ground near
/// (`exact`, for waypoints, which may be in a house or a cave), otherwise
/// on the ground under the open sky, or on a tree that covers it.
pub fn landing(v: &Voxels, to: Vec3, exact: bool) -> Vec3 {
    let x = (to.x.floor() as i32).clamp(1, WX - 2);
    let z = (to.z.floor() as i32).clamp(1, WZ - 2);
    let open = |y: i32| !blocks::is_solid(v.get(x, y, z)) && !blocks::is_solid(v.get(x, y + 1, z));
    let floor = |y: i32| (1..=3).any(|d| blocks::is_solid(v.get(x, y - d, z)) || v.get(x, y - d, z) == WATER);
    let at = |y: i32| Vec3::new(x as f32 + 0.5, y as f32 + 0.01, z as f32 + 0.5);
    if exact {
        let y = to.y.floor() as i32;
        for dy in [0, 1, -1, 2, -2] {
            let y = y + dy;
            if (1..WY - 2).contains(&y) && open(y) && floor(y) {
                return at(y);
            }
        }
    }
    let ground = v.top(x, z) + 1;
    if open(ground) {
        return at(ground);
    }
    // under leaves: stand on the canopy
    let top = (0..WY - 2).rev().find(|&y| blocks::is_solid(v.get(x, y, z))).unwrap_or(0);
    at(top + 1)
}

#[derive(Clone, Debug, PartialEq)]
pub enum Stage {
    /// Standing still for the seconds left (survival).
    Waiting(f32),
    /// The screen darkening.
    Leaving,
    /// Arrived, in the dark a moment while the place appears.
    Arrived(f32),
    /// The screen lightening again.
    Coming,
}

#[derive(Clone, Debug)]
pub struct Trip {
    pub to: Vec3,
    /// Land at `to`'s height if it's clear (waypoints), or on the ground.
    pub exact: bool,
    pub name: String,
    pub stage: Stage,
    /// Where the player stood and their health when they asked.
    from: Vec3,
    health: f32,
}

/// A trip under way, the darkness of the screen and the rest before the
/// next one in survival.
#[derive(Resource, Default)]
pub struct Travel {
    pub trip: Option<Trip>,
    /// Seconds before survival lets the player go again.
    pub rest: f32,
    /// How dark the screen is, 0 to 1.
    pub dark: f32,
    /// What the countdown says, for the HUD.
    pub line: String,
}

const DARKEN: f32 = 0.22;
const HOLD: f32 = 0.18;
const LIGHTEN: f32 = 0.45;

impl Travel {
    /// Starts going somewhere, after `wait` seconds standing still.
    pub fn start(&mut self, me: &Me, to: Vec3, exact: bool, name: &str, wait: f32) {
        let stage = if wait > 0.0 { Stage::Waiting(wait) } else { Stage::Leaving };
        self.trip = Some(Trip { to, exact, name: name.to_string(), stage, from: me.pos, health: me.health });
    }

    pub fn going(&self) -> bool {
        self.trip.is_some()
    }

    /// One frame of a trip. Returns what to tell the player, if anything.
    pub fn step(&mut self, dt: f32, me: &mut Me, v: &Voxels, survival: bool) -> Option<String> {
        self.rest = (self.rest - dt).max(0.0);
        let Some(trip) = &mut self.trip else {
            self.dark = (self.dark - dt / LIGHTEN).max(0.0);
            self.line.clear();
            return None;
        };
        let mut said = None;
        match trip.stage.clone() {
            Stage::Waiting(left) => {
                let d = me.pos - trip.from;
                if Vec2::new(d.x, d.z).length() > 0.6 || me.health < trip.health || me.riding.is_some() {
                    self.trip = None;
                    self.line.clear();
                    return Some("Stopped: stand still to teleport.".into());
                }
                let left = left - dt;
                self.line = format!("Going to {} in {}  ·  move to stop", trip.name, left.ceil().max(1.0) as i32);
                trip.stage = if left <= 0.0 { Stage::Leaving } else { Stage::Waiting(left) };
            }
            Stage::Leaving => {
                self.line.clear();
                self.dark = (self.dark + dt / DARKEN).min(1.0);
                if self.dark >= 1.0 {
                    let at = landing(v, trip.to, trip.exact);
                    me.place(at);
                    if survival {
                        self.rest = REST;
                    }
                    said = Some(format!("You're at {}", trip.name));
                    trip.stage = Stage::Arrived(HOLD);
                }
            }
            Stage::Arrived(left) => {
                trip.stage = if left - dt <= 0.0 { Stage::Coming } else { Stage::Arrived(left - dt) };
            }
            Stage::Coming => {
                self.dark = (self.dark - dt / LIGHTEN).max(0.0);
                if self.dark <= 0.0 {
                    self.trip = None;
                }
            }
        }
        said
    }
}

/// The screen going dark for a trip, and the countdown before it.
#[derive(Component)]
pub struct Fade;
#[derive(Component)]
pub struct Countdown;

pub fn spawn_fade(mut commands: Commands, fonts: Res<crate::ui::Fonts>) {
    commands.spawn((Fade, Node { position_type: PositionType::Absolute, width: percent(100), height: percent(100), ..default() }, BackgroundColor(Color::NONE), GlobalZIndex(40), Pickable::IGNORE));
    commands.spawn((
        Countdown,
        Text::new(""),
        TextFont { font: fonts.ui.clone().into(), font_size: FontSize::Px(20.0), ..default() },
        TextColor(crate::ui::INK),
        TextLayout::justify(Justify::Center),
        crate::map::shadow(),
        Node { position_type: PositionType::Absolute, top: percent(30), width: percent(100), justify_content: JustifyContent::Center, ..default() },
        GlobalZIndex(41),
        Pickable::IGNORE,
        Visibility::Hidden,
    ));
}

/// Trips, the fade, and waypoints noticing they've been stood at.
#[allow(clippy::too_many_arguments)]
pub fn travel(
    time: Res<Time>,
    mut travel: ResMut<Travel>,
    mut me: ResMut<Me>,
    voxels: Res<Voxels>,
    settings: Res<crate::settings::Settings>,
    mut waypoints: ResMut<Waypoints>,
    mut toasts: ResMut<crate::ui::Toasts>,
    mut heard: ResMut<crate::sound::Heard>,
    mut fade: Query<&mut BackgroundColor, With<Fade>>,
    mut countdown: Query<(&mut Text, &mut Visibility), With<Countdown>>,
    mut since: Local<f32>,
) {
    let dt = time.delta_secs().min(0.05);
    let was_dark = travel.dark >= 1.0;
    if let Some(said) = travel.step(dt, &mut me, &voxels, settings.survival) {
        toasts.push(said);
    }
    if !was_dark && travel.dark >= 1.0 {
        heard.0.push(crate::sound::Play { name: "whoosh".into(), at: None, volume: 0.25, pitch: 1.3, caption: Some("Whoosh".into()) });
    }
    for mut bg in &mut fade {
        bg.0 = Color::srgba(0.02, 0.03, 0.05, travel.dark);
    }
    for (mut t, mut v) in &mut countdown {
        if t.0 != travel.line {
            t.0 = travel.line.clone();
        }
        *v = if travel.line.is_empty() { Visibility::Hidden } else { Visibility::Inherited };
    }
    // where you fell, in survival
    if let Some(at) = me.fell_at.take() {
        waypoints.fell(at.floor().as_ivec3());
    }
    *since += dt;
    if *since > 0.5 {
        *since = 0.0;
        waypoints.stand(me.pos);
        waypoints.save();
    }
}

/// Keeps `Waypoints::here` on the world being played.
pub fn follow_world(info: Res<crate::ui::WorldInfo>, net: Res<crate::net::Net>, voxels: Res<Voxels>, mut waypoints: ResMut<Waypoints>) {
    let key = world_key(info.in_world, net.joined_address(), voxels.seed);
    if waypoints.here != key {
        waypoints.save();
        waypoints.here = key;
    }
}
