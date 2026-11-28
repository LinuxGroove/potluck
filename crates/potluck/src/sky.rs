//! The weather and the season (`@potluck/sky`). Creations on the host say
//! what they are (`Sky`, kept in `GameShared.sky`), the host sends that to
//! every game (`Msg::Sky`, and `Msg::Strike` for lightning), and each game
//! eases into it in `Weather`, which the drawing reads. Nothing here moves
//! the camera; lightning is a soft glow, and none with Reduce flashing on.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::creations::Library;
use crate::net::{Net, Role};
use crate::player::Me;
use crate::script::Scripts;

/// What's falling, or hanging, in the sky.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    #[default]
    Clear,
    Cloudy,
    Rain,
    Storm,
    Snow,
    Fog,
}

impl Kind {
    pub const ALL: [Kind; 6] = [Kind::Clear, Kind::Cloudy, Kind::Rain, Kind::Storm, Kind::Snow, Kind::Fog];

    /// Its name for creations: "clear", "cloudy", "rain", "storm", "snow" or "fog".
    pub fn name(self) -> &'static str {
        match self {
            Kind::Clear => "clear",
            Kind::Cloudy => "cloudy",
            Kind::Rain => "rain",
            Kind::Storm => "storm",
            Kind::Snow => "snow",
            Kind::Fog => "fog",
        }
    }

    pub fn named(name: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.name() == name)
    }

    /// Whether rain or snow falls.
    pub fn falls(self) -> bool {
        matches!(self, Kind::Rain | Kind::Storm | Kind::Snow)
    }
}

/// The four seasons.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Season {
    #[default]
    Spring,
    Summer,
    Autumn,
    Winter,
}

impl Season {
    pub const ALL: [Season; 4] = [Season::Spring, Season::Summer, Season::Autumn, Season::Winter];

    pub fn name(self) -> &'static str {
        match self {
            Season::Spring => "spring",
            Season::Summer => "summer",
            Season::Autumn => "autumn",
            Season::Winter => "winter",
        }
    }

    pub fn named(name: &str) -> Option<Season> {
        Season::ALL.into_iter().find(|s| s.name() == name)
    }
}

/// The most wind there is, in blocks a second.
pub const MAX_WIND: f32 = 12.0;
/// The longest a change of weather takes to come in, in seconds.
pub const MAX_OVER: f32 = 120.0;
/// How long a change takes when a creation doesn't say.
pub const OVER: f32 = 20.0;
/// Strikes a creation may make a second, and at once.
pub const STRIKES_A_SECOND: f32 = 1.0;
pub const STRIKES_AT_ONCE: f32 = 3.0;

/// What the sky is doing, as the host's creations last said: the same on
/// every game in a world.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sky {
    pub kind: Kind,
    /// How heavy: 0 to 1.
    pub strength: f32,
    /// Blocks a second along x and z.
    pub wind: [f32; 2],
    /// Seconds to come in from what it was.
    pub over: f32,
    pub season: Season,
    /// How far through the season, 0 to 1.
    pub progress: f32,
}

impl Default for Sky {
    fn default() -> Self {
        Sky { kind: Kind::Clear, strength: 0.0, wind: [0.0, 0.0], over: OVER, season: Season::Spring, progress: 0.0 }
    }
}

impl Sky {
    /// The same, with every number in range (from another game).
    pub fn tidy(mut self) -> Sky {
        let num = |v: f32, lo: f32, hi: f32| if v.is_finite() { v.clamp(lo, hi) } else { lo };
        self.strength = num(self.strength, 0.0, 1.0);
        self.over = num(self.over, 0.0, MAX_OVER);
        self.progress = num(self.progress, 0.0, 1.0);
        let w = Vec2::from(self.wind.map(|v| num(v, -MAX_WIND, MAX_WIND)));
        self.wind = w.clamp_length_max(MAX_WIND).to_array();
        self
    }
}

