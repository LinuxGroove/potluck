//! Sounds: the game's own (steps, digging, placing, menus) and the ones
//! creations play by name with `@potluck/sound`. Every one is made from
//! numbers when the game starts, the same every time, so there are no
//! sound files to ship or share.

use std::f32::consts::{PI, TAU};
use std::num::NonZero;
use std::sync::Arc;
use std::time::Duration;

use bevy::audio::{AddAudioSource, Decodable, PlaybackMode, SpatialListener, SpatialScale, Volume};
use bevy::prelude::*;

use crate::blocks::{self, BlockId};
use crate::noise::Rng;
use crate::player::{MainCamera, Me};
use crate::settings::Settings;

/// Samples a second, one channel.
pub const RATE: u32 = 22050;
/// How far away a sound in the world reaches anyone, in blocks.
pub const HEARD_WITHIN: f32 = 40.0;
/// Blocks to one unit of the ears' distance falloff: full loudness within
/// five blocks, a quarter at ten, a sixteenth at twenty.
const SCALE: f32 = 0.2;
/// Sounds playing at once on one device; past it, new ones are skipped.
const AT_ONCE: usize = 24;
/// How long a subtitle stays, in seconds.
const CAPTION_FOR: f32 = 3.0;

/// Every sound there is, by name, and what subtitles say for it. Creations
/// can play any of them; an empty caption is one subtitles leave out (your
/// own steps and digging).
pub const SOUNDS: &[(&str, &str)] = &[
    ("pop", "Pop"),
    ("chime", "Chime"),
    ("ding", "Ding"),
    ("boing", "Boing"),
    ("beep", "Beep"),
    ("quack", "Quack"),
    ("croak", "Croak"),
    ("bleat", "Bleat"),
    ("yip", "Yip"),
    ("chirp", "Chirp"),
    ("grumble", "Grumbling"),
    ("splash", "Splash"),
    ("thud", "Thud"),
    ("whoosh", "Whoosh"),
    ("fanfare", "Fanfare"),
    ("buzz", "Buzzing"),
    ("sparkle", "Sparkle"),
    ("click", "Click"),
    ("step", ""),
    ("step_hard", ""),
    ("step_wood", ""),
    ("step_plant", ""),
    ("dig", "Digging"),
    ("crack", "Stone breaks"),
    ("chop", "Wood breaks"),
    ("rustle", "Leaves rustle"),
    ("shatter", "Glass breaks"),
    ("place", ""),
];

pub fn caption_of(name: &str) -> Option<&'static str> {
    SOUNDS.iter().find(|s| s.0 == name).map(|s| s.1)
}

/// One sound to play, as the host sends it.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Play {
    pub name: String,
    /// Where it is in the world; None plays it the same loudness anywhere.
    pub at: Option<[f32; 3]>,
    /// 0 to 1.
    pub volume: f32,
    /// 1 is as made; 2 an octave up.
    pub pitch: f32,
    /// What subtitles say, if not the sound's own word.
    pub caption: Option<String>,
}

/// Sounds for this device to play this frame.
#[derive(Resource, Default)]
pub struct Heard(pub Vec<Play>);

impl Heard {
    /// A menu's click: the same loudness anywhere, and no subtitle.
    pub fn click(&mut self) {
        self.0.push(Play { name: "click".into(), at: None, volume: 0.35, pitch: 1.0, caption: Some(String::new()) });
    }

    /// One of the game's own sounds, somewhere in the world.
    pub fn game(&mut self, name: &str, at: Vec3, volume: f32, pitch: f32) {
        self.0.push(Play { name: name.into(), at: Some(at.to_array()), volume, pitch, caption: None });
    }
}

/// What a block sounds like underfoot and when it breaks.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Material {
    Soft,
    Hard,
    Wood,
    Plant,
    Glass,
    Water,
    Air,
}

