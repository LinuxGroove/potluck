//! The world: endless columns of blocks made from a seed, and the changes
//! players and creations make to it. The island everyone starts on (0 to
//! `WX` and `WZ`) is made whole and is always there; past it the world goes
//! on in every direction (`wild`), a column at a time as players come near
//! (`stream`), and columns nobody is near are let go again. A column
//! someone changed is kept (`Voxels::kept`), so it comes back as they left
//! it, and it's what saves and friends who join are sent: the rest comes
//! from the seed.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use bevy::prelude::*;

use crate::blocks::{self, *};
use crate::noise::{fbm, hash2, smooth, Rng};

mod column;
mod places;
pub mod stream;
pub mod wild;

pub use column::Column;
pub use wild::{biome, climate, Biome, Climate};

/// The island's size, west to east and north to south.
pub const WX: i32 = 384;
pub const WZ: i32 = 384;
/// The top of the world (one past the highest block).
pub const WY: i32 = 128;
/// The bottom of the world: bedrock.
pub const MIN_Y: i32 = -128;
pub const SEA: i32 = 19;
/// Chunks are cubes of this many blocks, meshed one at a time; a column is
/// a chunk across and the world high.
pub const CHUNK: i32 = 32;
/// The sections of a column, bottom to top.
pub const SECTIONS: usize = ((WY - MIN_Y) / CHUNK) as usize;
/// The lowest and one past the highest chunk row.
pub const CY0: i32 = MIN_Y / CHUNK;
pub const CY1: i32 = WY / CHUNK;
/// The island's columns.
pub const CX: i32 = WX / CHUNK;
pub const CZ: i32 = WZ / CHUNK;
/// How far the world goes from the middle in each direction: past this
/// the numbers that place things get too coarse to draw them steadily.
pub const BORDER: i32 = 30_000;
/// Which way of making the world this is. Saves say which they were made
/// with; bump it when what the seed makes changes. Columns players changed
/// are kept whole, so a new version only changes ground nobody touched.
pub const GEN: u32 = 1;
/// Where deep stone (slate) starts, about.
pub const SLATE_Y: i32 = -40;

/// Places in the world that the terrain, the Green and creations know by name.
pub struct Spot {
    pub x: f32,
    pub z: f32,
}
pub const GREEN: Spot = Spot { x: 192.0, z: 244.0 };
pub const GREEN_Y: i32 = 23;
pub const GREEN_R: f32 = 24.0;
const LAKE: (f32, f32, f32) = (244.0, 180.0, 34.0);
const HILL: Spot = Spot { x: 188.0, z: 310.0 };
/// Where a new player first stands, on the hill south of the Green.
pub const START: Spot = Spot { x: 186.0, z: 286.0 };
pub const PORTAL_Z: i32 = GREEN.z as i32 - 13;

/// The column a block is in.
#[inline]
pub fn key_of(x: i32, z: i32) -> IVec2 {
    IVec2::new(x >> 5, z >> 5)
}

/// Whether a column is the island's (always there, made whole).
pub fn is_island(key: IVec2) -> bool {
    key.x >= 0 && key.y >= 0 && key.x < CX && key.y < CZ
}

/// Whether a column is inside the world's border.
pub fn in_border(key: IVec2) -> bool {
    key.x.abs() * CHUNK < BORDER && key.y.abs() * CHUNK < BORDER
}

struct Slot {
    key: IVec2,
    col: Column,
    /// Differs from what the seed makes.
    touched: bool,
    /// When it came in (`Voxels::serial`).
    serial: u64,
}

const NONE: u32 = u32::MAX;
/// The widest the dense look-up grid grows, in columns: past it, columns
/// are found by hashing (players very far apart on a server).
const GRID_MAX: i32 = 512;

/// A dense look-up from column to slot over the box round what's loaded,
/// since `get` is called for nearly every block meshed.
#[derive(Default)]
struct Grid {
    lo: IVec2,
    size: IVec2,
    cells: Vec<u32>,
}

impl Grid {
    #[inline]
    fn cell(&self, k: IVec2) -> Option<usize> {
        let d = k - self.lo;
        ((d.x as u32) < self.size.x as u32 && (d.y as u32) < self.size.y as u32).then(|| (d.y * self.size.x + d.x) as usize)
    }
}

#[derive(Resource)]
pub struct Voxels {
    pub seed: u32,
    slots: Vec<Option<Slot>>,
    free: Vec<u32>,
    index: HashMap<IVec2, u32>,
    grid: Grid,
    /// Columns someone changed that aren't loaded now, as `Column::encode`
    /// wrote them.
    kept: HashMap<IVec2, Vec<u8>>,
    /// Changes from the host to columns this game hasn't made yet, put in
    /// when they are.
    pending: HashMap<IVec2, Vec<(IVec3, BlockId)>>,
    /// Counts columns coming in: each loaded column keeps the count it came
    /// in at, so the map and the meshes know what's new.
    pub serial: u64,
    /// Chunks whose mesh is out of date.
    pub dirty: HashSet<IVec3>,
    /// Chunks whose sky a change may have opened or closed (`mesh::Sky`),
    /// remeshed a few at a time once the dirty ones are done.
    pub shade: HashSet<IVec3>,
    /// Block changes since the last network send: (position, block).
    pub changes: Vec<(IVec3, BlockId)>,
    /// Lantern positions, for the lights near the player.
    pub lanterns: HashSet<IVec3>,
    /// Which world this is: a new number for each one made or received, so
    /// the map knows to draw it again (`map::MapImage`).
    pub id: u64,
    /// Columns (x, z) whose blocks changed since the map last looked.
    pub map_dirty: HashSet<IVec2>,
    /// Nothing is made from the seed: new columns are empty air (tests and
    /// scratch worlds; `generate` makes real ones).
    pub blank: bool,
}

/// The next `Voxels::id`.
static WORLDS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

impl Voxels {
    /// A world with nothing in it: columns are made as air when something
    /// goes in them.
    pub fn empty(seed: u32) -> Self {
        Self {
            seed,
            slots: Vec::new(),
            free: Vec::new(),
            index: HashMap::new(),
            grid: Grid::default(),
            kept: HashMap::new(),
            pending: HashMap::new(),
            serial: 0,
            dirty: HashSet::new(),
            shade: HashSet::new(),
            changes: Vec::new(),
            lanterns: HashSet::new(),
            id: WORLDS.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            map_dirty: HashSet::new(),
            blank: true,
        }
    }

