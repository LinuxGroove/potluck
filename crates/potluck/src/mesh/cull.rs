//! Chunks the camera can't see through open space aren't drawn: since
//! caves, nearly every chunk underground has something in it to mesh, and
//! drawing them all from up on the grass cost more than the land in view.
//!
//! Each chunk keeps which of its six faces open space joins up inside it
//! (`connections`, worked out when it's meshed), and `cull` walks out from
//! the camera's chunk through those, never turning back toward the camera
//! (the way Minecraft's cave culling does). What the walk doesn't reach is
//! hidden. Out under the sky, a hidden chunk under the ground everywhere
//! in its column leaves the shadows too, as nothing there can shade what
//! the camera sees; anything else hidden still casts the sun's shadows
//! (`SHADOW_ONLY`), so a cave keeps the rock over it dark. Ray tracing
//! finds its own shadows and sees every chunk either way.

use super::{ChunkEntities, ChunkPart};
use crate::blocks::{self, AIR};
use crate::world::{Voxels, CHUNK, CY0, CY1};
use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use std::collections::{HashMap, HashSet, VecDeque};

/// The faces of a chunk, in this order: +x, -x, +y, -y, +z, -z (a face's
/// opposite is its number with the last bit flipped).
pub const FACES: [IVec3; 6] = [IVec3::X, IVec3::NEG_X, IVec3::Y, IVec3::NEG_Y, IVec3::Z, IVec3::NEG_Z];

/// Every face joined to every other.
pub const ALL: u64 = all();

const fn all() -> u64 {
    let mut out = 0;
    let mut a = 0;
    while a < 6 {
        let mut b = 0;
        while b < 6 {
            if a != b {
                out |= 1 << (a * 6 + b);
            }
            b += 1;
        }
        a += 1;
    }
    out
}

/// Whether open space inside a chunk joins face `a` to face `b`.
pub fn joins(c: u64, a: usize, b: usize) -> bool {
    c >> (a * 6 + b) & 1 == 1
}

/// The pairs of faces a set of faces (one bit each) joins.
fn pairs(faces: u8) -> u64 {
    let mut out = 0;
    for a in 0..6 {
        for b in 0..6 {
            if a != b && faces >> a & 1 == 1 && faces >> b & 1 == 1 {
                out |= 1 << (a * 6 + b);
            }
        }
    }
    out
}

/// Which of a chunk's faces open space (anything but solid blocks: air,
/// water, leaves, glass, plants, models) joins up inside it.
pub fn connections(w: &Voxels, c: IVec3) -> u64 {
    if let Some(b) = w.uniform(c) {
        return if blocks::is_opaque(b) { 0 } else { ALL };
    }
    const N: usize = CHUNK as usize;
    let o = c * CHUNK;
    let at = |i: usize| (i % N, i / (N * N), i / N % N);
    let mut open = vec![false; N * N * N];
    let mut any = false;
    for (i, cell) in open.iter_mut().enumerate() {
        let (x, y, z) = at(i);
        *cell = !blocks::is_opaque(w.get(o.x + x as i32, o.y + y as i32, o.z + z as i32));
        any |= *cell;
    }
    if !any {
        return 0;
    }
    let mut out = 0;
    let mut stack: Vec<usize> = Vec::with_capacity(4096);
    for start in 0..open.len() {
        if !open[start] {
            continue;
        }
        open[start] = false;
        stack.push(start);
        let mut faces = 0u8;
        while let Some(i) = stack.pop() {
            let (x, y, z) = at(i);
            faces |= (x == N - 1) as u8 | ((x == 0) as u8) << 1 | ((y == N - 1) as u8) << 2 | ((y == 0) as u8) << 3 | ((z == N - 1) as u8) << 4 | ((z == 0) as u8) << 5;
            let mut go = |j: usize| {
                if open[j] {
                    open[j] = false;
                    stack.push(j);
                }
            };
            if x > 0 {
                go(i - 1);
            }
            if x < N - 1 {
                go(i + 1);
            }
            if z > 0 {
                go(i - N);
            }
            if z < N - 1 {
                go(i + N);
            }
            if y > 0 {
                go(i - N * N);
            }
            if y < N - 1 {
                go(i + N * N);
            }
        }
        out |= pairs(faces);
        if out == ALL {
            break;
        }
    }
    out
}

/// Seen from the chunk `from`, the chunks open space reaches, among those
/// `shown` says are drawn. `join` gives a chunk's connections.
pub fn reached(from: IVec3, shown: impl Fn(IVec3) -> bool, join: impl Fn(IVec3) -> u64) -> HashSet<IVec3> {
    let from = IVec3::new(from.x, from.y.clamp(CY0, CY1 - 1), from.z);
    let mut seen = HashSet::new();
    seen.insert(from);
    let mut queue = VecDeque::new();
    // (chunk, the face it was come into by, the ways gone so far)
    for (d, step) in FACES.iter().enumerate() {
        queue.push_back((from + *step, d ^ 1, 1u8 << d));
    }
    while let Some((c, came, ways)) = queue.pop_front() {
        if c.y < CY0 || c.y >= CY1 || !shown(c) || !seen.insert(c) {
            continue;
        }
        let j = join(c);
        for (d, step) in FACES.iter().enumerate() {
            // never back toward the camera
            if ways >> (d ^ 1) & 1 == 1 || !joins(j, came, d) {
                continue;
            }
            queue.push_back((c + *step, d ^ 1, ways | 1 << d));
        }
    }
    seen
}

