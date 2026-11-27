//! What a kind of creature does by itself, before any code (world 0.10's
//! `behaviours`): wander about its home, follow a player who holds its food
//! or who used it, run from one who hit it, hop, and now and then say
//! something with a sound. Behaviours run before the creation's own
//! handlers, each tick and on each event, so its code can always do
//! something else instead.

use std::collections::HashMap;

use bevy::prelude::*;
use mlua::{Table, Value};

use crate::blocks::{self, BlockId};
use crate::creatures::{Body, Herd};
use crate::noise::Rng;
use crate::script::{bad, KindDef, PlayerInfo};
use crate::sound::Play;

/// Every behaviour there is, as `behaviours = { ... }` names them.
pub const NAMES: &[&str] = &["wander", "follow_food", "follow_when_used", "flee_when_hit", "hop_when_hit", "chatter", "sound"];
/// How far off a creature sees food in a player's hand.
pub const FOOD_REACH: f32 = 10.0;
/// How close a follower comes before it stops and looks at you.
const NEAR: f32 = 2.2;

#[derive(Clone, Debug, PartialEq)]
pub struct Behaviours {
    /// Wanders about its home when it has nothing else to do.
    pub wander: bool,
    /// Blocks it follows a player holding, from `FOOD_REACH` away.
    pub follow_food: Vec<BlockId>,
    /// Seconds it follows whoever used it.
    pub follow_when_used: f32,
    /// How far it runs from whoever hit it.
    pub flee_when_hit: f32,
    /// How high it hops when hit, as `creature:hop` has it.
    pub hop_when_hit: f32,
    /// Lines it says now and then, and when it's used.
    pub chatter: Vec<String>,
    /// The sound it makes with them, and when it's used or hit: the name
    /// games know it by (the creation's own are "creation/name") and what
    /// subtitles say, if not the game's word.
    pub sound: Option<(String, Option<String>)>,
}

/// Where one creature is with its behaviours.
#[derive(Clone, Debug, Default)]
pub struct Behaving {
    /// Who it follows for having used it, and for how much longer.
    following: Option<(u32, f32)>,
    /// Whether it went after a player last tick, to stop when they go.
    after: bool,
    /// How much longer it runs from a hit.
    fleeing: f32,
    chatter_in: f32,
}

/// Reads `behaviours = { ... }`, saying plainly what's wrong.
pub fn parse(t: &Table, own: &crate::sound::Own) -> mlua::Result<Behaviours> {
    for pair in t.pairs::<Value, Value>() {
        let (k, _) = pair?;
        let Value::String(k) = k else {
            return Err(bad("behaviours are named: behaviours = { wander = true, flee_when_hit = 6 }"));
        };
        let k = k.to_str()?.to_string();
        if !NAMES.contains(&k.as_str()) {
            return Err(bad(format!("there's no behaviour called \"{k}\" (there are: {})", NAMES.join(", "))));
        }
    }
    // a name, or a list of them
    let names = |key: &str| -> mlua::Result<Vec<String>> {
        Ok(match t.get::<Value>(key)? {
            Value::Nil | Value::Boolean(false) => vec![],
            Value::String(s) => vec![s.to_str()?.to_string()],
            Value::Table(list) => list.sequence_values::<String>().collect::<mlua::Result<_>>()?,
            _ => return Err(bad(format!("{key} takes a name, or a list of them"))),
        })
    };
    // true for the usual amount, a number for another, false or nothing for none
    let amount = |key: &str, usual: f32, most: f32| -> mlua::Result<f32> {
        Ok(match t.get::<Value>(key)? {
            Value::Nil | Value::Boolean(false) => 0.0,
            Value::Boolean(true) => usual,
            Value::Integer(i) => (i as f32).clamp(0.0, most),
            Value::Number(n) if n.is_finite() => (n as f32).clamp(0.0, most),
            _ => return Err(bad(format!("{key} takes true, or a number up to {most}"))),
        })
    };
    let mut follow_food = Vec::new();
    for n in names("follow_food")? {
        follow_food.push(blocks::by_name(&n).ok_or_else(|| bad(format!("follow_food: there's no block called \"{n}\"")))?);
    }
    let sound = match t.get::<Option<String>>("sound")? {
        Some(s) => Some(own.resolve(&s).ok_or_else(|| bad(crate::sound::Own::unknown(&s)))?),
        None => None,
    };
    Ok(Behaviours {
        wander: t.get::<Option<bool>>("wander")?.unwrap_or(true),
        follow_food,
        follow_when_used: amount("follow_when_used", 20.0, 300.0)?,
        flee_when_hit: amount("flee_when_hit", 6.0, 16.0)?,
        hop_when_hit: amount("hop_when_hit", 0.6, 3.0)?,
        chatter: names("chatter")?.into_iter().take(16).map(|l| l.chars().take(80).collect()).collect(),
        sound,
    })
}

fn flat(v: Vec3) -> Vec3 {
    Vec3::new(v.x, 0.0, v.z)
}