    /// Bytes the blocks take, for the memory report.
    pub fn bytes(&self) -> usize {
        self.slots.iter().flatten().map(|s| s.col.bytes()).sum::<usize>() + self.kept.values().map(|k| k.len()).sum::<usize>()
    }

    /// Whether a block's place is in the world at all (between bedrock and
    /// the sky, inside the border).
    pub fn inside(x: i32, y: i32, z: i32) -> bool {
        y >= MIN_Y && y < WY && x.abs() < BORDER && z.abs() < BORDER
    }

    #[inline]
    fn slot(&self, k: IVec2) -> Option<&Slot> {
        let i = match self.grid.cell(k) {
            Some(c) => self.grid.cells[c],
            None if self.index.len() > 0 => *self.index.get(&k)?,
            None => return None,
        };
        if i == NONE {
            return None;
        }
        self.slots[i as usize].as_ref()
    }

    fn slot_mut(&mut self, k: IVec2) -> Option<&mut Slot> {
        let i = *self.index.get(&k)?;
        self.slots[i as usize].as_mut()
    }

    /// Whether the column holding (x, z) is here now.
    #[inline]
    pub fn loaded(&self, x: i32, z: i32) -> bool {
        self.slot(key_of(x, z)).is_some()
    }

    pub fn has(&self, key: IVec2) -> bool {
        self.slot(key).is_some()
    }

    /// A loaded column.
    pub fn column(&self, key: IVec2) -> Option<&Column> {
        self.slot(key).map(|s| &s.col)
    }

