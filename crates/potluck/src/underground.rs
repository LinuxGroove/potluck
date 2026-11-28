//! What's underground: caves, ore veins, crystal hollows and old workings
//! with chests in them. All of it is decided one block at a time from the
//! seed and where the block is (`carve`), so a column of the world comes
//! out the same on every machine, in any order and alone; nothing here
//! looks at the world.
//!
//! - **Caves** are tunnels where two noises are both near zero, which
//!   makes long winding tubes, and, deep down, caverns where a third is
//!   high. Near the surface they stop a few blocks under the ground, except
//!   where a cave mouth lets one come out, never by the sea.
//! - **Ores** come in veins, at most one of each ore in each cell of a
//!   grid, at the depths each belongs to: coal near the top, moonsilver
//!   only at the bottom.
//! - **Crystal hollows** are round rooms lined with crystal ore, deep down.
//! - **Old workings** are straight corridors with posts and beams, one
//!   long one and a few off it in some stretches of the world, with old
//!   chests at their ends (the mining creation opens them).
//! - **Glowcaps** grow on some cave floors.
//!
//! Heights are the world's: the sea is at 19 and the deepest ground at
//! -128.

use crate::blocks::{self, BlockId, AIR, DIRT, GRASS, GRAVEL, LOG, PLANKS, SNOW, STONE};

/// The darker stone of the deep, below about y = -40 (the looks' block).
const SLATE: BlockId = 47;
/// The sea's top, below which caves don't open.
const SEA: i32 = 19;
/// Nothing is carved this deep or deeper, so there's always a floor.
const FLOOR: i32 = -120;

/// A number for a point and a seed, well mixed.
fn h(seed: u32, x: i32, y: i32, z: i32, k: u32) -> u32 {
    let mut v = seed ^ k.wrapping_mul(0x9e37_79b9);
    for c in [x, y, z] {
        v ^= (c as u32).wrapping_mul(0x85eb_ca6b);
        v = v.rotate_left(13).wrapping_mul(0xc2b2_ae35);
    }
    v ^= v >> 16;
    v = v.wrapping_mul(0x7feb_352d);
    v ^= v >> 15;
    v = v.wrapping_mul(0x846c_a68b);
    v ^ (v >> 16)
}

/// `h` as a number in `0.0..1.0`.
fn unit(seed: u32, x: i32, y: i32, z: i32, k: u32) -> f32 {
    (h(seed, x, y, z, k) >> 8) as f32 / 16_777_216.0
}

