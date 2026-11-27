//! The Green's four arches. They show what the world's players brought into
//! it first, newest last; the rest come from the market: favourites of jams
//! that ended lately, entries in this week's jam, then what's trending (for
//! younger players, only what's checked for all ages). Walk up to one and
//! press Interact to bring it into the world.

use bevy::prelude::*;

use crate::creations::{Library, Origin};
use crate::share::Pick;
use crate::world::{portal_xs, GREEN_Y, PORTAL_Z};

/// How often to ask the market again while playing, in seconds.
pub const ASK_EVERY: f32 = 30.0 * 60.0;
/// How far from an arch's opening Interact reaches it.
pub const REACH: f32 = 6.0;

#[derive(Resource, Default)]
pub struct Green {
    /// What the market offered, best first.
    pub picks: Vec<Pick>,
    /// When the market was last asked, if it has been.
    pub asked: Option<std::time::Instant>,
    /// A code being fetched for an arch: bring it in when it comes.
    pub fetching: Option<String>,
}

/// What one arch shows.
#[derive(Clone, Debug, PartialEq)]
pub enum Shows {
    /// A creation running in this world (its library id).
    Here(String),
    /// One the market offers, not in this world yet.
    Pick(Pick),
}

impl Green {
    /// What each arch shows, west to east.
    pub fn arches(&self, lib: &Library) -> [Option<Shows>; 4] {
        let mut out: Vec<Shows> = vec![];
        let brought: Vec<&String> = lib.running.iter().filter(|id| lib.list.get(*id).is_some_and(|c| c.origin != Origin::Base)).collect();
        // the newest four players brought, in the order they came
        for id in brought.iter().skip(brought.len().saturating_sub(4)) {
            out.push(Shows::Here((*id).clone()));
        }
        for p in &self.picks {
            if out.len() >= 4 {
                break;
            }
            let running = lib.running.iter().any(|id| lib.list.get(id).is_some_and(|c| c.code.as_deref() == Some(p.code.as_str())));
            if !running && !out.iter().any(|s| matches!(s, Shows::Pick(q) if q.code == p.code)) {
                out.push(Shows::Pick(p.clone()));
            }
        }
        std::array::from_fn(|i| out.get(i).cloned())
    }
}

/// The middle of an arch's opening.
pub fn arch_at(i: usize) -> Vec3 {
    Vec3::new(portal_xs()[i] as f32 + 0.5, GREEN_Y as f32 + 3.0, PORTAL_Z as f32 + 0.5)
}

/// The arch a look from `eye` along `dir` lands on, nearer than `block_t`
/// (the block in the crosshair) and within reach.
pub fn aimed(eye: Vec3, dir: Vec3, block_t: f32) -> Option<usize> {
    let z = PORTAL_Z as f32 + 0.5;
    if dir.z.abs() < 1e-3 {
        return None;
    }
    let t = (z - eye.z) / dir.z;
    if t <= 0.0 || t > REACH || t > block_t + 0.75 {
        return None;
    }
    let p = eye + dir * t;
    let y0 = GREEN_Y as f32 + 1.0;
    (0..4).find(|&i| {
        let x = portal_xs()[i] as f32 + 0.5;
        (p.x - x).abs() <= 3.5 && p.y >= y0 && p.y <= y0 + 7.0
    })
}

/// The label over an arch, and what the HUD says when it's aimed at.
pub fn describe(s: &Shows, lib: &Library) -> (String, String) {
    match s {
        Shows::Here(id) => {
            let (name, by) = lib.list.get(id).map(|c| (c.manifest.name.clone(), c.manifest.by.clone())).unwrap_or_default();
            (format!("{name}\nby {by}"), format!("{name} by {by}\nIn this world"))
        }
        Shows::Pick(p) => (format!("{}\n{}", p.name, p.why), format!("{} by {}\n{}", p.name, p.by, p.why)),
    }
}