    /// The columns loaded now.
    pub fn keys(&self) -> impl Iterator<Item = IVec2> + '_ {
        self.slots.iter().flatten().map(|s| s.key)
    }

    /// The columns loaded now that came in after `serial`.
    pub fn since(&self, serial: u64) -> impl Iterator<Item = IVec2> + '_ {
        self.slots.iter().flatten().filter(move |s| s.serial > serial).map(|s| s.key)
    }

    /// Columns someone changed, loaded or not.
    pub fn touched(&self) -> Vec<IVec2> {
        let mut out: Vec<IVec2> = self.slots.iter().flatten().filter(|s| s.touched).map(|s| s.key).chain(self.kept.keys().copied()).collect();
        out.sort_by_key(|k| (k.y, k.x));
        out
    }

    #[inline]
    pub fn get(&self, x: i32, y: i32, z: i32) -> BlockId {
        if y < MIN_Y {
            return BEDROCK;
        }
        if y >= WY {
            return AIR;
        }
        match self.slot(key_of(x, z)) {
            Some(s) => s.col.get(x & (CHUNK - 1), y, z & (CHUNK - 1)),
            None => AIR,
        }
    }

    pub fn at(&self, p: IVec3) -> BlockId {
        self.get(p.x, p.y, p.z)
    }

    /// A chunk (by chunk coordinates) that's all one block, if it is.
    pub fn uniform(&self, c: IVec3) -> Option<BlockId> {
        if c.y < CY0 {
            return Some(BEDROCK);
        }
        if c.y >= CY1 {
            return Some(AIR);
        }
        self.slot(IVec2::new(c.x, c.z))?.col.uniform((c.y - CY0) as usize)
    }

    /// Whether a chunk (by chunk coordinates) is solid blocks right through.
    fn solid(&self, c: IVec3) -> bool {
        if c.y < CY0 {
            return true;
        }
        if c.y >= CY1 {
            return false;
        }
        self.slot(IVec2::new(c.x, c.z)).is_some_and(|s| s.col.solid((c.y - CY0) as usize))
    }

    /// Whether a chunk has nothing to draw: all air, or solid right through
    /// with solid all round it (the rock deep down).
    pub fn nothing_to_draw(&self, c: IVec3) -> bool {
        self.uniform(c) == Some(AIR) || self.solid(c) && [IVec3::X, IVec3::NEG_X, IVec3::Y, IVec3::NEG_Y, IVec3::Z, IVec3::NEG_Z].iter().all(|d| self.solid(c + *d))
    }

    /// Changes a block in play: remeshes what it touches and records the
    /// change. A column that isn't here yet is made first.
    pub fn set(&mut self, p: IVec3, id: BlockId) -> bool {
        if !Self::inside(p.x, p.y, p.z) {
            return false;
        }
        if p.y == MIN_Y && id != BEDROCK {
            return false;
        }
        self.ensure(key_of(p.x, p.z));
        if self.at(p) == id {
            return false;
        }
        self.apply(p, id);
        self.changes.push((p, id));
        true
    }

    /// Changes a block that arrived from the network (not sent on again).
    /// A change to a column this game hasn't made yet waits for it.
    pub fn apply(&mut self, p: IVec3, id: BlockId) {
        if !Self::inside(p.x, p.y, p.z) {
            return;
        }
        let key = key_of(p.x, p.z);
        if !self.has(key) {
            if self.kept.contains_key(&key) {
                self.ensure(key);
            } else {
                self.pending.entry(key).or_default().push((p, id));
                return;
            }
        }
        let Some(slot) = self.slot_mut(key) else { return };
        let old = slot.col.get(p.x & (CHUNK - 1), p.y, p.z & (CHUNK - 1));
        slot.col.put(p.x & (CHUNK - 1), p.y, p.z & (CHUNK - 1), id);
        slot.touched = true;
        if self.map_dirty.len() < 65536 {
            self.map_dirty.insert(IVec2::new(p.x, p.z));
        }
        if blocks::block(old).light {
            self.lanterns.remove(&p);
        }
        if blocks::block(id).light {
            self.lanterns.insert(p);
        }
        // the chunk and any neighbour the block touches (for faces and AO)
        let chunks = |d: IVec3, u: IVec3| {
            let lo = (p - d).div_euclid(IVec3::splat(CHUNK)).max(IVec3::new(i32::MIN, CY0, i32::MIN));
            let hi = (p + u).div_euclid(IVec3::splat(CHUNK)).min(IVec3::new(i32::MAX, CY1 - 1, i32::MAX));
            (lo.z..=hi.z).flat_map(move |z| (lo.y..=hi.y).flat_map(move |y| (lo.x..=hi.x).map(move |x| IVec3::new(x, y, z))))
        };
        self.dirty.extend(chunks(IVec3::ONE, IVec3::ONE));
        // and, soon after, any whose sky it can block (below and across)
        let r = crate::mesh::SKY_REACH + 1;
        for c in chunks(IVec3::splat(r), IVec3::new(r, 1, r)) {
            if !self.dirty.contains(&c) {
                self.shade.insert(c);
            }
        }
    }

    /// Makes sure a column is here, making it from the seed (or from what
    /// was kept of it) if it isn't. Takes a few milliseconds when it does.
    pub fn ensure(&mut self, key: IVec2) {
        if !self.has(key) && in_border(key) {
            let col = if self.kept.contains_key(&key) || self.blank { Column::default() } else { column(self.seed, key) };
            self.insert(key, col);
        }
    }

    /// Puts in a column made from the seed (by `ensure` or `stream`): what
    /// was kept of it instead, if anyone changed it, and any changes that
    /// came for it while it wasn't here.
    pub fn insert(&mut self, key: IVec2, col: Column) {
        if self.has(key) {
            return;
        }
        let (mut col, mut touched) = match self.kept.remove(&key).and_then(|k| Column::decode(&k)) {
            Some(c) => (c, true),
            None => (col, false),
        };
        if let Some(list) = self.pending.remove(&key) {
            for (p, id) in list {
                col.put(p.x & (CHUNK - 1), p.y, p.z & (CHUNK - 1), id);
            }
            touched = true;
        }
        self.serial += 1;
        let o = key * CHUNK;
        col.each(|b| blocks::block(b).light, |x, y, z, _| {
            self.lanterns.insert(IVec3::new(o.x + x, y, o.y + z));
        });
        let slot = Slot { key, col, touched, serial: self.serial };
        let i = match self.free.pop() {
            Some(i) => {
                self.slots[i as usize] = Some(slot);
                i
            }
            None => {
                self.slots.push(Some(slot));
                (self.slots.len() - 1) as u32
            }
        };
        self.index.insert(key, i);
        match self.grid.cell(key) {
            Some(c) => self.grid.cells[c] = i,
            None => self.regrid(),
        }
    }

    /// Lets a column go (nobody's near it): kept if anyone changed it. The
    /// island stays.
    pub fn unload(&mut self, key: IVec2) {
        if is_island(key) {
            return;
        }
        let Some(i) = self.index.remove(&key) else { return };
        let Some(slot) = self.slots[i as usize].take() else { return };
        self.free.push(i);
        if let Some(c) = self.grid.cell(key) {
            self.grid.cells[c] = NONE;
        }
        if slot.touched {
            let mut bytes = Vec::new();
            slot.col.encode(&mut bytes);
            self.kept.insert(key, bytes);
        }
        let (x0, z0) = (key.x * CHUNK, key.y * CHUNK);
        self.lanterns.retain(|p| p.x < x0 || p.x >= x0 + CHUNK || p.z < z0 || p.z >= z0 + CHUNK);
        self.dirty.retain(|c| c.x != key.x || c.z != key.y);
        self.shade.retain(|c| c.x != key.x || c.z != key.y);
    }

    /// Fits the look-up grid round what's loaded now.
    fn regrid(&mut self) {
        let mut lo = IVec2::MAX;
        let mut hi = IVec2::MIN;
        for k in self.index.keys() {
            lo = lo.min(*k);
            hi = hi.max(*k);
        }
        if self.index.is_empty() {
            self.grid = Grid::default();
            return;
        }
        // centred on the island when it's all too wide
        let size = (hi - lo + IVec2::ONE).min(IVec2::splat(GRID_MAX));
        if hi.x - lo.x + 1 > GRID_MAX {
            lo.x = (CX / 2 - GRID_MAX / 2).max(lo.x);
        }
        if hi.y - lo.y + 1 > GRID_MAX {
            lo.y = (CZ / 2 - GRID_MAX / 2).max(lo.y);
        }
        let mut grid = Grid { lo, size, cells: vec![NONE; (size.x * size.y) as usize] };
        for (k, i) in &self.index {
            if let Some(c) = grid.cell(*k) {
                grid.cells[c] = *i;
            }
        }
        self.grid = grid;
    }

    /// Tidies the look-up grid after columns went (players moved away).
    pub fn shrink(&mut self) {
        self.regrid();
    }

    /// The highest block that isn't air, water or leaves. For a column
    /// that isn't here, where the seed puts the ground.
    pub fn top(&self, x: i32, z: i32) -> i32 {
        let Some(s) = self.slot(key_of(x, z)) else {
            return ground_height(self.seed, x, z);
        };
        let (lx, lz) = (x & (CHUNK - 1), z & (CHUNK - 1));
        for si in (0..SECTIONS).rev() {
            if let Some(b) = s.col.uniform(si) {
                if b == AIR || b == WATER {
                    continue;
                }
            }
            let y0 = MIN_Y + si as i32 * CHUNK;
            for y in (y0..y0 + CHUNK).rev() {
                let b = s.col.get(lx, y, lz);
                if b != AIR && b != WATER && !matches!(b, LEAVES | PINE | BIRCH_LEAVES) && blocks::shape(b) != Shape::Cross {
                    return y;
                }
            }
        }
        MIN_Y
    }

    /// Marks every loaded chunk to be meshed again.
    pub fn mark_all_dirty(&mut self) {
        let keys: Vec<IVec2> = self.keys().collect();
        for k in keys {
            for y in CY0..CY1 {
                self.dirty.insert(IVec3::new(k.x, y, k.y));
            }
        }
    }

    fn find_lanterns(&mut self) {
        let mut found = HashSet::new();
        for s in self.slots.iter().flatten() {
            let o = s.key * CHUNK;
            s.col.each(|b| blocks::block(b).light, |x, y, z, _| {
                found.insert(IVec3::new(o.x + x, y, o.y + z));
            });
        }
        self.lanterns = found;
    }

    /// Calls `f` with each loaded block whose id is `from` or more, and
    /// where it is (blocks creations made, when their looks change:
    /// `made::draw`).
    pub fn each_from(&self, from: BlockId, mut f: impl FnMut(IVec3, BlockId)) {
        for s in self.slots.iter().flatten() {
            let o = s.key * CHUNK;
            s.col.each(|b| b >= from, |x, y, z, b| f(IVec3::new(o.x + x, y, o.y + z), b));
        }
    }

    /// Gives blocks new ids: a saved world's made blocks, when another
    /// block has theirs here (`made::restore`).
    pub fn renumber(&mut self, to: &[BlockId; 256]) {
        for s in self.slots.iter_mut().flatten() {
            s.col.renumber(to);
        }
        for bytes in self.kept.values_mut() {
            if let Some(mut c) = Column::decode(bytes) {
                c.renumber(to);
                bytes.clear();
                c.encode(bytes);
            }
        }
        for list in self.pending.values_mut() {
            for (_, b) in list.iter_mut() {
                *b = to[*b as usize];
            }
        }
        self.find_lanterns();
        self.mark_all_dirty();
    }

    /// What friends who join and saves need besides the seed: every column
    /// someone changed, whole.
    pub fn encode(&self) -> Vec<u8> {
        let touched = self.touched();
        let mut out = Vec::with_capacity(1 << 16);
        out.extend_from_slice(b"PWC");
        out.extend_from_slice(&GEN.to_le_bytes());
        out.extend_from_slice(&(touched.len() as u32).to_le_bytes());
        for k in touched {
            out.extend_from_slice(&k.x.to_le_bytes());
            out.extend_from_slice(&k.y.to_le_bytes());
            let mut col = Vec::new();
            match self.slot(k) {
                Some(s) => s.col.encode(&mut col),
                None => col.extend_from_slice(&self.kept[&k]),
            }
            out.extend_from_slice(&(col.len() as u32).to_le_bytes());
            out.extend_from_slice(&col);
        }
        out
    }

    /// The world `encode` described, made from its seed with the changed
    /// columns put back; None if the bytes are damaged.
    pub fn decode(seed: u32, bytes: &[u8]) -> Option<Self> {
        let mut v = generate(seed);
        if !bytes.starts_with(b"PWC") || bytes.len() < 11 {
            return None;
        }
        let u32_at = |i: usize| bytes.get(i..i + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()));
        let n = u32_at(7)? as usize;
        let mut i = 11;
        for _ in 0..n {
            let x = u32_at(i)? as i32;
            let z = u32_at(i + 4)? as i32;
            let len = u32_at(i + 8)? as usize;
            let data = bytes.get(i + 12..i + 12 + len)?;
            let col = Column::decode(data)?;
            v.put_whole(IVec2::new(x, z), col);
            i += 12 + len;
        }
        if i != bytes.len() {
            return None;
        }
        Some(v)
    }

    /// Puts a whole column someone changed in place of the one here, or
    /// keeps it for when it's needed (past the island).
    fn put_whole(&mut self, key: IVec2, col: Column) {
        if is_island(key) {
            self.unload_island(key);
            self.insert(key, col);
            if let Some(s) = self.slot_mut(key) {
                s.touched = true;
            }
        } else {
            self.unload(key);
            let mut bytes = Vec::new();
            col.encode(&mut bytes);
            self.kept.insert(key, bytes);
        }
    }

    fn unload_island(&mut self, key: IVec2) {
        if let Some(i) = self.index.remove(&key) {
            self.slots[i as usize] = None;
            self.free.push(i);
            if let Some(c) = self.grid.cell(key) {
                self.grid.cells[c] = NONE;
            }
            let (x0, z0) = (key.x * CHUNK, key.y * CHUNK);
            self.lanterns.retain(|p| p.x < x0 || p.x >= x0 + CHUNK || p.z < z0 || p.z >= z0 + CHUNK);
        }
    }

    /// The island as saves before the endless world kept it (for tests of
    /// loading those): y from 0 to 128, bedrock at 0, run-length encoded.
    #[cfg(test)]
    pub fn encode_island(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let mut run: Option<(BlockId, u16)> = None;
        for y in 0..WY {
            for z in 0..WZ {
                for x in 0..WX {
                    let b = if y == 0 { BEDROCK } else { self.get(x, y, z) };
                    match &mut run {
                        Some((rb, n)) if *rb == b && *n < u16::MAX => *n += 1,
                        _ => {
                            if let Some((rb, n)) = run.take() {
                                out.push(rb);
                                out.extend_from_slice(&n.to_le_bytes());
                            }
                            run = Some((b, 1));
                        }
                    }
                }
            }
        }
        if let Some((rb, n)) = run {
            out.push(rb);
            out.extend_from_slice(&n.to_le_bytes());
        }
        out
    }

    /// A world as saves before the endless world kept it: the whole island
    /// run-length encoded, y from 0 to 128. Its columns that differ from
    /// what the seed makes now are kept whole; bedrock at the old floor is
    /// stone and the deep goes on below it.
    pub fn decode_island(seed: u32, bytes: &[u8]) -> Option<Self> {
        let (old_h, n) = (128usize, (WX * 128 * WZ) as usize);
        let mut old = vec![AIR; n];
        let mut i = 0usize;
        for run in bytes.chunks_exact(3) {
            let k = u16::from_le_bytes([run[1], run[2]]) as usize;
            if i + k > n {
                return None;
            }
            old[i..i + k].fill(run[0]);
            i += k;
        }
        if i != n || bytes.len() % 3 != 0 {
            return None;
        }
        let mut v = generate(seed);
        for cz in 0..CZ {
            for cx in 0..CX {
                let key = IVec2::new(cx, cz);
                let mut col = v.column(key).cloned().unwrap_or_default();
                let mut differs = false;
                for y in 1..old_h as i32 {
                    for lz in 0..CHUNK {
                        for lx in 0..CHUNK {
                            let (x, z) = (cx * CHUNK + lx, cz * CHUNK + lz);
                            let b = old[((y * WZ + z) * WX + x) as usize];
                            if col.get(lx, y, lz) != b {
                                col.put(lx, y, lz, b);
                                differs = true;
                            }
                        }
                    }
                }
                if differs {
                    col.compact();
                    v.put_whole(key, col);
                }
            }
        }
        v.mark_all_dirty();
        Some(v)
    }
}