/// A block's material, by its name, so new blocks sound right without a
/// list here.
pub fn material(id: BlockId) -> Material {
    if id == blocks::AIR {
        return Material::Air;
    }
    let n = blocks::block(id).name.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| n.contains(w));
    if has(&["water"]) {
        Material::Water
    } else if has(&["glass"]) {
        Material::Glass
    } else if has(&["leaves", "needles", "tall grass", "flower", "fern", "bush", "wheat", "carrot", "reed", "vine", "moss carpet"]) {
        Material::Plant
    } else if has(&["log", "plank", "barrel", "crate", "bookshelf", "table", "stool", "fence", "stake", "wood", "door"]) {
        Material::Wood
    } else if has(&["stone", "brick", "cobble", "bedrock", "column", "brazier", "pot", "lantern", "slate", "tile", "ore", "iron"]) {
        Material::Hard
    } else {
        Material::Soft
    }
}

/// The kinds of block a tool can dig faster (`world.digging`), in the order
/// of a tool's speeds.
pub const DIG_KINDS: [&str; 5] = ["soft", "hard", "wood", "plant", "glass"];

impl Material {
    /// Its name for creations (`world.kind`): one of `DIG_KINDS`, or water or air.
    pub fn name(self) -> &'static str {
        match self {
            Material::Soft => "soft",
            Material::Hard => "hard",
            Material::Wood => "wood",
            Material::Plant => "plant",
            Material::Glass => "glass",
            Material::Water => "water",
            Material::Air => "air",
        }
    }

    /// Where it is in a tool's speeds, for the kinds that can be dug.
    pub fn dig(self) -> Option<usize> {
        DIG_KINDS.iter().position(|k| *k == self.name())
    }

    pub fn step(self) -> Option<&'static str> {
        match self {
            Material::Soft => Some("step"),
            Material::Hard | Material::Glass => Some("step_hard"),
            Material::Wood => Some("step_wood"),
            Material::Plant => Some("step_plant"),
            Material::Water => Some("splash"),
            Material::Air => None,
        }
    }

    pub fn breaks(self) -> Option<&'static str> {
        match self {
            Material::Soft => Some("dig"),
            Material::Hard => Some("crack"),
            Material::Wood => Some("chop"),
            Material::Plant => Some("rustle"),
            Material::Glass => Some("shatter"),
            Material::Water | Material::Air => None,
        }
    }

    /// How high a placed block's knock is.
    pub fn knock(self) -> f32 {
        match self {
            Material::Wood => 1.15,
            Material::Hard => 0.9,
            Material::Plant => 1.4,
            Material::Glass => 1.3,
            _ => 0.75,
        }
    }
}

// ---------- making the sounds ----------

fn len(secs: f32) -> usize {
    (secs * RATE as f32) as usize
}

/// A quick rise and a long fall.
fn perc(t: f32, attack: f32, decay: f32) -> f32 {
    if t < attack {
        t / attack
    } else {
        (-(t - attack) / decay).exp()
    }
}

fn saw(p: f32) -> f32 {
    2.0 * (p - (p + 0.5).floor())
}

fn tri(p: f32) -> f32 {
    2.0 * saw(p).abs() - 1.0
}

fn sine(p: f32) -> f32 {
    (p * TAU).sin()
}

/// A rounded square.
fn soft_square(p: f32) -> f32 {
    (4.0 * sine(p)).tanh()
}

/// A tone whose pitch and loudness follow `freq` and `amp` over time.
fn tone(secs: f32, freq: impl Fn(f32) -> f32, amp: impl Fn(f32) -> f32, wave: fn(f32) -> f32) -> Vec<f32> {
    let mut phase = 0.0f32;
    (0..len(secs))
        .map(|i| {
            let t = i as f32 / RATE as f32;
            phase = (phase + freq(t) / RATE as f32).fract();
            wave(phase) * amp(t)
        })
        .collect()
}

