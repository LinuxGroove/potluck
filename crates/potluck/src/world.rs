//! The world: a finite island of blocks, made from a seed, and the changes
//! players and creations make to it.

use std::collections::HashSet;

use bevy::prelude::*;

use crate::blocks::{self, *};
use crate::noise::{fbm, smooth, Rng};

mod places;

pub const WX: i32 = 384;
pub const WY: i32 = 128;
pub const WZ: i32 = 384;
pub const SEA: i32 = 19;
/// Chunks are cubes of this many blocks, meshed one at a time.
pub const CHUNK: i32 = 32;
pub const CX: i32 = WX / CHUNK;
pub const CY: i32 = WY / CHUNK;
pub const CZ: i32 = WZ / CHUNK;

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

#[derive(Resource)]
pub struct Voxels {
    pub seed: u32,
    data: Vec<BlockId>,
    /// Chunks whose mesh is out of date.
    pub dirty: HashSet<IVec3>,
    /// Chunks whose sky a change may have opened or closed (`mesh::Sky`),
    /// remeshed a few at a time once the dirty ones are done.
    pub shade: HashSet<IVec3>,
    /// Block changes since the last network send: (position, block).
    pub changes: Vec<(IVec3, BlockId)>,
    /// Lantern positions, for the lights near the player.
    pub lanterns: HashSet<IVec3>,
}

impl Voxels {
    pub fn empty(seed: u32) -> Self {
        Self {
            seed,
            data: vec![AIR; (WX * WY * WZ) as usize],
            dirty: HashSet::new(),
            shade: HashSet::new(),
            changes: Vec::new(),
            lanterns: HashSet::new(),
        }
    }

    #[inline]
    fn idx(x: i32, y: i32, z: i32) -> usize {
        ((y * WZ + z) * WX + x) as usize
    }

    #[inline]
    pub fn inside(x: i32, y: i32, z: i32) -> bool {
        x >= 0 && y >= 0 && z >= 0 && x < WX && y < WY && z < WZ
    }

    #[inline]
    pub fn get(&self, x: i32, y: i32, z: i32) -> BlockId {
        if Self::inside(x, y, z) { self.data[Self::idx(x, y, z)] } else { AIR }
    }

    pub fn at(&self, p: IVec3) -> BlockId {
        self.get(p.x, p.y, p.z)
    }

    /// Sets a block while generating: no change is recorded.
    fn put(&mut self, x: i32, y: i32, z: i32, id: BlockId) {
        if Self::inside(x, y, z) {
            self.data[Self::idx(x, y, z)] = id;
        }
    }

    /// Changes a block in play: remeshes what it touches and records the change.
    pub fn set(&mut self, p: IVec3, id: BlockId) -> bool {
        if !Self::inside(p.x, p.y, p.z) || self.at(p) == id {
            return false;
        }
        if p.y == 0 && id != BEDROCK {
            return false;
        }
        self.apply(p, id);
        self.changes.push((p, id));
        true
    }