// ---------- making the world ----------

/// A box of blocks being made, the world high: the island while it's
/// laid out, or a column and the margin its trees reach into.
pub(crate) struct Area {
    x0: i32,
    z0: i32,
    w: i32,
    d: i32,
    data: Vec<BlockId>,
}

impl Area {
    fn new(x0: i32, z0: i32, w: i32, d: i32) -> Self {
        Area { x0, z0, w, d, data: vec![AIR; (w * d * (WY - MIN_Y)) as usize] }
    }

    #[inline]
    fn idx(&self, x: i32, y: i32, z: i32) -> Option<usize> {
        let (lx, lz, ly) = (x - self.x0, z - self.z0, y - MIN_Y);
        (lx >= 0 && lz >= 0 && ly >= 0 && lx < self.w && lz < self.d && y < WY).then(|| ((ly * self.d + lz) * self.w + lx) as usize)
    }

    #[inline]
    pub(crate) fn get(&self, x: i32, y: i32, z: i32) -> BlockId {
        self.idx(x, y, z).map_or(AIR, |i| self.data[i])
    }

    #[inline]
    pub(crate) fn put(&mut self, x: i32, y: i32, z: i32, id: BlockId) {
        if let Some(i) = self.idx(x, y, z) {
            self.data[i] = id;
        }
    }

