//! Sounds: the game's own (steps, digging, placing, menus) and the ones
//! creations play by name with `@potluck/sound`. Every one is made from
//! numbers when the game starts, the same every time, so there are no
//! sound files to ship or share.

use std::collections::BTreeMap;
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
    // a block a creation made says, if it likes
    if let Some(k) = crate::made::kind(id) {
        return match k {
            "hard" => Material::Hard,
            "wood" => Material::Wood,
            "plant" => Material::Plant,
            "glass" => Material::Glass,
            _ => Material::Soft,
        };
    }
    named(blocks::block(id).name)
}

/// What something is made of, going by its name.
pub fn named(name: &str) -> Material {
    let n = name.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| n.contains(w));
    if has(&["water"]) {
        Material::Water
    } else if has(&["glass"]) {
        Material::Glass
    } else if has(&["leaves", "needles", "tall grass", "flower", "fern", "bush", "wheat", "carrot", "reed", "vine", "moss carpet"]) {
        Material::Plant
    } else if has(&["log", "plank", "barrel", "crate", "bookshelf", "table", "stool", "fence", "stake", "wood", "door", "basket"]) {
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
    level(&mut v);
    v
}

/// Makes a sound about as loud as the rest, with no click at the end.
fn level(v: &mut [f32]) {
    // peaks at 0.8 at most, and the steady ones (squares, fanfares)
    // brought down to the rest
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
}

// ---------- creations' own sounds ----------

/// Sounds one creation may make of its own.
pub const MOST_OWN: usize = 16;
/// How long one of them may be, in seconds.
pub const OWN_LONGEST: f32 = 3.0;
/// Layers one may have besides its first.
pub const MOST_LAYERS: usize = 3;
/// Notes one layer may play in a row.
pub const MOST_NOTES: usize = 32;
/// What a creation's sound may say how to make, as `[sounds.moo]` has it.
pub const RECIPE_KEYS: &[&str] = &["caption", "wave", "pitch", "notes", "each", "length", "attack", "decay", "wobble", "pulse", "lowpass", "highpass", "layers"];
/// What a layer may say: the same, less caption and layers, and where it starts and how loud it is.
pub const LAYER_KEYS: &[&str] = &["wave", "pitch", "notes", "each", "length", "attack", "decay", "wobble", "pulse", "lowpass", "highpass", "at", "gain"];
pub const WAVES: &[&str] = &["sine", "triangle", "saw", "square", "noise"];

/// One of a creation's own sounds, made from numbers in its potluck.toml
/// (`[sounds.moo]`) the same way on every game. The table is its first
/// layer; `layers` adds more, each starting `at` seconds in.
#[derive(Clone, Debug, PartialEq)]
pub struct Recipe {
    pub caption: String,
    pub layer: Layer,
    pub layers: Vec<Layer>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    pub wave: usize,
    /// Hz at the start and the end (for noise, how bright it is).
    pub pitch: (f32, f32),
    /// Notes in a row, in Hz, each `each` seconds long; 0 is a rest.
    pub notes: Vec<f32>,
    pub each: f32,
    pub length: f32,
    pub attack: f32,
    pub decay: f32,
    /// Times a second, and Hz up and down.
    pub wobble: (f32, f32),
    /// Times a second, and how much quieter (0 to 1).
    pub pulse: (f32, f32),
    pub lowpass: Option<f32>,
    pub highpass: Option<f32>,
    pub at: f32,
    pub gain: f32,
}

/// A note's name ("C5", "F#4", "Bb3"; "-" is a rest) in Hz.
pub fn note(name: &str) -> Option<f32> {
    let name = name.trim();
    if name == "-" {
        return Some(0.0);
    }
    let mut c = name.chars();
    let step = match c.next()?.to_ascii_uppercase() {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => return None,
    };
    let rest: String = c.collect();
    let (shift, octave) = match rest.chars().next() {
        Some('#') => (1, &rest[1..]),
        Some('b') => (-1, &rest[1..]),
        _ => (0, &rest[..]),
    };
    let octave: i32 = if octave.is_empty() { 4 } else { octave.parse().ok().filter(|o| (0..=9).contains(o))? };
    let midi = 12 * (octave + 1) + step + shift;
    Some(440.0 * 2f32.powf((midi - 69) as f32 / 12.0))
}