/// Hidden chunks that still cast the sun's shadows sit on this layer,
/// which the sun sees and the camera doesn't.
pub const SHADOW_ONLY: usize = 1;

#[derive(Resource, Default)]
pub struct Culling {
    /// What the last walk started from: the world, the camera's chunk,
    /// the chunks' connections and columns, and whether it was under cover.
    looked: (u64, IVec3, u64, u64, bool),
    /// When chunks were last meshed, to look again for new ones.
    meshed: usize,
    frames: u32,
    /// Each column's lowest ground, sampled, for whether a chunk is buried.
    lowest: HashMap<IVec2, i32>,
    /// How many chunks the last walk hid, for the timings.
    pub hidden: usize,
}

/// How a chunk's parts are drawn.
#[derive(Clone, Copy, PartialEq)]
enum Shown {
    Drawn,
    /// Not drawn, but casting the sun's shadows.
    Shadow,
    Hidden,
}

/// Hides the chunks the camera can't see (see the module's notes). Runs
/// after `remesh`, and walks again when the camera moves to another chunk,
/// a chunk's connections change, or new chunks have been meshed.
#[allow(clippy::too_many_arguments)]
pub fn cull(
    mut cull: ResMut<Culling>,
    chunks: Res<ChunkEntities>,
    voxels: Res<Voxels>,
    cover: Res<super::Cover>,
    // where `place_camera` put it this frame (the camera has no parent)
    cam: Query<&Transform, With<crate::player::MainCamera>>,
    mut parts: Query<(&mut Visibility, Option<&RenderLayers>), With<ChunkPart>>,
    mut commands: Commands,
) {
    let Ok(cam) = cam.single() else { return };
    let eye = cam.translation;
    let here = (eye / CHUNK as f32).floor().as_ivec3();
    let under = under_cover(&voxels, eye);
    let now = (voxels.id, here, chunks.connect_rev, chunks.shown_rev, under);
    let was = cull.looked;
    cull.frames += 1;
    // a move walks at once; while chunks are meshed, every few frames (new
    // ones show until then, deep under the ground)
    let moved = now.0 != was.0 || now.1 != was.1 || now.4 != was.4;
    let changed = now != was || chunks.meshed != cull.meshed;
    if !moved && !(changed && cull.frames >= 4) {
        return;
    }
    if now.0 != was.0 {
        cull.lowest.clear();
    }
    cull.looked = now;
    cull.meshed = chunks.meshed;
    cull.frames = 0;
    let seen = if cover.off.cull {
        None
    } else {
        Some(reached(here, |c| chunks.shown.contains(&IVec2::new(c.x, c.z)), |c| join_of(&chunks, &voxels, c)))
    };
    let mut hidden = 0;
    let lowest = &mut cull.lowest;
    for (c, slots) in &chunks.map {
        let want = match &seen {
            Some(seen) if !seen.contains(c) => {
                hidden += 1;
                let k = IVec2::new(c.x, c.z);
                let ground = *lowest.entry(k).or_insert_with(|| lowest_ground(&voxels, k));
                if !under && (c.y + 1) * CHUNK <= ground {
                    Shown::Hidden
                } else {
                    Shown::Shadow
                }
            }
            _ => Shown::Drawn,
        };
        for e in slots.iter().flatten() {
            let Ok((mut vis, layers)) = parts.get_mut(*e) else { continue };
            let shadow = layers.is_some_and(|l| l.intersects(&RenderLayers::layer(SHADOW_ONLY)) && !l.intersects(&RenderLayers::layer(0)));
            vis.set_if_neq(if want == Shown::Hidden { Visibility::Hidden } else { Visibility::Inherited });
            match (want == Shown::Shadow, shadow) {
                (true, false) => {
                    commands.entity(*e).insert(RenderLayers::layer(SHADOW_ONLY));
                }
                (false, true) => {
                    commands.entity(*e).remove::<RenderLayers>();
                }
                _ => {}
            }
        }
    }
    cull.hidden = hidden;
}

/// A chunk's connections: as worked out when it was meshed, or from what
/// it's made of when it had nothing to mesh; open all round when it hasn't
/// been looked at yet.
fn join_of(chunks: &ChunkEntities, voxels: &Voxels, c: IVec3) -> u64 {
    if let Some(j) = chunks.connect.get(&c) {
        return *j;
    }
    if voxels.uniform(c) != Some(AIR) && voxels.nothing_to_draw(c) {
        0
    } else {
        ALL
    }
}

/// Whether solid blocks other than a tree's are over the eye, so the sky's
/// shadows come from chunks the camera may not see (caves, houses).
fn under_cover(voxels: &Voxels, eye: Vec3) -> bool {
    let (x, z) = (eye.x.floor() as i32, eye.z.floor() as i32);
    let top = voxels.top(x, z);
    (eye.y.floor() as i32 + 1..=top).any(|y| {
        let b = voxels.get(x, y, z);
        blocks::is_opaque(b) && b != blocks::LOG
    })
}

/// About the lowest the ground comes in a column: its tops at the corners,
/// the edges' middles and the middle, less a little.
fn lowest_ground(voxels: &Voxels, k: IVec2) -> i32 {
    let o = k * CHUNK;
    let mut low = i32::MAX;
    for dz in [0, CHUNK / 2, CHUNK - 1] {
        for dx in [0, CHUNK / 2, CHUNK - 1] {
            low = low.min(voxels.top(o.x + dx, o.y + dz));
        }
    }
    low - 4
}