    /// The highest block that isn't air, water or leaves.
    pub(crate) fn top(&self, x: i32, z: i32) -> i32 {
        for y in (MIN_Y..WY).rev() {
            let b = self.get(x, y, z);
            if b != AIR && b != WATER && !matches!(b, LEAVES | PINE | BIRCH_LEAVES) && blocks::shape(b) != Shape::Cross {
                return y;
            }
        }
        MIN_Y
    }

    /// One column of it.
    fn column(&self, key: IVec2) -> Column {
        let (x0, z0) = (key.x * CHUNK, key.y * CHUNK);
        let mut flat = vec![AIR; column::BLOCKS];
        for y in MIN_Y..WY {
            for lz in 0..CHUNK {
                let row = ((y - MIN_Y) * CHUNK + lz) * CHUNK;
                for lx in 0..CHUNK {
                    flat[(row + lx) as usize] = self.get(x0 + lx, y, z0 + lz);
                }
            }
        }
        Column::from_flat(&flat)
    }
}

/// What's deep under the ground: stone, slate below about `SLATE_Y`, and
/// bedrock at the very bottom, ragged for a few blocks.
pub(crate) fn deep(seed: u32, x: i32, y: i32, z: i32) -> BlockId {
    let s = (seed % 9973) as i32;
    if y == MIN_Y || (y < MIN_Y + 4 && hash2(x ^ (y * 7919), z, s + 300) < 0.75 - (y - MIN_Y) as f32 * 0.22) {
        return BEDROCK;
    }
    let edge = SLATE_Y + ((hash2(x, z, s + 301) * 4.0) as i32 - 2) + ((fbm(x as f32 / 40.0, z as f32 / 40.0, s + 302, 2) - 0.5) * 12.0) as i32;
    if y < edge { SLATE } else { STONE }
}

/// What a block under the ground is, given what the terrain put there:
/// caves (air) and ores go here (`underground::below`, from the survival
/// work). Pure, and deciding only this one block, so a column comes out
/// the same wherever and whenever it's made.
pub(crate) fn below(_seed: u32, _x: i32, _y: i32, _z: i32, _ground: i32, b: BlockId) -> BlockId {
    b
}

/// Places caves never cut into: the Green, the camp, the farm and the
/// villa and the ground under them, and round where players start (above
/// about y = -8; deeper down anything goes).
pub fn keep_out(x: i32, y: i32, z: i32) -> bool {
    if y < -8 {
        return false;
    }
    let (fx, fz) = (x as f32, z as f32);
    let green = (fx - GREEN.x).hypot((fz - GREEN.z) * 1.1) < GREEN_R + 14.0;
    let start = (fx - START.x).hypot(fz - START.z) < 18.0;
    green || start || places::keep_clear(x, z)
}

/// Where the seed puts the ground at a spot (before plants, places and
/// anything players do).
pub fn ground_height(seed: u32, x: i32, z: i32) -> i32 {
    (wild::height(seed, x as f32, z as f32).floor() as i32).clamp(MIN_Y + 2, WY - 8)
}

/// A column as the seed makes it.
pub fn column(seed: u32, key: IVec2) -> Column {
    if is_island(key) {
        return island(seed).iter().find(|(k, _)| *k == key).map(|(_, c)| c.clone()).unwrap_or_default();
    }
    wild::column(seed, key)
}

/// The island's columns for a seed, made once and shared (every new world,
/// and every test's fresh copy, starts from them).
fn island(seed: u32) -> Arc<Vec<(IVec2, Column)>> {
    static MADE: Mutex<Vec<(u32, Arc<Vec<(IVec2, Column)>>)>> = Mutex::new(Vec::new());
    if let Some((_, cols)) = MADE.lock().unwrap().iter().find(|(s, _)| *s == seed) {
        return cols.clone();
    }
    let area = make_island(seed);
    let cols: Arc<Vec<(IVec2, Column)>> = Arc::new((0..CZ).flat_map(|z| (0..CX).map(move |x| IVec2::new(x, z))).map(|k| (k, area.column(k))).collect());
    let mut made = MADE.lock().unwrap();
    if made.len() >= 4 {
        made.remove(0);
    }
    made.push((seed, cols.clone()));
    cols
}

/// The island, from the seed: the world every player starts in, with the
/// lands past it made as players go.
pub fn generate(seed: u32) -> Voxels {
    let mut w = Voxels::empty(seed);
    w.blank = false;
    for (k, c) in island(seed).iter() {
        w.insert(*k, c.clone());
    }
    w
}

/// The island's ground height at a spot, before anything is built on it.
pub(crate) fn height_at(x: f32, z: f32, s: i32) -> f32 {
    let sea = SEA as f32;
    let mut h = 21.0 + (fbm(x / 70.0, z / 70.0, 1 + s, 5) - 0.5) * 18.0 + (fbm(x / 22.0, z / 22.0, 2 + s, 5) - 0.5) * 4.0;
    // mountains to the north
    let north = smooth(168.0, 68.0, z);
    let ridge = 1.0 - (fbm(x / 45.0, z / 45.0, 5 + s, 5) * 2.0 - 1.0).abs();
    h += north * (18.0 + ridge * 48.0 + fbm(x / 15.0, z / 15.0, 6 + s, 5) * 8.0);
    // a hill to the south, where players start
    let dh = ((x - HILL.x) / 1.4).hypot(z - HILL.z);
    h += 14.0 * smooth(46.0, 6.0, dh);
    // the lake
    let (lx, lz, lr) = LAKE;
    let dl = (x - lx).hypot((z - lz) * 1.3) + (fbm(x / 12.0, z / 12.0, 9 + s, 5) - 0.5) * 14.0;
    let lake = smooth(lr - 12.0, lr + 10.0, dl);
    h = h * lake + (sea - 4.0) * (1.0 - lake);
    // a creek from the lake towards the Green's east side
    let creek_z = 198.0 + (x / 18.0).sin() * 8.0;
    if x > 198.0 && x < 244.0 {
        let c = smooth(1.5, 5.0, (z - creek_z).abs());
        h = h.min(h * c + (sea - 2.0) * (1.0 - c));
    }
    // the Green is flat
    let dg = (x - GREEN.x).hypot((z - GREEN.z) * 1.1);
    let g = smooth(GREEN_R + 12.0, GREEN_R, dg);
    h = h * (1.0 - g) + GREEN_Y as f32 * g;
    // an island: the land sinks into the sea near the edges
    let edge = x.min(z).min(WX as f32 - 1.0 - x).min(WZ as f32 - 1.0 - z)
        + (fbm(x / 20.0, z / 20.0, 11 + s, 3) - 0.5) * 30.0;
    let e = smooth(6.0, 44.0, edge);
    h * e + (sea - 7.0) * (1.0 - e)
}