/// The weather each game draws: the sky as the host said, and how far
/// it has come into it. Every amount is 0 to 1 and eases toward what `sky`
/// says over `sky.over` seconds, so nothing changes all at once.
#[derive(Resource, Clone, Debug)]
pub struct Weather {
    pub sky: Sky,
    /// Rain falling (storms too).
    pub rain: f32,
    /// Snow falling.
    pub snow: f32,
    /// Fog: the view closes in.
    pub fog: f32,
    /// Cloud over the sun: some when cloudy, more with rain, most in a storm.
    pub cloud: f32,
    /// How stormy: darker cloud, heavier rain and lightning.
    pub storm: f32,
    /// The wind, blocks a second along x and z.
    pub wind: Vec2,
    /// How far into autumn the land is (leaves turning), and into winter
    /// (snow lying, bare trees): each eases in and out over a day or so of
    /// the season's start and end.
    pub autumn: f32,
    pub winter: f32,
    /// Lightning: where each strike was and how many seconds ago, for a few
    /// seconds after.
    pub strikes: Vec<(Vec3, f32)>,
    /// Takes on the next sky at once rather than easing into it: a world
    /// starting, or joining one.
    pub fresh: bool,
}

impl Default for Weather {
    fn default() -> Self {
        Weather { sky: Sky::default(), rain: 0.0, snow: 0.0, fog: 0.0, cloud: 0.0, storm: 0.0, wind: Vec2::ZERO, autumn: 0.0, winter: 0.0, strikes: vec![], fresh: true }
    }
}

/// How long a strike's glow lasts, and how long it takes to come up.
pub const GLOW_FOR: f32 = 1.6;
const GLOW_RISE: f32 = 0.15;

impl Weather {
    /// What each amount is heading for.
    pub fn targets(sky: &Sky) -> [f32; 5] {
        let s = sky.strength.clamp(0.0, 1.0);
        let (rain, snow, fog, cloud, storm) = match sky.kind {
            Kind::Clear => (0.0, 0.0, 0.0, 0.0, 0.0),
            Kind::Cloudy => (0.0, 0.0, 0.0, 0.35 + 0.4 * s, 0.0),
            Kind::Rain => (s, 0.0, 0.15 * s, 0.6 + 0.3 * s, 0.0),
            Kind::Storm => ((0.6 + 0.4 * s).min(1.0), 0.0, 0.2 * s, 1.0, s.max(0.3)),
            Kind::Snow => (0.0, s, 0.25 * s, 0.6 + 0.3 * s, 0.0),
            Kind::Fog => (0.0, 0.0, 0.3 + 0.7 * s, 0.4, 0.0),
        };
        [rain, snow, fog, cloud, storm]
    }

    /// Where the land is heading for autumn and winter: each comes in over
    /// the first part of its season and goes over the first part of the
    /// next (the leaves fall as winter starts, the snow melts in spring).
    pub fn season_targets(sky: &Sky) -> (f32, f32) {
        let p = sky.progress.clamp(0.0, 1.0);
        let coming = (p / 0.15).min(1.0);
        let going = (1.0 - p / 0.1).max(0.0);
        match sky.season {
            Season::Autumn => (coming, 0.0),
            Season::Winter => (going, coming),
            Season::Spring => (0.0, going),
            Season::Summer => (0.0, 0.0),
        }
    }

    /// Moves every amount on by `dt` seconds, and ages the strikes.
    pub fn step(&mut self, dt: f32) {
        let dt = dt.clamp(0.0, 1.0);
        let over = self.sky.over.max(0.05);
        let rate = dt / over;
        let to = Weather::targets(&self.sky);
        for (v, t) in [&mut self.rain, &mut self.snow, &mut self.fog, &mut self.cloud, &mut self.storm].into_iter().zip(to) {
            *v += (t - *v).clamp(-rate, rate);
        }
        let wind = Vec2::from(self.sky.wind);
        let most = rate * MAX_WIND;
        self.wind += (wind - self.wind).clamp_length_max(most);
        // the land takes a few minutes to turn
        let (autumn, winter) = Weather::season_targets(&self.sky);
        let land = dt / 180.0;
        self.autumn += (autumn - self.autumn).clamp(-land, land);
        self.winter += (winter - self.winter).clamp(-land, land);
        for s in &mut self.strikes {
            s.1 += dt;
        }
        self.strikes.retain(|s| s.1 < GLOW_FOR);
    }

    /// The sky as the host now says: eased into, or at once when fresh.
    pub fn take(&mut self, sky: Sky) {
        self.sky = sky.tidy();
        if std::mem::take(&mut self.fresh) {
            self.settle();
        }
    }