/// Noise through a low pass (two poles, so it's properly dull) that
/// follows `cutoff` (in Hz) over time.
fn noise(secs: f32, rng: &mut Rng, amp: impl Fn(f32) -> f32, cutoff: impl Fn(f32) -> f32) -> Vec<f32> {
    let (mut y1, mut y2) = (0.0f32, 0.0f32);
    (0..len(secs))
        .map(|i| {
            let t = i as f32 / RATE as f32;
            let a = 1.0 - (-TAU * cutoff(t) / RATE as f32).exp();
            y1 += a * (rng.next() * 2.0 - 1.0 - y1);
            y2 += a * (y1 - y2);
            y2 * amp(t)
        })
        .collect()
}

/// A one-pole low pass over what's there.
fn lowpass(v: &mut [f32], cutoff: f32) {
    let a = 1.0 - (-TAU * cutoff / RATE as f32).exp();
    let mut y = 0.0;
    for s in v.iter_mut() {
        y += a * (*s - y);
        *s = y;
    }
}

/// Keeps only what's above `cutoff`.
fn highpass(v: &mut [f32], cutoff: f32) {
    let mut low = v.to_vec();
    lowpass(&mut low, cutoff);
    for (s, l) in v.iter_mut().zip(low) {
        *s -= l;
    }
}

/// Adds `b` into `a`, `at` seconds in, `gain` loud.
fn mix(a: &mut Vec<f32>, b: &[f32], at: f32, gain: f32) {
    let start = len(at);
    if a.len() < start + b.len() {
        a.resize(start + b.len(), 0.0);
    }
    for (i, s) in b.iter().enumerate() {
        a[start + i] += s * gain;
    }
}