fn ease(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

/// How far apart each noise's cells are across the ground (x and z) and
/// up (y), by its number. The ground noises (1 and 2) don't change with
/// height.
const SCALE: [(f32, f32); 7] = [(1.0, 1.0), (48.0, 0.0), (90.0, 0.0), (30.0, 20.0), (30.0, 20.0), (56.0, 26.0), (18.0, 12.0)];

/// The lowest cell level kept for a column (y / 12 at the bottom of the
/// world), and how many levels.
const LEVEL0: i32 = -12;
const LEVELS: usize = 24;

/// One column's noise, level by level: at each level of cells, each
/// noise's corners blended across the ground for this column, worked out
/// the first time a block of the column needs them. Columns are worked out
/// a block at a time, bottom to top, so this leaves only a blend up and
/// down for each block. It only remembers, so answers are the same with or
/// without it.
struct Column {
    seed: u32,
    x: i32,
    z: i32,
    level: [[f32; LEVELS]; 7],
}

thread_local! {
    static COLUMN: std::cell::RefCell<Column> = const { std::cell::RefCell::new(Column { seed: u32::MAX, x: i32::MIN, z: 0, level: [[f32::NAN; LEVELS]; 7] }) };
}

/// Noise `k` at level `iy` of cells, blended across the ground at (x, z).
fn level(seed: u32, k: u32, x: i32, z: i32, iy: i32) -> f32 {
    let (across, _) = SCALE[k as usize];
    let (fx, fz) = (x as f32 / across, z as f32 / across);
    let (ix, iz) = (fx.floor() as i32, fz.floor() as i32);
    let (tx, tz) = (ease(fx - ix as f32), ease(fz - iz as f32));
    let c = |dx: i32, dz: i32| unit(seed, ix + dx, iy, iz + dz, k) * 2.0 - 1.0;
    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
    lerp(lerp(c(0, 0), c(1, 0), tx), lerp(c(0, 1), c(1, 1), tx), tz)
}

/// Smooth noise `k` (1 to 6) in -1..1 at a block.
fn noise(seed: u32, k: u32, x: i32, y: i32, z: i32) -> f32 {
    let (_, up) = SCALE[k as usize];
    // the ground noises sit halfway between their first two levels
    let fy = if up == 0.0 { 0.5 } else { y as f32 / up };
    let iy = fy.floor() as i32;
    let t = ease(fy - iy as f32);
    let at = (iy - LEVEL0) as usize;
    let (a, b) = if at + 1 < LEVELS {
        COLUMN.with_borrow_mut(|c| {
            if (c.seed, c.x, c.z) != (seed, x, z) {
                *c = Column { seed, x, z, level: [[f32::NAN; LEVELS]; 7] };
            }
            let l = &mut c.level[k as usize];
            if l[at].is_nan() {
                l[at] = level(seed, k, x, z, iy);
            }
            if l[at + 1].is_nan() {
                l[at + 1] = level(seed, k, x, z, iy + 1);
            }
            (l[at], l[at + 1])
        })
    } else {
        (level(seed, k, x, z, iy), level(seed, k, x, z, iy + 1))
    };
    a + (b - a) * t
}

/// What the ground is made of that caves and workings cut through; at
/// the very top, a cave mouth cuts through grass and snow too.
fn ground_stuff(b: BlockId, top: bool) -> bool {
    matches!(b, STONE | SLATE | DIRT | GRAVEL) || (top && matches!(b, GRASS | SNOW))
}

/// A tunnel or a cavern at this block: air.
pub fn cave(seed: u32, x: i32, y: i32, z: i32, ground: i32) -> bool {
    if y <= FLOOR {
        return false;
    }
    let under = ground - y;
    // near the surface only where a mouth lets one out, and never by water
    if under < 7 {
        if ground <= SEA + 6 || noise(seed, 1, x, 0, z) < 0.45 {
            return false;
        }
    }
    // tunnels: where two noises are both close to zero, the second worked
    // out only where the first is
    let a = noise(seed, 3, x, y, z);
    if a.abs() < 0.135 {
        let wide = 0.085 + 0.05 * noise(seed, 2, x, 0, z);
        if a.abs() < wide {
            let b = noise(seed, 4, x, y, z);
            if a * a + b * b < wide * wide {
                return true;
            }
        }
    }
    // caverns, deep down: big rooms, flatter than they're wide
    if y < 0 && y > FLOOR + 6 {
        // they thin out toward the bottom and toward the top of the deep
        let room = 0.62 + 0.004 * (y + 50).abs() as f32;
        // the second noise only where it could tip the first over
        let big = noise(seed, 5, x, y, z);
        if big + 0.35 > room && big + 0.35 * noise(seed, 6, x, y, z) > room {
            return true;
        }
    }
    false
}

/// One kind of ore: the block, the depths it's found at, its grid's cell
/// size, how often a cell has a vein, and how big veins are.
struct Vein {
    ore: BlockId,
    from: i32,
    to: i32,
    cell: i32,
    chance: f32,
    least: f32,
    most: f32,
}

const VEINS: [Vein; 6] = [
    Vein { ore: blocks::COAL_ORE, from: -16, to: 96, cell: 12, chance: 0.6, least: 1.8, most: 3.0 },
    Vein { ore: blocks::COPPER_ORE, from: -48, to: 64, cell: 13, chance: 0.5, least: 1.6, most: 2.6 },
    Vein { ore: blocks::IRON_ORE, from: -80, to: 32, cell: 13, chance: 0.5, least: 1.4, most: 2.3 },
    Vein { ore: blocks::GOLD_ORE, from: -112, to: -24, cell: 15, chance: 0.4, least: 1.2, most: 1.9 },
    Vein { ore: blocks::CRYSTAL_ORE, from: -120, to: -56, cell: 17, chance: 0.35, least: 1.0, most: 1.6 },
    Vein { ore: blocks::MOONSILVER_ORE, from: -124, to: -88, cell: 21, chance: 0.3, least: 0.9, most: 1.4 },
];

/// A vein's middle and size in one cell, or nothing there.
type VeinAt = Option<(i32, i32, i32, f32)>;

thread_local! {
    /// Each ore's vein in the last cell asked about: (seed, cell, vein),
    /// for the same reason as `Column`.
    static VEIN_CELLS: std::cell::RefCell<[(u32, i32, i32, i32, VeinAt); 6]> = const { std::cell::RefCell::new([(u32::MAX, i32::MIN, 0, 0, None); 6]) };
}

/// The vein of ore `k` in a cell, if it has one.
fn vein(seed: u32, k: usize, cx: i32, cy: i32, cz: i32) -> VeinAt {
    let v = &VEINS[k];
    let n = 10 + k as u32;
    if unit(seed, cx, cy, cz, n) >= v.chance {
        return None;
    }
    // the middle, far enough in that it stays in its cell
    let margin = v.most.ceil() as i32 + 1;
    let span = (v.cell - 2 * margin).max(1) as f32;
    let at = |c: i32, m: u32| c * v.cell + margin + (unit(seed, cx, cy, cz, n + m) * span) as i32;
    let r = v.least + (v.most - v.least) * unit(seed, cx, cy, cz, n + 400);
    Some((at(cx, 100), at(cy, 200), at(cz, 300), r))
}

/// The ore at this block, if a vein reaches it.
pub fn ore(seed: u32, x: i32, y: i32, z: i32) -> Option<BlockId> {
    VEIN_CELLS.with_borrow_mut(|all| {
        for (k, v) in VEINS.iter().enumerate() {
            if y < v.from || y > v.to {
                continue;
            }
            let (cx, cy, cz) = (x.div_euclid(v.cell), y.div_euclid(v.cell), z.div_euclid(v.cell));
            let e = &mut all[k];
            if (e.0, e.1, e.2, e.3) != (seed, cx, cy, cz) {
                *e = (seed, cx, cy, cz, vein(seed, k, cx, cy, cz));
            }
            let Some((mx, my, mz, r)) = e.4 else { continue };
            let (dx, dy, dz) = ((mx - x) as f32, (my - y) as f32 * 1.3, (mz - z) as f32);
            let d = dx * dx + dy * dy + dz * dz;
            if d > r * r * 1.45 {
                continue;
            }
            // lumpy: each block a little in or out
            let lump = 0.55 + 0.9 * unit(seed, x, y, z, 10 + k as u32 + 500);
            if d < r * r * lump {
                return Some(v.ore);
            }
        }
        None
    })
}

/// A crystal hollow at this block: air inside, crystal ore (or stone) in
/// its shell, nothing outside it.
fn hollow(seed: u32, x: i32, y: i32, z: i32) -> Option<BlockId> {
    const CELL: i32 = 40;
    if !(-116..=-44).contains(&y) {
        return None;
    }
    let (cx, cy, cz) = (x.div_euclid(CELL), y.div_euclid(CELL), z.div_euclid(CELL));
    if unit(seed, cx, cy, cz, 30) >= 0.18 {
        return None;
    }
    let r = 3.5 + 2.0 * unit(seed, cx, cy, cz, 31);
    let span = (CELL - 16) as f32;
    let mid = |c: i32, n: u32| c * CELL + 8 + (unit(seed, cx, cy, cz, n) * span) as i32;
    let (mx, my, mz) = (mid(cx, 32), mid(cy, 33).clamp(-110, -50), mid(cz, 34));
    let d = (((x - mx).pow(2) + (y - my).pow(2) + (z - mz).pow(2)) as f32).sqrt();
    if d < r - 1.0 {
        Some(AIR)
    } else if d < r + 0.5 {
        Some(if unit(seed, x, y, z, 35) < 0.6 { blocks::CRYSTAL_ORE } else { STONE })
    } else {
        None
    }
}

/// Old workings: in some stretches of `REGION` blocks, a long corridor
/// east to west at one depth and a few running north or south off it,
/// three wide and three high, with posts and a beam every few blocks and
/// old chests at the ends of the side ones.
const REGION: i32 = 80;

/// What `workings` hands back inside for "leave it as it is".
const NONE: BlockId = BlockId::MAX;

/// What old workings put at this block, if they reach it.
fn workings(seed: u32, x: i32, y: i32, z: i32, ground: i32) -> Option<BlockId> {
    let (rx, rz) = (x.div_euclid(REGION), z.div_euclid(REGION));
    if unit(seed, rx, 0, rz, 40) >= 0.45 {
        return None;
    }
    let floor = -72 + (unit(seed, rx, 0, rz, 41) * 72.0) as i32;
    if y < floor - 1 || y > floor + 2 {
        return None;
    }
    // never near the surface
    if ground - (floor + 2) < 8 {
        return None;
    }
    let (x0, z0) = (rx * REGION, rz * REGION);
    let main_z = z0 + 30 + (unit(seed, rx, 0, rz, 42) * 20.0) as i32;
    let (main_from, main_to) = (x0 + 6, x0 + REGION - 6);
    // a corridor along one way, `along` from `from` to `to`, `across` its middle
    let corridor = |along: i32, across: i32, mid: i32, from: i32, to: i32, ends: bool| -> Option<BlockId> {
        if along < from || along > to || (across - mid).abs() > 1 {
            return None;
        }
        let side = across - mid;
        let up = y - floor;
        // under the corridor, boards where a cave would leave a hole
        if up < 0 {
            return Some(if cave(seed, x, y, z, ground) { PLANKS } else { NONE });
        }
        // posts at the sides and a beam over them, every six blocks
        if (along - from) % 6 == 0 && along != from && along != to {
            if side != 0 && up < 2 {
                return Some(LOG);
            }
            if up == 2 {
                return Some(PLANKS);
            }
        }
        // a chest at the far end, against one side
        if ends && along == to && side == 1 && up == 0 {
            return Some(blocks::OLD_CHEST);
        }
        Some(AIR)
    };
    if let Some(b) = corridor(x, z, main_z, main_from, main_to, false) {
        return (b != NONE).then_some(b);
    }
    // two to four side corridors, north or south
    let n = 2 + (unit(seed, rx, 0, rz, 43) * 3.0) as i32;
    for i in 0..n {
        let at = x0 + 10 + (i * (REGION - 20)) / n.max(1) + (unit(seed, rx, i, rz, 44) * 6.0) as i32;
        // short enough to stay in the region, whichever way it runs
        let long = 12 + (unit(seed, rx, i, rz, 45) * 16.0) as i32;
        let north = unit(seed, rx, i, rz, 46) < 0.5;
        let chest = unit(seed, rx, i, rz, 47) < 0.75;
        // along runs away from the main corridor, so its far end is `to`
        let (along, from, to) = if north { (main_z - z, 2, long) } else { (z - main_z, 2, long) };
        if let Some(b) = corridor(along, x, at, from, to, chest) {
            return (b != NONE).then_some(b);
        }
    }
    None
}

/// Whether this block is left open: a cave, a corridor or a hollow.
fn open(seed: u32, x: i32, y: i32, z: i32, ground: i32) -> bool {
    workings(seed, x, y, z, ground).map_or_else(|| hollow(seed, x, y, z) == Some(AIR) || cave(seed, x, y, z, ground), |b| b == AIR)
}

/// What block `b` becomes underground at (x, y, z), in a column whose
/// ground is at `ground`: air for a cave, an ore, part of old workings or a
/// crystal hollow, a glowcap, or `b` itself. Only stone, slate, dirt and
/// gravel change, and grass or snow on top where a cave comes out.
pub fn carve(seed: u32, x: i32, y: i32, z: i32, ground: i32, b: BlockId) -> BlockId {
    if !ground_stuff(b, y == ground) || y <= FLOOR {
        return b;
    }
    if let Some(w) = workings(seed, x, y, z, ground) {
        return w;
    }
    if let Some(c) = hollow(seed, x, y, z) {
        return c;
    }
    if cave(seed, x, y, z, ground) {
        // a glowcap now and then on a cave floor, well under the ground
        if y < ground - 12 && y - 1 > FLOOR && unit(seed, x, y, z, 50) < 0.05 && !open(seed, x, y - 1, z, ground) {
            return blocks::GLOWCAP;
        }
        return AIR;
    }
    if b == STONE || b == SLATE {
        if let Some(o) = ore(seed, x, y, z) {
            return o;
        }
    }
    b
}