fn num(v: &serde_json::Value) -> Option<f32> {
    v.as_f64().map(|n| n as f32).filter(|n| n.is_finite())
}

/// Two numbers, as [a, b].
fn pair(v: &serde_json::Value) -> Option<(f32, f32)> {
    match v {
        serde_json::Value::Array(a) if a.len() == 2 => Some((num(&a[0])?, num(&a[1])?)),
        _ => None,
    }
}

fn parse_layer(what: &str, t: &serde_json::Map<String, serde_json::Value>, keys: &[&str]) -> Result<Layer, String> {
    for k in t.keys() {
        if !keys.contains(&k.as_str()) {
            return Err(format!("{what}: there's no \"{k}\" (a sound has {})", keys.join(", ")));
        }
    }
    let number = |k: &str, usual: f32, lo: f32, hi: f32| -> Result<f32, String> {
        match t.get(k) {
            None => Ok(usual),
            Some(v) => num(v).map(|n| n.clamp(lo, hi)).ok_or_else(|| format!("{what}: {k} is a number")),
        }
    };
    let wave = match t.get("wave") {
        None => 0,
        Some(v) => v.as_str().and_then(|w| WAVES.iter().position(|x| *x == w)).ok_or_else(|| format!("{what}: wave is one of {}", WAVES.join(", ")))?,
    };
    let hz = |n: f32| n.clamp(20.0, 12000.0);
    let pitch = match t.get("pitch") {
        None => (440.0, 440.0),
        Some(v) => match (num(v), pair(v)) {
            (Some(n), _) => (hz(n), hz(n)),
            (_, Some((a, b))) => (hz(a), hz(b)),
            _ => return Err(format!("{what}: pitch is a number of Hz, or [from, to]")),
        },
    };
    let notes = match t.get("notes") {
        None => vec![],
        Some(serde_json::Value::Array(a)) => {
            if a.len() > MOST_NOTES {
                return Err(format!("{what}: {MOST_NOTES} notes at most"));
            }
            a.iter()
                .map(|n| match n {
                    serde_json::Value::String(s) => note(s),
                    v => num(v).map(|n| if n <= 0.0 { 0.0 } else { hz(n) }),
                })
                .collect::<Option<Vec<f32>>>()
                .ok_or_else(|| format!("{what}: notes are names like \"C5\", \"F#4\" or \"-\" for a rest, or numbers of Hz"))?
        }
        Some(_) => return Err(format!("{what}: notes are a list, like [\"C5\", \"E5\", \"G5\"]")),
    };
    let each = number("each", 0.15, 0.02, 1.0)?;
    let length = if notes.is_empty() { number("length", 0.4, 0.02, OWN_LONGEST)? } else { (notes.len() as f32 * each).min(OWN_LONGEST) };
    let one = if notes.is_empty() { length } else { each };
    let attack = number("attack", 0.01, 0.0, one)?;
    let decay = number("decay", one / 3.0, 0.005, 10.0)?;
    let two = |k: &str, rate: f32, most: f32| -> Result<(f32, f32), String> {
        match t.get(k) {
            None => Ok((0.0, 0.0)),
            Some(v) => pair(v).map(|(a, b)| (a.clamp(0.0, rate), b.clamp(0.0, most))).ok_or_else(|| format!("{what}: {k} is [times a second, how much]")),
        }
    };
    let filter = |k: &str| -> Result<Option<f32>, String> {
        match t.get(k) {
            None => Ok(None),
            Some(v) => num(v).map(|n| Some(n.clamp(20.0, 20000.0))).ok_or_else(|| format!("{what}: {k} is a number of Hz")),
        }
    };
    Ok(Layer {
        wave,
        pitch,
        notes,
        each,
        length,
        attack,
        decay,
        wobble: two("wobble", 40.0, 2000.0)?,
        pulse: two("pulse", 40.0, 1.0)?,
        lowpass: filter("lowpass")?,
        highpass: filter("highpass")?,
        at: number("at", 0.0, 0.0, OWN_LONGEST)?,
        gain: number("gain", 1.0, 0.0, 2.0)?,
    })
}