/// Makes one sound's samples: at most a couple of seconds, peaking at 0.8.
pub fn synth(name: &str) -> Vec<f32> {
    let mut rng = Rng::new(crate::noise::seed_of(name));
    let mut v: Vec<f32> = match name {
        "pop" => tone(0.12, |t| 900.0 - 5600.0 * t, |t| perc(t, 0.002, 0.03), sine),
        "chime" => {
            let mut v = tone(1.6, |_| 880.0, |t| perc(t, 0.002, 0.45), sine);
            mix(&mut v, &tone(1.6, |_| 1322.6, |t| perc(t, 0.002, 0.35), sine), 0.0, 0.5);
            mix(&mut v, &tone(1.6, |_| 2217.0, |t| perc(t, 0.002, 0.22), sine), 0.0, 0.25);
            v
        }
        "ding" => {
            let mut v = tone(1.0, |_| 1568.0, |t| perc(t, 0.001, 0.25), sine);
            mix(&mut v, &tone(1.0, |_| 3136.0, |t| perc(t, 0.001, 0.12), sine), 0.0, 0.3);
            v
        }
        "boing" => tone(0.6, |t| 160.0 + 260.0 * (1.0 - (-t * 8.0).exp()) + 30.0 * (TAU * 14.0 * t).sin() * (-t * 3.0).exp(), |t| perc(t, 0.005, 0.22), tri),
        "beep" => {
            let mut v = tone(0.09, |_| 1000.0, |t| perc(t, 0.004, 0.2), soft_square);
            mix(&mut v, &tone(0.1, |_| 1500.0, |t| perc(t, 0.004, 0.2), soft_square), 0.12, 1.0);
            lowpass(&mut v, 4000.0);
            v
        }
        "quack" => {
            let mut v = tone(0.2, |t| 340.0 - 600.0 * t, |t| (t / 0.02).min(1.0) * (-t / 0.09).exp(), saw);
            lowpass(&mut v, 1800.0);
            // the nasal honk
            let honk = v.clone();
            let mut band = honk.clone();
            highpass(&mut band, 900.0);
            mix(&mut v, &band, 0.0, 1.2);
            v
        }
        "croak" => {
            let mut v = tone(0.4, |t| 95.0 - 50.0 * t, |t| perc(t, 0.01, 0.2) * if (TAU * 28.0 * t).sin() > 0.0 { 1.0 } else { 0.35 }, saw);
            lowpass(&mut v, 900.0);
            v
        }
        "bleat" => {
            let mut v = tone(0.55, |t| 420.0 + 25.0 * (TAU * 7.0 * t).sin() - 60.0 * t, |t| perc(t, 0.04, 0.25) * (0.75 + 0.25 * (TAU * 7.0 * t).sin()), saw);
            lowpass(&mut v, 2200.0);
            v
        }
        "yip" => {
            let mut v = tone(0.13, |t| 800.0 + 900.0 * (PI * t / 0.13).sin(), |t| perc(t, 0.005, 0.05), sine);
            mix(&mut v, &tone(0.13, |t| 1600.0 + 1800.0 * (PI * t / 0.13).sin(), |t| perc(t, 0.005, 0.04), sine), 0.0, 0.25);
            v
        }
        "chirp" => {
            let one = tone(0.06, |t| 2800.0 + 23000.0 * t, |t| perc(t, 0.004, 0.02), sine);
            let mut v = one.clone();
            mix(&mut v, &one, 0.09, 0.8);
            v
        }
        "grumble" => {
            let mut v = tone(0.7, |t| 65.0 + 10.0 * (TAU * 3.0 * t).sin(), |t| perc(t, 0.08, 0.3) * (0.6 + 0.4 * (TAU * 9.0 * t).sin()), saw);
            lowpass(&mut v, 500.0);
            mix(&mut v, &noise(0.7, &mut rng, |t| perc(t, 0.08, 0.3), |_| 300.0), 0.0, 0.6);
            v
        }
        "splash" => noise(0.6, &mut rng, |t| perc(t, 0.005, 0.16), |t| 4000.0 * (-t * 4.0).exp() + 300.0),
        "thud" => {
            let mut v = tone(0.25, |t| 70.0 - 120.0 * t, |t| perc(t, 0.002, 0.07), sine);
            mix(&mut v, &noise(0.03, &mut rng, |t| perc(t, 0.001, 0.01), |_| 1500.0), 0.0, 0.8);
            v
        }
        "whoosh" => noise(0.6, &mut rng, |t| (PI * t / 0.6).sin().powi(2), |t| 400.0 + 2400.0 * (PI * t / 0.6).sin()),
        "fanfare" => {
            let mut v = vec![];
            for (i, f) in [523.25f32, 659.25, 783.99, 1046.5].into_iter().enumerate() {
                let held = if i == 3 { 0.6 } else { 0.11 };
                let note = tone(held + 0.05, move |_| f, move |t| (t / 0.01).min(1.0) * if t > held { (-(t - held) / 0.02).exp() } else { 1.0 }, soft_square);
                mix(&mut v, &note, i as f32 * 0.12, 1.0);
            }
            lowpass(&mut v, 3000.0);
            v
        }
        "buzz" => {
            let mut v = tone(0.5, |t| 190.0 + 8.0 * (TAU * 5.0 * t).sin(), |t| (t / 0.03).min(1.0) * ((0.5 - t) / 0.08).clamp(0.0, 1.0) * (0.7 + 0.3 * (TAU * 23.0 * t).sin()), saw);
            lowpass(&mut v, 2500.0);
            v
        }
        "sparkle" => {
            let mut v = vec![0.0; len(0.6)];
            for _ in 0..8 {
                let f = 2500.0 + rng.next() * 2500.0;
                let at = rng.next() * 0.45;
                mix(&mut v, &tone(0.12, move |_| f, |t| perc(t, 0.001, 0.03), sine), at, 0.6);
            }
            v
        }
        "click" => {
            let mut v = noise(0.015, &mut rng, |t| perc(t, 0.0005, 0.003), |_| 9000.0);
            highpass(&mut v, 1500.0);
            v
        }
        "step" => noise(0.08, &mut rng, |t| perc(t, 0.003, 0.022), |_| 700.0),
        "step_hard" => {
            let mut v = noise(0.06, &mut rng, |t| perc(t, 0.001, 0.012), |_| 3000.0);
            mix(&mut v, &tone(0.04, |_| 180.0, |t| perc(t, 0.001, 0.01), sine), 0.0, 0.5);
            v
        }
        "step_wood" => {
            let mut v = tone(0.08, |t| 220.0 - 400.0 * t, |t| perc(t, 0.001, 0.02), tri);
            mix(&mut v, &noise(0.05, &mut rng, |t| perc(t, 0.001, 0.012), |_| 1200.0), 0.0, 0.6);
            v
        }
        "step_plant" | "rustle" => {
            let secs = if name == "rustle" { 0.28 } else { 0.12 };
            let crackle: Vec<f32> = (0..len(secs)).map(|_| if rng.next() > 0.8 { 1.0 } else { 0.25 }).collect();
            let mut v = noise(secs, &mut rng, |t| perc(t, 0.004, secs / 3.0), |_| 6000.0);
            highpass(&mut v, 1500.0);
            for (s, c) in v.iter_mut().zip(crackle) {
                *s *= c;
            }
            v
        }
        "dig" => {
            let mut v = noise(0.18, &mut rng, |t| perc(t, 0.004, 0.05), |_| 900.0);
            mix(&mut v, &noise(0.18, &mut rng, |t| perc(t, 0.004, 0.08), |_| 250.0), 0.0, 0.8);
            v
        }
        "crack" => {
            let mut v = noise(0.14, &mut rng, |t| perc(t, 0.001, 0.03), |_| 4000.0);
            mix(&mut v, &tone(0.1, |t| 1100.0 - 4000.0 * t, |t| perc(t, 0.001, 0.02), soft_square), 0.0, 0.25);
            v
        }
        "chop" => {
            let mut v = tone(0.15, |t| 140.0 - 260.0 * t, |t| perc(t, 0.001, 0.04), tri);
            mix(&mut v, &noise(0.05, &mut rng, |t| perc(t, 0.001, 0.015), |_| 2500.0), 0.0, 0.7);
            v
        }
        "shatter" => {
            let mut v = noise(0.35, &mut rng, |t| perc(t, 0.001, 0.07), |_| 9000.0);
            highpass(&mut v, 2500.0);
            for _ in 0..12 {
                let f = 3000.0 + rng.next() * 4000.0;
                let at = rng.next() * 0.2;
                mix(&mut v, &tone(0.08, move |_| f, |t| perc(t, 0.0005, 0.02), sine), at, 0.35);
            }
            v
        }
        "place" => {
            let mut v = tone(0.09, |t| 110.0 - 220.0 * t, |t| perc(t, 0.001, 0.025), sine);
            mix(&mut v, &noise(0.02, &mut rng, |t| perc(t, 0.0005, 0.006), |_| 2000.0), 0.0, 0.5);
            v
        }
        _ => vec![],
    };
    // about as loud as each other: peaks at 0.8 at most, and the steady
    // ones (squares, fanfares) brought down to the rest
    let peak = v.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    let rms = (v.iter().map(|s| s * s).sum::<f32>() / v.len().max(1) as f32).sqrt();
    if peak > 0.0 {
        let gain = (0.8 / peak).min(0.25 / rms.max(1e-6));
        for s in v.iter_mut() {
            *s *= gain;
        }
    }
    // no click at the end
    let fade = len(0.005).min(v.len());
    let n = v.len();
    for i in 0..fade {
        v[n - 1 - i] *= i as f32 / fade as f32;
    }
    v
}