fn face(b: &mut Body, at: Vec3) {
    let d = at - b.pos;
    if d.x.abs() + d.z.abs() > 0.01 {
        b.face = Some(d.x.atan2(d.z) + std::f32::consts::PI);
    }
}

fn sound((name, caption): &(String, Option<String>), b: &Body, pitch: f32) -> Play {
    Play { name: name.clone(), at: Some(b.pos.to_array()), volume: 0.8, pitch, caption: caption.clone() }
}

fn line(be: &Behaviours, rng: &mut Rng) -> Option<String> {
    let n = be.chatter.len();
    (n > 0).then(|| be.chatter[(rng.next() * n as f32) as usize % n].clone())
}

/// One tick of what a creation's creatures with behaviours do.
#[allow(clippy::too_many_arguments)]
pub fn step(herd: &mut Herd, creation: &str, kinds: &[KindDef], players: &[PlayerInfo], held: &HashMap<u32, BlockId>, state: &mut HashMap<u64, Behaving>, rng: &mut Rng, dt: f32, sounds: &mut Vec<Play>) {
    for b in herd.bodies.values_mut().filter(|b| b.creation == creation) {
        let Some(k) = kinds.iter().find(|k| k.name == b.kind) else { continue };
        let Some(be) = &k.behaviours else { continue };
        let st = state.entry(b.id).or_insert_with(|| Behaving { chatter_in: 8.0 + rng.next() * 20.0, ..default() });
        // now and then, something to say
        if !be.chatter.is_empty() || be.sound.is_some() {
            st.chatter_in -= dt;
            if st.chatter_in <= 0.0 {
                st.chatter_in = 10.0 + rng.next() * 25.0;
                if let Some(l) = line(be, rng) {
                    b.says = Some((l, 3.0));
                }
                if let Some(s) = &be.sound {
                    sounds.push(sound(s, b, 0.9 + rng.next() * 0.2));
                }
            }
        }
        // its rider has it
        if b.rider.is_some() {
            continue;
        }
        if st.fleeing > 0.0 {
            st.fleeing -= dt;
            if st.fleeing > 0.0 && b.goal.is_some() {
                continue;
            }
            st.fleeing = 0.0;
            b.speed = k.speed;
        }
        // someone to go to: who used it, or the nearest holding its food
        let mut to = None;
        if let Some((pid, left)) = st.following.as_mut() {
            *left -= dt;
            match players.iter().find(|p| p.id == *pid) {
                Some(p) if *left > 0.0 => to = Some(p.pos),
                _ => st.following = None,
            }
        }
        if to.is_none() && !be.follow_food.is_empty() {
            to = players
                .iter()
                .filter(|p| held.get(&p.id).is_some_and(|h| be.follow_food.contains(h)))
                .map(|p| (flat(p.pos - b.pos).length(), p.pos))
                .filter(|(d, _)| *d <= FOOD_REACH)
                .min_by(|a, c| a.0.total_cmp(&c.0))
                .map(|(_, p)| p);
        }
        match to {
            Some(p) => {
                b.wander = false;
                if flat(p - b.pos).length() > NEAR {
                    b.goal = Some(p);
                } else {
                    b.goal = None;
                    face(b, p);
                }
                st.after = true;
            }
            None => {
                if std::mem::take(&mut st.after) {
                    // they went, or put its food away
                    b.goal = None;
                }
                if b.goal.is_none() {
                    b.wander = be.wander;
                }
            }
        }
    }
}

/// A player used or hit one of a creation's creatures.
pub fn react(b: &mut Body, k: &KindDef, event: &str, player: &PlayerInfo, state: &mut HashMap<u64, Behaving>, rng: &mut Rng, sounds: &mut Vec<Play>) {
    let Some(be) = &k.behaviours else { return };
    let st = state.entry(b.id).or_insert_with(|| Behaving { chatter_in: 8.0 + rng.next() * 20.0, ..default() });
    match event {
        "used" => {
            if be.follow_when_used > 0.0 {
                st.following = Some((player.id, be.follow_when_used));
            }
            if let Some(l) = line(be, rng) {
                b.says = Some((l, 3.0));
            }
            if let Some(s) = &be.sound {
                sounds.push(sound(s, b, 1.15));
            }
        }
        "hit" => {
            st.following = None;
            st.after = false;
            if be.hop_when_hit > 0.0 && b.on_ground {
                b.vel.y = 7.0 * be.hop_when_hit;
            }
            if be.flee_when_hit > 0.0 && k.speed > 0.0 && b.rider.is_none() {
                // straight away from them, however close they stood
                let away = flat(b.pos - player.pos);
                let away = if away.length() < 0.1 { Vec3::X } else { away.normalize() };
                b.goal = Some(b.pos + away * be.flee_when_hit);
                b.wander = false;
                b.speed = (k.speed * 1.8).min(20.0);
                st.fleeing = be.flee_when_hit / b.speed + 1.0;
            }
            if let Some(s) = &be.sound {
                sounds.push(sound(s, b, 1.3));
            }
        }
        _ => {}
    }
}