fn make_island(seed: u32) -> Area {
    let mut w = Area::new(0, 0, WX, WZ);
    let s = (seed % 1000) as i32 * 31;
    let mut hs = vec![0i32; (WX * WZ) as usize];
    for z in 0..WZ {
        for x in 0..WX {
            hs[(z * WX + x) as usize] = height_at(x as f32, z as f32, s).floor() as i32;
        }
    }
    let hat = |x: i32, z: i32| hs[(z.clamp(0, WZ - 1) * WX + x.clamp(0, WX - 1)) as usize];
    for z in 0..WZ {
        for x in 0..WX {
            let h = hat(x, z).clamp(1, WY - 8);
            let slope = [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().map(|(dx, dz)| (hat(x + dx, z + dz) - h).abs()).max().unwrap();
            for y in MIN_Y..=h {
                let mut b = STONE;
                if y > h - 4 {
                    b = DIRT;
                }
                if y == h {
                    b = if h <= SEA + 1 {
                        SAND
                    } else if h > 74 {
                        SNOW
                    } else if h > 62 || slope >= 3 {
                        STONE
                    } else {
                        GRASS
                    };
                }
                if h > 62 && y > h - 3 {
                    b = if y == h && h > 74 { SNOW } else { STONE };
                }
                if y < 0 {
                    b = deep(seed, x, y, z);
                }
                w.put(x, y, z, below(seed, x, y, z, h, b));
            }
            for y in h + 1..=SEA {
                w.put(x, y, z, WATER);
            }
        }
    }
    let mut r = Rng::new(seed ^ 0x9e37_79b9);
    plant(&mut w, &hs, &mut r, s);
    places::build(&mut w, &hs, &mut r);
    build_green(&mut w);
    w
}

fn blob(w: &mut Area, c: IVec3, rx: i32, ry: i32, rz: i32, id: BlockId, r: &mut Rng, rough: f32) {
    for y in -ry..=ry {
        for z in -rz..=rz {
            for x in -rx..=rx {
                let d = (x * x) as f32 / (rx * rx) as f32 + (y * y) as f32 / (ry * ry) as f32 + (z * z) as f32 / (rz * rz) as f32;
                if d <= 1.0 - r.next() * rough && w.get(c.x + x, c.y + y, c.z + z) == AIR {
                    w.put(c.x + x, c.y + y, c.z + z, id);
                }
            }
        }
    }
}

/// Puts wood where there's air or leaves (branches push through crowns).
fn wood(w: &mut Area, p: IVec3, id: BlockId) {
    if matches!(w.get(p.x, p.y, p.z), AIR | LEAVES | BIRCH_LEAVES | TALLGRASS) {
        w.put(p.x, p.y, p.z, id);
    }
}

/// An oak: a trunk with branches stepping out from its top, each carrying a
/// crown of leaves, so the canopy is several lumps rather than one ball. Big
/// ones flare at the foot and spread wide.
fn oak(w: &mut Area, x: i32, y0: i32, z: i32, r: &mut Rng, big: bool) {
    let h = if big { 7 } else { 5 } + (r.next() * 3.0) as i32;
    for y in 0..h {
        w.put(x, y0 + y, z, LOG);
    }
    if big {
        for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            if r.next() < 0.7 {
                wood(w, IVec3::new(x + dx, y0, z + dz), LOG);
            }
        }
    }
    let branches = if big { 3 + r.range(0, 1) } else { 2 + r.range(0, 1) };
    let turn = r.next() * std::f32::consts::TAU;
    let mut crowns = vec![(IVec3::new(x, y0 + h, z), if big { 4 } else { 3 }, if big { 3 } else { 2 })];
    for i in 0..branches {
        let ang = turn + (i as f32 + r.next() * 0.5) * std::f32::consts::TAU / branches as f32;
        let dir = Vec3::new(ang.cos(), if big { 0.65 } else { 0.8 }, ang.sin());
        let mut p = Vec3::new(x as f32 + 0.5, (y0 + h - 2 - r.range(0, 1) - big as i32) as f32 + 0.5, z as f32 + 0.5);
        for _ in 0..if big { 3 + r.range(0, 1) } else { 2 } {
            p += dir;
            wood(w, p.floor().as_ivec3(), LOG);
        }
        crowns.push((p.floor().as_ivec3(), if big { 3 } else { 2 }, 2));
    }
    for (c, a, b) in crowns {
        crown(w, c, a, b, LEAVES, r);
    }
}

/// A crown of leaves as a few overlapping lumps round `c`, so it reads as
/// rounded clumps rather than one ball, with a leaf or two hanging under it.
fn crown(w: &mut Area, c: IVec3, a: i32, b: i32, id: BlockId, r: &mut Rng) {
    blob(w, c, a, b, a, id, r, 0.2);
    let lumps = 2 + r.range(0, 2);
    for _ in 0..lumps {
        let ang = r.next() * std::f32::consts::TAU;
        let out = a as f32 * (0.5 + 0.3 * r.next());
        let at = c + IVec3::new((ang.cos() * out).round() as i32, r.range(-1, 1), (ang.sin() * out).round() as i32);
        let ra = (a - 1 - r.range(0, 1)).max(1);
        blob(w, at, ra, (b - 1).max(1), ra, id, r, 0.2);
    }
    // under the rim: short hanging bits so the bottom isn't flat
    for _ in 0..a * 2 {
        let ang = r.next() * std::f32::consts::TAU;
        let out = a as f32 * (0.4 + 0.5 * r.next());
        let (hx, hz) = (c.x + (ang.cos() * out).round() as i32, c.z + (ang.sin() * out).round() as i32);
        if w.get(hx, c.y, hz) != id {
            continue;
        }
        let mut y = c.y;
        while y > c.y - b - 2 && w.get(hx, y, hz) == id {
            y -= 1;
        }
        for d in 0..1 + r.range(0, 1) {
            if w.get(hx, y - d, hz) == AIR {
                w.put(hx, y - d, hz, id);
            }
        }
    }
}

/// A birch: tall and slim, with a narrow crown in two lumps.
fn birch(w: &mut Area, x: i32, y0: i32, z: i32, r: &mut Rng) {
    let h = 7 + (r.next() * 4.0) as i32;
    for y in 0..h {
        w.put(x, y0 + y, z, BIRCH);
    }
    crown(w, IVec3::new(x, y0 + h - 1, z), 2, 3, BIRCH_LEAVES, r);
    let side = IVec3::new(r.range(-1, 1), y0 + h - 4, r.range(-1, 1));
    wood(w, IVec3::new(x + side.x, side.y, z + side.z), BIRCH);
    blob(w, IVec3::new(x + side.x * 2, side.y, z + side.z * 2), 2, 2, 2, BIRCH_LEAVES, r, 0.3);
}