    /// Takes on the sky at once (a new world, or joining one).
    pub fn settle(&mut self) {
        let [rain, snow, fog, cloud, storm] = Weather::targets(&self.sky);
        (self.rain, self.snow, self.fog, self.cloud, self.storm) = (rain, snow, fog, cloud, storm);
        self.wind = Vec2::from(self.sky.wind);
        (self.autumn, self.winter) = Weather::season_targets(&self.sky);
    }

    /// How much lightning brightens the sky now, 0 to 1: a soft swell and
    /// fade, never a flash, and nothing at all with Reduce flashing on.
    pub fn glow(&self, reduce_flashing: bool) -> f32 {
        if reduce_flashing {
            return 0.0;
        }
        self.strikes
            .iter()
            .map(|(_, age)| if *age < GLOW_RISE { age / GLOW_RISE } else { 1.0 - (age - GLOW_RISE) / (GLOW_FOR - GLOW_RISE) })
            .fold(0.0f32, f32::max)
            .clamp(0.0, 1.0)
            * 0.5
    }
}

/// Thunder still to come: (seconds until it's heard, how loud).
#[derive(Resource, Default)]
pub struct Thunder(pub Vec<(f32, f32)>);

/// Sound goes about this many blocks a second.
const SOUND_SPEED: f32 = 340.0;
/// How often the rain's hiss is played again (it's longer than this, so
/// they overlap into one sound).
const RAIN_EVERY: f32 = 1.5;

/// A strike reached this game: its glow, and its thunder after a moment.
pub fn strike(weather: &mut Weather, thunder: &mut Thunder, at: Vec3, ear: Option<Vec3>) {
    if !at.is_finite() {
        return;
    }
    weather.strikes.push((at, 0.0));
    if weather.strikes.len() > 8 {
        weather.strikes.remove(0);
    }
    let far = ear.map(|e| e.distance(at)).unwrap_or(0.0);
    if far < 600.0 && thunder.0.len() < 8 {
        thunder.0.push((far / SOUND_SPEED, (1.0 - far / 600.0).clamp(0.2, 1.0) * 0.8));
    }
}

/// Eases each game's weather, and plays the rain and the thunder where
/// there's a player to hear them (not on the server).
fn ease(time: Res<Time>, mut weather: ResMut<Weather>, mut thunder: ResMut<Thunder>, mut heard: ResMut<crate::sound::Heard>, me: Option<Res<Me>>, mut hiss: Local<f32>) {
    let dt = time.delta_secs();
    weather.step(dt);
    if me.is_none() {
        thunder.0.clear();
        return;
    }
    for t in &mut thunder.0 {
        t.0 -= dt;
        if t.0 <= 0.0 {
            heard.0.push(crate::sound::Play { name: "thunder".into(), at: None, volume: t.1, pitch: 1.0, caption: None });
        }
    }
    thunder.0.retain(|t| t.0 > 0.0);
    let falling = weather.rain.max(weather.snow * 0.3);
    *hiss -= dt;
    if falling > 0.05 && *hiss <= 0.0 {
        *hiss = RAIN_EVERY;
        heard.0.push(crate::sound::Play { name: "rain".into(), at: None, volume: 0.35 * falling, pitch: 1.0, caption: Some(String::new()) });
    }
}

/// On the host (or alone): what creations said of the sky and the strikes
/// they made, for this game's own weather and every other game's.
fn host_sky(mut scripts: ResMut<Scripts>, mut net: ResMut<Net>, lib: Res<Library>, mut weather: ResMut<Weather>, mut thunder: ResMut<Thunder>, me: Option<Res<Me>>, mut world: Local<Option<u32>>) {
    if matches!(net.role, Role::Client(_)) {
        return;
    }
    // a new world starts as it is, not easing in from the last one
    if *world != Some(lib.world) {
        *world = Some(lib.world);
        weather.fresh = true;
        weather.strikes.clear();
    }
    if let Some(sky) = scripts.sky_changes() {
        weather.take(sky);
        net.broadcast_sky(sky);
    }
    for at in scripts.strike_changes() {
        strike(&mut weather, &mut thunder, at, me.as_ref().map(|m| m.pos));
        net.broadcast_strike(at);
    }
}

pub struct SkyPlugin;

impl Plugin for SkyPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Weather>()
            .init_resource::<Thunder>()
            .add_systems(Update, host_sky.after(crate::game::host_step).before(crate::net::host_net))
            .add_systems(Update, ease.after(crate::Sim));
    }
}