    /// Changes a block that arrived from the network (not sent on again).
    pub fn apply(&mut self, p: IVec3, id: BlockId) {
        if !Self::inside(p.x, p.y, p.z) {
            return;
        }
        let old = self.at(p);
        self.data[Self::idx(p.x, p.y, p.z)] = id;
        if blocks::block(old).light {
            self.lanterns.remove(&p);
        }
        if blocks::block(id).light {
            self.lanterns.insert(p);
        }
        // the chunk and any neighbour the block touches (for faces and AO)
        let chunks = |d: IVec3, u: IVec3| {
            let lo = (p - d).div_euclid(IVec3::splat(CHUNK)).max(IVec3::ZERO);
            let hi = (p + u).div_euclid(IVec3::splat(CHUNK)).min(IVec3::new(CX - 1, CY - 1, CZ - 1));
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

    /// The highest block that isn't air, water or leaves.
    pub fn top(&self, x: i32, z: i32) -> i32 {
        for y in (0..WY).rev() {
            let b = self.get(x, y, z);
            if b != AIR && b != WATER && !matches!(b, LEAVES | PINE | BIRCH_LEAVES) && blocks::shape(b) != Shape::Cross {
                return y;
            }
        }
        0
    }

    pub fn mark_all_dirty(&mut self) {
        for z in 0..CZ {
            for y in 0..CY {
                for x in 0..CX {
                    self.dirty.insert(IVec3::new(x, y, z));
                }
            }
        }
    }

    fn find_lanterns(&mut self) {
        self.lanterns.clear();
        for (i, &b) in self.data.iter().enumerate() {
            if blocks::block(b).light {
                let i = i as i32;
                self.lanterns.insert(IVec3::new(i % WX, i / (WX * WZ), (i / WX) % WZ));
            }
        }
    }

    /// The whole world, run-length encoded (for sending to friends who join).
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(1 << 20);
        let mut i = 0;
        while i < self.data.len() {
            let b = self.data[i];
            let mut n = 1usize;
            while i + n < self.data.len() && self.data[i + n] == b && n < 65535 {
                n += 1;
            }
            out.push(b);
            out.extend_from_slice(&(n as u16).to_le_bytes());
            i += n;
        }
        out
    }

    pub fn decode(seed: u32, bytes: &[u8]) -> Option<Self> {
        let mut v = Self::empty(seed);
        let mut i = 0usize;
        for run in bytes.chunks_exact(3) {
            let n = u16::from_le_bytes([run[1], run[2]]) as usize;
            if i + n > v.data.len() {
                return None;
            }
            v.data[i..i + n].fill(run[0]);
            i += n;
        }
        if i != v.data.len() {
            return None;
        }
        v.find_lanterns();
        v.mark_all_dirty();
        Some(v)
    }
}

// ---------- terrain ----------

fn height_at(x: f32, z: f32, s: i32) -> f32 {
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

pub fn generate(seed: u32) -> Voxels {
    let mut w = Voxels::empty(seed);
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
            for y in 0..=h {
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
                if y == 0 {
                    b = BEDROCK;
                }
                w.put(x, y, z, b);
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
    w.find_lanterns();
    w.mark_all_dirty();
    w
}

fn blob(w: &mut Voxels, c: IVec3, rx: i32, ry: i32, rz: i32, id: BlockId, r: &mut Rng, rough: f32) {
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
fn wood(w: &mut Voxels, p: IVec3, id: BlockId) {
    if matches!(w.get(p.x, p.y, p.z), AIR | LEAVES | BIRCH_LEAVES | TALLGRASS) {
        w.put(p.x, p.y, p.z, id);
    }
}

/// An oak: a trunk with branches stepping out from its top, each carrying a
/// crown of leaves, so the canopy is several lumps rather than one ball. Big
/// ones flare at the foot and spread wide.
fn oak(w: &mut Voxels, x: i32, z: i32, r: &mut Rng, big: bool) {
    let y0 = w.top(x, z) + 1;
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
fn crown(w: &mut Voxels, c: IVec3, a: i32, b: i32, id: BlockId, r: &mut Rng) {
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
fn birch(w: &mut Voxels, x: i32, z: i32, r: &mut Rng) {
    let y0 = w.top(x, z) + 1;
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
fn shrub(w: &mut Voxels, x: i32, z: i32, r: &mut Rng) {
    let y = w.top(x, z) + 1;
    let (a, b) = if r.next() < 0.5 { (1, 1) } else { (2, 1) };
    blob(w, IVec3::new(x, y, z), a, b, a, LEAVES, r, 0.45);
}

fn pine(w: &mut Voxels, x: i32, z: i32, r: &mut Rng) {
    let y0 = w.top(x, z) + 1;
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

fn plant(w: &mut Voxels, hs: &[i32], r: &mut Rng, s: i32) {
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
                pine(w, x, z, r);
            } else if h <= 34 && forest > 0.55 && !glade && v < 0.008 + 0.024 * smooth(0.55, 0.8, forest) {
                let k = r.next();
                if k < 0.3 {
                    birch(w, x, z, r)
                } else {
                    oak(w, x, z, r, k > 0.85)
                }
            } else if h <= 34 && forest > 0.45 && v < 0.05 {
                shrub(w, x, z, r);
            } else if h <= 34 && v < 0.0035 {
                let big = r.next() < 0.3;
                oak(w, x, z, r, big);
            }
        }
    }
    // trees that frame the view from the start
    let (cx, cz) = (START.x as i32, START.z as i32);
    oak(w, cx - 17, cz - 7, r, true);
    birch(w, cx + 16, cz - 9, r);
    birch(w, cx + 19, cz - 5, r);
    birch(w, cx + 22, cz - 12, r);
    oak(w, cx - 27, cz - 20, r, false);
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
fn build_green(w: &mut Voxels) {
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