// ---------- playing them ----------

/// A made sound, for Bevy's audio.
#[derive(Asset, TypePath, Clone)]
pub struct Synth(pub Arc<[f32]>);

pub struct SynthDecoder {
    data: Arc<[f32]>,
    i: usize,
}

impl Iterator for SynthDecoder {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        let s = self.data.get(self.i).copied();
        self.i += 1;
        s
    }
}

impl bevy::audio::Source for SynthDecoder {
    fn current_span_len(&self) -> Option<usize> {
        Some(self.data.len().saturating_sub(self.i))
    }
    fn channels(&self) -> bevy::audio::ChannelCount {
        NonZero::new(1).unwrap()
    }
    fn sample_rate(&self) -> bevy::audio::SampleRate {
        NonZero::new(RATE).unwrap()
    }
    fn total_duration(&self) -> Option<Duration> {
        Some(Duration::from_secs_f32(self.data.len() as f32 / RATE as f32))
    }
}

impl Decodable for Synth {
    type Decoder = SynthDecoder;
    fn decoder(&self) -> SynthDecoder {
        SynthDecoder { data: self.0.clone(), i: 0 }
    }
}

/// Every sound, made once.
#[derive(Resource, Default)]
pub struct Sounds(pub std::collections::HashMap<&'static str, Handle<Synth>>);

/// Plays sounds where there's a speaker: called from `sim` when the app
/// has Bevy's audio (the game, not the server).
pub fn add(app: &mut App) {
    app.add_audio_source::<Synth>().init_resource::<Sounds>().add_systems(Startup, make).add_systems(Last, (listen, play).chain());
}

fn make(mut sounds: ResMut<Sounds>, mut assets: ResMut<Assets<Synth>>) {
    for (name, _) in SOUNDS {
        sounds.0.insert(name, assets.add(Synth(synth(name).into())));
    }
}

/// The camera's ears, added once there's a camera.
fn listen(mut commands: Commands, cams: Query<Entity, (With<MainCamera>, Without<SpatialListener>)>) {
    for e in &cams {
        commands.entity(e).insert(SpatialListener::new(4.0));
    }
}

#[allow(clippy::too_many_arguments)]
fn play(
    mut commands: Commands,
    mut heard: ResMut<Heard>,
    sounds: Res<Sounds>,
    settings: Res<Settings>,
    time: Res<Time>,
    mut me: Option<ResMut<Me>>,
    cam: Query<&GlobalTransform, With<MainCamera>>,
    playing: Query<(), With<AudioPlayer<Synth>>>,
) {
    if let Some(me) = me.as_mut() {
        for c in me.captions.iter_mut() {
            c.1 -= time.delta_secs();
        }
        me.captions.retain(|c| c.1 > 0.0);
    }
    let list = std::mem::take(&mut heard.0);
    let ears = cam.single().ok().map(|c| (c.translation(), c.right()));
    let mut room = AT_ONCE.saturating_sub(playing.iter().count());
    for p in list {
        let Some(h) = sounds.0.get(p.name.as_str()) else { continue };
        let at = p.at.map(Vec3::from_array);
        if let (Some(at), Some((ear, _))) = (at, ears) {
            if at.distance(ear) > HEARD_WITHIN {
                continue;
            }
        }
        // subtitles, with which side it's on
        // its own words, its sound's, or none for an empty one (the menus' clicks)
        let caption = match &p.caption {
            Some(c) => Some(c.clone()).filter(|c| !c.is_empty()),
            None => caption_of(&p.name).filter(|c| !c.is_empty()).map(String::from),
        };
        if let (Some(c), Some(me), true) = (caption, me.as_mut(), settings.subtitles) {
            let side = match (at, ears) {
                (Some(at), Some((ear, right))) if at.distance(ear) > 2.0 => {
                    let x = (at - ear).normalize().dot(*right);
                    if x > 0.5 { 1 } else if x < -0.5 { -1 } else { 0 }
                }
                _ => 0,
            };
            let line = match side {
                -1 => format!("‹ {c}"),
                1 => format!("{c} ›"),
                _ => c,
            };
            me.captions.retain(|l| l.0 != line);
            me.captions.push((line, CAPTION_FOR));
            if me.captions.len() > 5 {
                me.captions.remove(0);
            }
        }
        if room == 0 || settings.volume <= 0.0 {
            continue;
        }
        room -= 1;
        let volume = (p.volume.clamp(0.0, 1.0) * settings.volume.clamp(0.0, 1.0)).max(0.0);
        commands.spawn((
            AudioPlayer::<Synth>(h.clone()),
            PlaybackSettings {
                mode: PlaybackMode::Despawn,
                volume: Volume::Linear(volume),
                speed: p.pitch.clamp(0.5, 2.0),
                spatial: at.is_some(),
                spatial_scale: Some(SpatialScale::new(SCALE)),
                ..default()
            },
            Transform::from_translation(at.unwrap_or_default()),
        ));
    }
}