/// Whether `name` will do for one of a creation's own sounds.
pub fn own_name_ok(name: &str) -> bool {
    !name.is_empty() && name.len() <= 24 && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// Reads one of a creation's own sounds, saying plainly what's wrong.
pub fn recipe(name: &str, v: &serde_json::Value) -> Result<Recipe, String> {
    let what = format!("sounds.{name}");
    if !own_name_ok(name) {
        return Err(format!("{what}: a sound's name is lowercase letters, digits and _"));
    }
    let serde_json::Value::Object(t) = v else {
        return Err(format!("{what}: a sound is a table, [sounds.{name}], with a wave, a pitch and a length"));
    };
    let layer = parse_layer(&what, t, RECIPE_KEYS)?;
    let caption = match t.get("caption") {
        None => {
            let words = name.replace('_', " ");
            let mut c = words.chars();
            c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
        }
        Some(serde_json::Value::String(c)) => c.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(30).collect(),
        Some(_) => return Err(format!("{what}: caption is words, what subtitles say")),
    };
    let layers = match t.get("layers") {
        None => vec![],
        Some(serde_json::Value::Array(a)) => {
            if a.len() > MOST_LAYERS {
                return Err(format!("{what}: {MOST_LAYERS} layers at most, besides the sound itself"));
            }
            let mut out = vec![];
            for (i, l) in a.iter().enumerate() {
                let what = format!("{what}.layers[{}]", i + 1);
                let serde_json::Value::Object(l) = l else {
                    return Err(format!("{what}: a layer is a table, [[sounds.{name}.layers]]"));
                };
                out.push(parse_layer(&what, l, LAYER_KEYS)?);
            }
            out
        }
        Some(_) => return Err(format!("{what}: layers are a list of tables, [[sounds.{name}.layers]]")),
    };
    Ok(Recipe { caption, layer, layers })
}

/// Reads all of a creation's own sounds.
pub fn recipes(sounds: &BTreeMap<String, serde_json::Value>) -> Result<BTreeMap<String, Recipe>, String> {
    if sounds.len() > MOST_OWN {
        return Err(format!("a creation has {MOST_OWN} sounds of its own at most, and this one has {}", sounds.len()));
    }
    sounds.iter().map(|(n, v)| Ok((n.clone(), recipe(n, v)?))).collect()
}

fn wave_of(i: usize) -> fn(f32) -> f32 {
    [sine, tri, saw, soft_square, sine][i.min(4)]
}

/// One layer's samples.
fn make_layer(l: &Layer, rng: &mut Rng) -> Vec<f32> {
    let (wobble, pulse) = (l.wobble, l.pulse);
    let shape = |t: f32, secs: f32| {
        // the attack and fall, the pulsing, and a quick fade at the end of
        // each note so notes don't click
        perc(t, l.attack, l.decay) * (1.0 - pulse.1 * (0.5 - 0.5 * (TAU * pulse.0 * t).cos())) * ((secs - t) / 0.008).clamp(0.0, 1.0)
    };
    let one = |secs: f32, from: f32, to: f32, rng: &mut Rng| -> Vec<f32> {
        let freq = move |t: f32| from * (to / from).powf((t / secs).min(1.0)) + wobble.1 * (TAU * wobble.0 * t).sin();
        if l.wave == 4 {
            noise(secs, rng, |t| shape(t, secs), |t| freq(t).max(20.0))
        } else {
            tone(secs, |t| freq(t).max(1.0), |t| shape(t, secs), wave_of(l.wave))
        }
    };
    let mut v = if l.notes.is_empty() {
        one(l.length, l.pitch.0, l.pitch.1, rng)
    } else {
        let mut v = vec![0.0; len(l.length)];
        for (i, f) in l.notes.iter().enumerate() {
            let at = i as f32 * l.each;
            if *f > 0.0 && at < l.length {
                let note = one(l.each, *f, *f, rng);
                mix(&mut v, &note, at, 1.0);
            }
        }
        v
    };
    if let Some(c) = l.lowpass {
        lowpass(&mut v, c);
    }
    if let Some(c) = l.highpass {
        highpass(&mut v, c);
    }
    v
}

/// Makes one of a creation's own sounds, the same on every game: `key` is
/// "creation/name", which its noise is seeded from.
pub fn make_own(r: &Recipe, key: &str) -> Vec<f32> {
    let mut rng = Rng::new(crate::noise::seed_of(key));
    let mut v = make_layer(&r.layer, &mut rng);
    for l in &r.layers {
        let s = make_layer(l, &mut rng);
        mix(&mut v, &s, l.at, l.gain);
    }
    v.truncate(len(OWN_LONGEST));
    level(&mut v);
    v
}

/// What a creation can play by name: its own sounds, then the game's.
#[derive(Clone, Debug, Default)]
pub struct Own {
    creation: String,
    captions: BTreeMap<String, String>,
}

impl Own {
    /// A creation's own sounds, from its potluck.toml.
    pub fn of(creation: &str, sounds: &BTreeMap<String, serde_json::Value>) -> Result<Own, String> {
        Ok(Own { creation: creation.to_string(), captions: recipes(sounds)?.into_iter().map(|(n, r)| (n, r.caption)).collect() })
    }

    /// What playing `name` sends: the name every game knows it by, and
    /// what subtitles say (None for the game's own word).
    pub fn resolve(&self, name: &str) -> Option<(String, Option<String>)> {
        match self.captions.get(name) {
            Some(c) => Some((format!("{}/{name}", self.creation), Some(c.clone()))),
            None => caption_of(name).map(|_| (name.to_string(), None)),
        }
    }

    /// Every name it can play, its own first.
    pub fn names(&self) -> Vec<String> {
        self.captions.keys().cloned().chain(SOUNDS.iter().map(|s| s.0.to_string())).collect()
    }

    /// The plain error for a name it can't play.
    pub fn unknown(name: &str) -> String {
        format!("there's no sound called \"{name}\" (sound.names() lists them, or make one of your own in potluck.toml as [sounds.{name}])")
    }
}

/// One of a creation's own sounds as it is now in the library, by
/// "creation/name".
pub fn own_recipe(lib: &crate::creations::Library, key: &str) -> Option<Recipe> {
    let (id, name) = key.split_once('/')?;
    recipe(name, lib.list.get(id)?.manifest.sounds.get(name)?).ok()
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

/// Every sound, made once: the game's at start, and creations' own the
/// first time each plays (again if its recipe changed).
#[derive(Resource, Default)]
pub struct Sounds {
    pub game: std::collections::HashMap<&'static str, Handle<Synth>>,
    own: std::collections::HashMap<String, (Recipe, Handle<Synth>)>,
}

impl Sounds {
    /// The sound to play for `name`: one of the game's, or a creation's own
    /// ("creation/name") as the library has it now.
    fn get(&mut self, name: &str, lib: Option<&crate::creations::Library>, assets: &mut Assets<Synth>) -> Option<Handle<Synth>> {
        if let Some(h) = self.game.get(name) {
            return Some(h.clone());
        }
        let r = own_recipe(lib?, name)?;
        if let Some((made, h)) = self.own.get(name) {
            if *made == r {
                return Some(h.clone());
            }
        }
        if self.own.len() >= 256 {
            self.own.clear();
        }
        let h = assets.add(Synth(make_own(&r, name).into()));
        self.own.insert(name.to_string(), (r, h.clone()));
        Some(h)
    }
}

/// Plays sounds where there's a speaker: called from `sim` when the app
/// has Bevy's audio (the game, not the server).
pub fn add(app: &mut App) {
    app.add_audio_source::<Synth>().init_resource::<Sounds>().add_systems(Startup, make).add_systems(Last, (listen, play).chain());
}

fn make(mut sounds: ResMut<Sounds>, mut assets: ResMut<Assets<Synth>>) {
    for (name, _) in SOUNDS {
        sounds.game.insert(name, assets.add(Synth(synth(name).into())));
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
    mut sounds: ResMut<Sounds>,
    mut assets: ResMut<Assets<Synth>>,
    lib: Option<Res<crate::creations::Library>>,
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
        let Some(h) = sounds.get(&p.name, lib.as_deref(), &mut assets) else { continue };
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
            AudioPlayer::<Synth>(h),
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
