//! Bringing the world in round players as they go, and letting it go
//! behind them. Columns are made on other threads, nearest first, so
//! walking, flying or riding into new land never stalls a frame; the
//! island is always there.
//!
//! Every game makes the land round its own player (it needs it to walk on
//! and draw); the host and the server also make it round everyone who
//! joined, for their creatures and creations.

use std::collections::HashMap;

use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};

use super::{in_border, is_island, key_of, Column, Voxels, CHUNK};

/// How many columns are made at once.
const AT_ONCE: usize = 4;
/// Columns round a friend kept loaded on the host (in columns each way):
/// enough for their creatures (`net::NEAR` is 144 blocks).
pub const AROUND_FRIENDS: i32 = 5;
/// Columns this far past what's wanted stay a while, so walking back and
/// forth across a line doesn't make the same ones again and again.
const LINGER: i32 = 3;

/// How far round this game's own player the land is made, in columns each
/// way (a column past the view distance, `mesh::View`), and the columns
/// being made now.
#[derive(Resource)]
pub struct Stream {
    pub around_me: i32,
    tasks: HashMap<IVec2, Task<Column>>,
    since_tidy: f32,
    /// The world (`Voxels::id`) and seed the columns being made are for.
    world: (u64, u32),
}

impl Default for Stream {
    fn default() -> Self {
        Stream { around_me: 8, tasks: HashMap::new(), since_tidy: 0.0, world: (0, 0) }
    }
}

impl Stream {
    /// Whether nothing is being made now.
    pub fn idle(&self) -> bool {
        self.tasks.is_empty()
    }
}

/// The squares of columns wanted: (middle column, columns each way).
pub fn wanted(me: Option<Vec3>, others: &[Vec3], around_me: i32) -> Vec<(IVec2, i32)> {
    let mut out = vec![];
    if let Some(p) = me {
        out.push((key_of(p.x.floor() as i32, p.z.floor() as i32), around_me));
    }
    for p in others {
        out.push((key_of(p.x.floor() as i32, p.z.floor() as i32), AROUND_FRIENDS));
    }
    out
}

/// Makes the columns round players that aren't here yet, nearest first,
/// puts in those that are done, and lets go of the ones nobody's near.
pub fn stream(
    time: Res<Time>,
    mut voxels: ResMut<Voxels>,
    mut st: ResMut<Stream>,
    me: Option<Res<crate::player::Me>>,
    net: Res<crate::net::Net>,
) {
    if voxels.blank {
        return;
    }
    // another world (or a friend's seed): what was being made was for the old one
    if st.world != (voxels.id, voxels.seed) {
        st.world = (voxels.id, voxels.seed);
        st.tasks.clear();
    }
    let others: Vec<Vec3> = match &net.role {
        crate::net::Role::Client(_) => vec![],
        _ => net.remote_players().iter().map(|p| p.pos).collect(),
    };
    let squares = wanted(me.as_ref().map(|m| m.pos), &others, st.around_me);
    // what's done goes in
    let done: Vec<IVec2> = st.tasks.iter_mut().filter_map(|(k, t)| t.is_finished().then_some(*k)).collect();
    for k in done {
        if let Some(t) = st.tasks.remove(&k) {
            if let Some(col) = block_on(poll_once(t)) {
                voxels.insert(k, col);
            }
        }
    }
    // the next nearest to start
    if st.tasks.len() < AT_ONCE {
        let mut need: Vec<(i32, IVec2)> = vec![];
        for &(c, r) in &squares {
            for dz in -r..=r {
                for dx in -r..=r {
                    let k = c + IVec2::new(dx, dz);
                    if !in_border(k) || voxels.has(k) || st.tasks.contains_key(&k) {
                        continue;
                    }
                    need.push((dx * dx + dz * dz, k));
                }
            }
        }
        need.sort_by_key(|(d, k)| (*d, k.y, k.x));
        need.dedup_by_key(|(_, k)| *k);
        let pool = AsyncComputeTaskPool::get();
        let seed = voxels.seed;
        for (_, k) in need.into_iter().take(AT_ONCE - st.tasks.len()) {
            st.tasks.insert(k, pool.spawn(async move { super::column(seed, k) }));
        }
    }
    // now and then, let go of what nobody's near
    st.since_tidy += time.delta_secs();
    if st.since_tidy > 1.0 {
        st.since_tidy = 0.0;
        let near = |k: IVec2| squares.iter().any(|&(c, r)| (k - c).abs().max_element() <= r + LINGER);
        let gone: Vec<IVec2> = voxels.keys().filter(|k| !is_island(*k) && !near(*k)).collect();
        if !gone.is_empty() {
            for k in gone {
                voxels.unload(k);
            }
            voxels.shrink();
        }
        // and stop making what's no longer wanted
        st.tasks.retain(|k, _| near(*k));
    }
}

/// Makes every column within `r` of a spot now, on this thread: for tests,
/// tools and the server's own checks, which can't wait for frames.
pub fn make_round(voxels: &mut Voxels, at: Vec3, r: i32) {
    let c = key_of(at.x.floor() as i32, at.z.floor() as i32);
    for dz in -r..=r {
        for dx in -r..=r {
            voxels.ensure(c + IVec2::new(dx, dz));
        }
    }
}

/// The column a point is in, for the view (in chunk steps).
pub fn column_of(p: Vec3) -> IVec2 {
    IVec2::new((p.x / CHUNK as f32).floor() as i32, (p.z / CHUNK as f32).floor() as i32)
}