/// A shrub: a knee-high lump of leaves.
fn shrub(w: &mut Area, x: i32, y: i32, z: i32, r: &mut Rng) {
    let (a, b) = if r.next() < 0.5 { (1, 1) } else { (2, 1) };
    blob(w, IVec3::new(x, y, z), a, b, a, LEAVES, r, 0.45);
}

fn pine(w: &mut Area, x: i32, y0: i32, z: i32, r: &mut Rng) {
    let h = 8 + (r.next() * 5.0) as i32;
    for y in 0..h {
        w.put(x, y0 + y, z, LOG);
    }
    for y in 2..=h + 1 {
        let rr = (((h + 1 - y) as f32 / 3.2 + if y % 2 == 1 { 0.6 } else { 0.0 }).round() as i32).max(0);
        for dz in -rr..=rr {
            for dx in -rr..=rr {
                if (dx * dx + dz * dz) as f32 <= (rr * rr) as f32 + 0.5 && w.get(x + dx, y0 + y, z + dz) == AIR {
                    w.put(x + dx, y0 + y, z + dz, PINE);
                }
            }
        }
    }
    w.put(x, y0 + h + 1, z, PINE);
}

fn plant(w: &mut Area, hs: &[i32], r: &mut Rng, s: i32) {
    let clear = |x: i32, z: i32| {
        let (x, z) = (x as f32, z as f32);
        let dg = (x - GREEN.x).hypot((z - GREEN.z) * 1.1);
        let dc = (x - START.x).hypot(z - START.z);
        // keep the view from the start to the Green open
        let along = (START.z - z) / (START.z - GREEN.z);
        let off = (x - (START.x + (GREEN.x - START.x) * along)).abs();
        let corridor = along > 0.0 && along < 1.0 && off < 9.0 + along * 14.0;
        dg < GREEN_R + 4.0 || dc < 12.0 || corridor || places::keep_clear(x as i32, z as i32)
    };
    let h_at = |x: i32, z: i32| hs[(z * WX + x) as usize];
    for z in 4..WZ - 4 {
        for x in 4..WX - 4 {
            let h = h_at(x, z);
            if w.get(x, h, z) != GRASS || clear(x, z) {
                continue;
            }
            let forest = fbm(x as f32 / 30.0, z as f32 / 30.0, 12 + s, 5);
            // glades: small clearings where light gets down to the floor
            let glade = fbm(x as f32 / 13.0, z as f32 / 13.0, 17 + s, 3) > 0.6;
            let v = r.next();
            if h > 34 && h < 62 && !glade && v < 0.01 + forest * 0.035 {
                let y0 = w.top(x, z) + 1;
                pine(w, x, y0, z, r);
            } else if h <= 34 && forest > 0.55 && !glade && v < 0.008 + 0.024 * smooth(0.55, 0.8, forest) {
                let k = r.next();
                if k < 0.3 {
                    let y0 = w.top(x, z) + 1;
                    birch(w, x, y0, z, r)
                } else {
                    let y0 = w.top(x, z) + 1;
                    oak(w, x, y0, z, r, k > 0.85)
                }
            } else if h <= 34 && forest > 0.45 && v < 0.05 {
                let y0 = w.top(x, z) + 1;
                shrub(w, x, y0, z, r);
            } else if h <= 34 && v < 0.0035 {
                let big = r.next() < 0.3;
                let y0 = w.top(x, z) + 1;
                oak(w, x, y0, z, r, big);
            }
        }
    }
    // trees that frame the view from the start
    let (cx, cz) = (START.x as i32, START.z as i32);
    let y0 = w.top(cx - 17, cz - 7) + 1;
    oak(w, cx - 17, y0, cz - 7, r, true);
    let y0 = w.top(cx + 16, cz - 9) + 1;
    birch(w, cx + 16, y0, cz - 9, r);
    let y0 = w.top(cx + 19, cz - 5) + 1;
    birch(w, cx + 19, y0, cz - 5, r);
    let y0 = w.top(cx + 22, cz - 12) + 1;
    birch(w, cx + 22, y0, cz - 12, r);
    let y0 = w.top(cx - 27, cz - 20) + 1;
    oak(w, cx - 27, y0, cz - 20, r, false);
    // grass, flowers in patches of one colour, and ferns and bushes under
    // the trees (the short grass over everything is drawn, not blocks: see
    // `mesh::ground_cover`)
    let flowers = [FLOWER_RED, FLOWER_BLUE, FLOWER_WHITE, FLOWER_VIOLET];
    for z in 4..WZ - 4 {
        for x in 4..WX - 4 {
            let h = h_at(x, z);
            if w.get(x, h, z) != GRASS || w.get(x, h + 1, z) != AIR {
                continue;
            }
            let (fx, fz) = (x as f32, z as f32);
            let near = (fx - HILL.x).hypot(fz - HILL.z) < 70.0;
            let meadow = fbm(fx / 9.0, fz / 9.0, 14 + s, 5);
            let patch = fbm(fx / 7.0, fz / 7.0, 15 + s, 3);
            let forest = fbm(fx / 30.0, fz / 30.0, 12 + s, 5);
            let shade = (1..12).any(|d| matches!(w.get(x, h + d, z), LEAVES | BIRCH_LEAVES | PINE));
            let v = r.next();
            let id = if (shade || forest > 0.55) && v < 0.16 {
                if r.next() < 0.22 { BUSH } else { FERN }
            } else if patch > 0.64 && v < 0.5 {
                flowers[(fbm(fx / 23.0, fz / 23.0, 16 + s, 2) * 7.99) as usize % 4]
            } else if r.next() < if near { 0.2 } else { 0.08 } * (0.4 + meadow) {
                TALLGRASS
            } else {
                continue;
            };
            w.put(x, h + 1, z, id);
        }
    }
}

/// The Green: the village square where creations are shown.
fn build_green(w: &mut Area) {
    let (gx, gz, gy) = (GREEN.x as i32, GREEN.z as i32, GREEN_Y);
    // clear plants on the Green itself
    for z in gz - 30..=gz + 30 {
        for x in gx - 34..=gx + 34 {
            for y in gy + 1..gy + 3 {
                if blocks::shape(w.get(x, y, z)) == Shape::Cross {
                    w.put(x, y, z, AIR);
                }
            }
        }
    }
    // gravel ring path and spokes
    for z in gz - 26..=gz + 26 {
        for x in gx - 30..=gx + 30 {
            let d = ((x - gx) as f32).hypot((z - gz) as f32 * 1.1);
            let ring = (d - 10.0).abs() < 1.1;
            let spoke = ((x - gx).abs() < 1 && d > 10.0 && d < 23.0) || ((z - gz).abs() < 1 && d > 10.0 && d < 23.0 && x < gx);
            let plaza = z < PORTAL_Z + 4 && z > PORTAL_Z && (x - gx).abs() < 20;
            if (ring || spoke || plaza) && w.get(x, gy, z) == GRASS {
                w.put(x, gy, z, GRAVEL);
                if blocks::shape(w.get(x, gy + 1, z)) == Shape::Cross {
                    w.put(x, gy + 1, z, AIR);
                }
            }
        }
    }
    // a well in the middle
    for z in -2i32..=2 {
        for x in -2i32..=2 {
            if x.abs() == 2 || z.abs() == 2 {
                w.put(gx + x, gy + 1, gz + z, COBBLE);
            } else {
                w.put(gx + x, gy, gz + z, WATER);
            }
        }
    }
    for (x, z) in [(-2, -2), (2, -2), (-2, 2), (2, 2)] {
        w.put(gx + x, gy + 2, gz + z, LOG);
        w.put(gx + x, gy + 3, gz + z, LOG);
    }
    for z in -2..=2 {
        for x in -2..=2 {
            w.put(gx + x, gy + 4, gz + z, if (x + z).rem_euclid(2) == 1 { WOOL_RED } else { WOOL_CREAM });
        }
    }
    // four arches facing south, where creations are shown
    for px in portal_xs() {
        for y in 0..=6 {
            for x in -3i32..=3 {
                let inner = x.abs() <= 1 && (1..=4).contains(&y);
                let corner = x.abs() == 3 && y == 6;
                w.put(px + x, gy + 1 + y, PORTAL_Z, if inner || corner { AIR } else { MOSSY });
            }
        }
        w.put(px, gy + 6, PORTAL_Z, LANTERN);
        for x in -3..=3 {
            w.put(px + x, gy, PORTAL_Z + 1, COBBLE);
        }
    }
    // a market stall to the west
    let (sx, sz) = (gx - 15, gz + 3);
    for (x, z) in [(-3, -2), (3, -2), (-3, 2), (3, 2)] {
        for y in 1..=4 {
            w.put(sx + x, gy + y, sz + z, LOG);
        }
    }
    for z in -3i32..=3 {
        for x in -4i32..=4 {
            let y = gy + 5 + if z.abs() <= 1 { 1 } else { 0 };
            w.put(sx + x, y, sz + z, if (x + 8) % 2 == 1 { WOOL_RED } else { WOOL_CREAM });
        }
    }
    for x in -2..=2 {
        w.put(sx + x, gy + 1, sz + 2, PLANKS);
        w.put(sx + x, gy + 2, sz + 2, PLANKS);
    }
    w.put(sx - 1, gy + 3, sz + 2, HAY);
    w.put(sx + 1, gy + 3, sz + 2, WOOL_BLUE);
    w.put(sx, gy + 3, sz + 2, LANTERN);
    // a ring of hay bales to the east, the mossbacks' paddock
    let (rx, rz) = (gx + 14, gz + 4);
    for z in -6..=6 {
        for x in -6..=6 {
            if x * x + z * z < 40 && w.get(rx + x, gy, rz + z) == GRASS {
                w.put(rx + x, gy, rz + z, SAND);
                w.put(rx + x, gy + 1, rz + z, AIR);
            }
        }
    }
    for a in (0..360).step_by(30) {
        let t = (a as f32).to_radians();
        let x = (rx as f32 + t.cos() * 7.5).round() as i32;
        let z = (rz as f32 + t.sin() * 7.5).round() as i32;
        w.put(x, gy + 1, z, HAY);
        if a % 60 == 0 {
            w.put(x, gy + 2, z, HAY);
        }
    }
    // lantern posts round the ring path
    for a in (20..360).step_by(45) {
        let t = (a as f32).to_radians();
        let x = (gx as f32 + t.cos() * 12.0).round() as i32;
        let z = (gz as f32 + t.sin() * 11.0).round() as i32;
        if ((x - rx) as f32).hypot((z - rz) as f32) < 9.0 || ((x - sx) as f32).hypot((z - sz) as f32) < 6.0 {
            continue;
        }
        for y in 1..=3 {
            w.put(x, gy + y, z, LOG);
        }
        w.put(x, gy + 4, z, LANTERN);
    }
    // someone's balloon over the Green
    let (bx, by, bz, br) = (gx - 27, 47, gz - 40, 7);
    for y in -br..=br + 2 {
        for z in -br..=br {
            for x in -br..=br {
                let yy = if y > 0 { y as f32 / 1.25 } else { y as f32 };
                let d = (x as f32).hypot(yy).hypot(z as f32);
                let r = if y < 0 { br as f32 * (1.0 - (-y as f32 / br as f32) * 0.45) } else { br as f32 };
                if d <= r && d > r - 1.4 {
                    let ang = (z as f32).atan2(x as f32);
                    let band = ((ang + std::f32::consts::PI) / (std::f32::consts::PI / 4.0)) as i32 % 2;
                    w.put(bx + x, by + y, bz + z, if band == 1 { WOOL_BLUE } else { WOOL_YELLOW });
                }
            }
        }
    }
    for z in -1..=1 {
        for x in -1..=1 {
            w.put(bx + x, by - br - 6, bz + z, PLANKS);
            if x.abs() + z.abs() == 2 {
                w.put(bx + x, by - br - 5, bz + z, PLANKS);
            }
        }
    }
}

/// The x of each arch on the Green.
pub fn portal_xs() -> [i32; 4] {
    let gx = GREEN.x as i32;
    [gx - 15, gx - 5, gx + 5, gx + 15]
}

/// A bend in the creek north-east of the Green, where water creatures live
/// (the creek is carved in `height`, along z = 198 + sin(x / 18) * 8).
pub fn creek() -> Vec3 {
    let x = 214.0;
    Vec3::new(x, SEA as f32 + 1.0, 198.0 + (x / 18.0f32).sin() * 8.0)
}

/// The paddock east of the well, where mossbacks start.
pub fn paddock() -> Vec3 {
    Vec3::new(GREEN.x + 14.5, GREEN_Y as f32 + 1.0, GREEN.z + 4.5)
}
