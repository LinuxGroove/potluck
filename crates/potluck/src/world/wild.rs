//! The lands past the island: an endless world made a column at a time
//! from the seed, the same on every machine and in any order. Round the
//! island there's open sea, then land rises out of it: meadows and woods
//! like the island's nearby, and further out pine woods, snowfields,
//! deserts, dry grassland, marshes and mountains, with rivers between.
//!
//! Every column comes from pure functions of (seed, x, z): the ground's
//! height, the climate and so the biome. Trees reach over a column's edge,
//! so a column is laid out in an `Area` with a margin round it: every tree
//! whose trunk is in the margin is grown too (each from its own seed, in
//! the same order everywhere) and only the column's own blocks are kept.

use bevy::prelude::*;

use super::{deep, height_at, Area, Column, CHUNK, MIN_Y, SEA, WX, WY, WZ};
use crate::blocks::*;
use crate::noise::{fbm, hash2, smooth, vnoise, Rng};

/// How far past a column's edge trees are grown, so the crowns of trees
/// standing just outside it reach in.
const MARGIN: i32 = 12;

/// The kinds of land.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Biome {
    /// The island itself.
    Home,
    /// Open water.
    Sea,
    Beach,
    /// Grass, flowers and the odd oak, like the island's meadows.
    Meadow,
    /// Oaks and birches with ferns under them.
    Forest,
    /// Tall birches.
    Birchwood,
    /// Pines on cooler ground.
    Pinewood,
    /// Snow, a few pines, frozen water.
    Snowfield,
    /// Sand over sandstone, cactus and dry scrub.
    Desert,
    /// Long dry grass and a few broad oaks.
    Savanna,
    /// Low wet ground with pools and reeds.
    Marsh,
    /// Bare rock and snow up high.
    Mountain,
}

impl Biome {
    pub fn name(self) -> &'static str {
        match self {
            Biome::Home => "the island",
            Biome::Sea => "the sea",
            Biome::Beach => "a beach",
            Biome::Meadow => "meadows",
            Biome::Forest => "a forest",
            Biome::Birchwood => "birch woods",
            Biome::Pinewood => "pine woods",
            Biome::Snowfield => "snowfields",
            Biome::Desert => "a desert",
            Biome::Savanna => "dry grassland",
            Biome::Marsh => "a marsh",
            Biome::Mountain => "the mountains",
        }
    }
}

/// How warm and how wet a place is, each 0 to 1. The island and the lands
/// near it are mild (about a half each); further out it varies.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Climate {
    pub warm: f32,
    pub wet: f32,
}

fn salt(seed: u32) -> i32 {
    (seed % 100_003) as i32 * 131 + 7
}

/// How far a spot is outside the island, in blocks (0 on it), a little
/// uneven so the sea round it isn't a square.
fn out_from_island(x: f32, z: f32, s: i32) -> f32 {
    let dx = (-x).max(x - (WX - 1) as f32).max(0.0);
    let dz = (-z).max(z - (WZ - 1) as f32).max(0.0);
    let d = dx.hypot(dz);
    if d <= 0.0 {
        return 0.0;
    }
    (d + (fbm(x / 90.0, z / 90.0, s + 120, 3) - 0.5) * 40.0).max(0.0)
}

/// The climate at a spot.
pub fn climate(seed: u32, x: f32, z: f32) -> Climate {
    let s = salt(seed);
    let d = out_from_island(x, z, s);
    // a little wander, so bands of climate aren't smooth blobs
    let (wx, wz) = (x + (fbm(x / 300.0, z / 300.0, s + 130, 2) - 0.5) * 400.0, z + (fbm(x / 300.0, z / 300.0, s + 131, 2) - 0.5) * 400.0);
    let warm = smooth(0.3, 0.7, fbm(wx / 1500.0, wz / 1500.0, s + 105, 3));
    let wet = smooth(0.3, 0.7, fbm(wx / 1000.0, wz / 1000.0, s + 106, 3));
    // near the island it's as mild as the island
    let near = smooth(250.0, 900.0, d);
    Climate { warm: 0.5 + (warm - 0.5) * near, wet: 0.5 + (wet - 0.5) * near }
}

/// The ground's height past the island, before it meets the island's sea.
fn wild_height(x: f32, z: f32, s: i32, d: f32) -> f32 {
    let sea = SEA as f32;
    // continents and oceans; land is likelier within sight of the island
    let cont = fbm(x / 1200.0, z / 1200.0, s + 101, 4) + 0.12 * smooth(900.0, 200.0, d);
    let land = smooth(0.42, 0.54, cont);
    let hills = fbm(x / 230.0, z / 230.0, s + 102, 5);
    let detail = fbm(x / 36.0, z / 36.0, s + 108, 4);
    let mut h_land = sea + 2.5 + (hills - 0.42) * 34.0 + (detail - 0.5) * 6.0;
    // mountain ranges: ridges on a wide swell
    let range = smooth(0.55, 0.72, fbm(x / 700.0, z / 700.0, s + 104, 3));
    let ridge = 1.0 - (fbm(x / 170.0, z / 170.0, s + 103, 5) * 2.0 - 1.0).abs();
    h_land += range * (12.0 + ridge * ridge * 72.0 + fbm(x / 17.0, z / 17.0, s + 109, 4) * 9.0);
    // marshes sit low and flat, with pools
    let c = climate_raw(x, z, s, d);
    let marsh = smooth(0.62, 0.72, c.warm) * smooth(0.56, 0.66, c.wet) * (1.0 - range);
    h_land = h_land + (sea + 0.4 + (vnoise(x / 6.0, z / 6.0, s + 110) - 0.5) * 3.0 - h_land) * marsh;
    // the sea floor, deeper far from shore
    let deep = smooth(0.42, 0.22, cont);
    let h_sea = sea - 4.0 - deep * 34.0 + (detail - 0.5) * 6.0;
    let mut h = h_sea + (h_land - h_sea) * land;
    // rivers, winding across the land down to the sea
    let rv = (fbm(x / 560.0, z / 560.0, s + 107, 4) - 0.5).abs();
    let river = smooth(0.014, 0.004, rv) * land * (1.0 - range);
    h = h.min(h + (sea - 3.0 - h) * river);
    h
}

/// The climate with no pull toward the island's (the marsh needs it
/// before the blend, and it's cheap).
fn climate_raw(x: f32, z: f32, s: i32, d: f32) -> Climate {
    let (wx, wz) = (x + (fbm(x / 300.0, z / 300.0, s + 130, 2) - 0.5) * 400.0, z + (fbm(x / 300.0, z / 300.0, s + 131, 2) - 0.5) * 400.0);
    let warm = smooth(0.3, 0.7, fbm(wx / 1500.0, wz / 1500.0, s + 105, 3));
    let wet = smooth(0.3, 0.7, fbm(wx / 1000.0, wz / 1000.0, s + 106, 3));
    let near = smooth(250.0, 900.0, d);
    Climate { warm: 0.5 + (warm - 0.5) * near, wet: 0.5 + (wet - 0.5) * near }
}

/// The ground's height anywhere: the island's own near it, the wild's
/// further out, and the sea between.
pub fn height(seed: u32, x: f32, z: f32) -> f32 {
    let s = salt(seed);
    let d = out_from_island(x, z, s);
    let home = height_at(x, z, (seed % 1000) as i32 * 31);
    if d <= 0.0 {
        return home;
    }
    let t = smooth(36.0, 190.0, d);
    if t <= 0.0 {
        return home;
    }
    // a few sandbars in the island's sea, to rest on when swimming across
    let bar = smooth(0.74, 0.8, fbm(x / 26.0, z / 26.0, s + 140, 3)) * smooth(10.0, 40.0, d) * (1.0 - t);
    let home = home + (SEA as f32 + 1.2 - home) * bar;
    home + (wild_height(x, z, s, d) - home) * t
}

/// The biome at a spot with ground at `h`.
pub fn biome_with(seed: u32, x: f32, z: f32, h: i32) -> Biome {
    if out_from_island(x, z, salt(seed)) <= 0.0 && (0.0..WX as f32).contains(&x) && (0.0..WZ as f32).contains(&z) {
        return Biome::Home;
    }
    let c = climate(seed, x, z);
    // the high cold
    let cold = c.warm - (h - SEA - 34).max(0) as f32 * 0.008;
    if h < SEA - 1 {
        return Biome::Sea;
    }
    if h > SEA + 54 {
        return Biome::Mountain;
    }
    if h <= SEA + 1 && cold >= 0.3 && !(c.warm > 0.62 && c.wet > 0.56) {
        return if c.warm > 0.62 && c.wet < 0.36 { Biome::Desert } else { Biome::Beach };
    }
    if cold < 0.3 {
        Biome::Snowfield
    } else if cold < 0.41 {
        Biome::Pinewood
    } else if c.warm < 0.62 {
        if c.wet < 0.4 {
            Biome::Meadow
        } else if c.wet < 0.63 {
            Biome::Forest
        } else {
            Biome::Birchwood
        }
    } else if c.wet < 0.36 {
        Biome::Desert
    } else if c.wet < 0.56 {
        Biome::Savanna
    } else {
        Biome::Marsh
    }
}

/// The biome at a spot.
pub fn biome(seed: u32, x: f32, z: f32) -> Biome {
    biome_with(seed, x, z, height(seed, x, z).floor() as i32)
}

/// A number from 0 to 1 for a spot and a purpose, the same everywhere.
fn dice(x: i32, z: i32, s: i32) -> f32 {
    hash2(x, z, s)
}

/// What a column's ground is made of from its top down to the deep stone.
fn lay(a: &mut Area, seed: u32, x: i32, z: i32, h: i32, slope: i32, b: Biome, from: i32) {
    let s = salt(seed);
    let h = h.clamp(MIN_Y + 2, WY - 8);
    for y in from.max(MIN_Y)..=h {
        let depth = h - y;
        let id = if y < 0 {
            deep(seed, x, y, z)
        } else {
            match b {
                Biome::Desert => {
                    if depth < 4 {
                        SAND
                    } else if depth < 9 {
                        SANDSTONE
                    } else {
                        STONE
                    }
                }
                Biome::Sea | Biome::Beach => {
                    let gravel = b == Biome::Sea && (h < SEA - 12 || dice(x, z, s + 150) < 0.04);
                    if depth < 3 {
                        if gravel { GRAVEL } else { SAND }
                    } else if depth < 5 {
                        if gravel { GRAVEL } else { SANDSTONE }
                    } else {
                        STONE
                    }
                }
                Biome::Mountain => {
                    if depth == 0 && h > SEA + 58 {
                        SNOW
                    } else if depth < 3 && h < SEA + 46 && slope < 3 {
                        if depth == 0 { GRASS } else { DIRT }
                    } else {
                        STONE
                    }
                }
                _ => {
                    if depth == 0 {
                        if slope >= 3 {
                            STONE
                        } else if b == Biome::Snowfield {
                            SNOW
                        } else {
                            GRASS
                        }
                    } else if depth < 4 && slope < 3 {
                        DIRT
                    } else {
                        STONE
                    }
                }
            }
        };
        let id = super::below(seed, x, y, z, h, id);
        a.put(x, y, z, id);
    }
    // the sea, rivers and pools, frozen over where it's cold
    for y in h + 1..=SEA {
        let id = if y == SEA && b == Biome::Snowfield { ICE } else { WATER };
        a.put(x, y, z, id);
    }
}

/// How likely a tree is to stand on a spot, and which.
fn tree_at(seed: u32, x: i32, z: i32, b: Biome) -> Option<Tree> {
    let s = salt(seed);
    let v = dice(x, z, s + 201);
    let k = dice(x, z, s + 202);
    let forest = fbm(x as f32 / 30.0, z as f32 / 30.0, s + 112, 4);
    let glade = fbm(x as f32 / 13.0, z as f32 / 13.0, s + 117, 3) > 0.62;
    let (chance, kind) = match b {
        Biome::Meadow => (0.0016 + 0.01 * smooth(0.6, 0.85, forest), if k < 0.25 { Tree::Birch } else { Tree::Oak(k > 0.8) }),
        Biome::Forest => (if glade { 0.0 } else { 0.012 + 0.022 * forest }, if k < 0.32 { Tree::Birch } else { Tree::Oak(k > 0.86) }),
        Biome::Birchwood => (if glade { 0.0 } else { 0.022 + 0.012 * forest }, if k < 0.85 { Tree::Birch } else { Tree::Oak(false) }),
        Biome::Pinewood => (if glade { 0.004 } else { 0.022 + 0.02 * forest }, Tree::Pine),
        Biome::Snowfield => (0.003 + 0.01 * smooth(0.6, 0.85, forest), Tree::Pine),
        Biome::Mountain => (0.008, Tree::Pine),
        Biome::Savanna => (0.0022, Tree::Oak(k > 0.4)),
        Biome::Marsh => (0.006, Tree::Oak(false)),
        Biome::Desert => (0.004, Tree::Cactus),
        _ => (0.0, Tree::Oak(false)),
    };
    (v < chance).then_some(kind)
}

#[derive(Clone, Copy)]
enum Tree {
    Oak(bool),
    Birch,
    Pine,
    Cactus,
}

/// A column past the island.
pub fn column(seed: u32, key: IVec2) -> Column {
    make(seed, key, IVec2::ONE).column(key)
}

/// Several columns together, `n` from `k0` (as `column` makes each: for
/// checking that what crosses their edges joins up).
#[cfg(test)]
pub(crate) fn columns(seed: u32, k0: IVec2, n: IVec2) -> Vec<(IVec2, Column)> {
    let a = make(seed, k0, n);
    (0..n.y).flat_map(|z| (0..n.x).map(move |x| k0 + IVec2::new(x, z))).map(|k| (k, a.column(k))).collect()
}

/// Lays out the columns `n` from `k0`, with the margin round them that
/// their trees reach in from.
fn make(seed: u32, k0: IVec2, n: IVec2) -> Area {
    let s = salt(seed);
    let (x0, z0) = (k0.x * CHUNK, k0.y * CHUNK);
    let (cw, cd) = (n.x * CHUNK, n.y * CHUNK);
    let (ax, az, w, d) = (x0 - MARGIN, z0 - MARGIN, cw + 2 * MARGIN, cd + 2 * MARGIN);
    // heights over the area and one round it, for slopes
    let hw = w + 2;
    let hs: Vec<i32> = (0..hw * (d + 2)).map(|i| height(seed, (ax - 1 + i % hw) as f32, (az - 1 + i / hw) as f32).floor() as i32).collect();
    let h_at = |x: i32, z: i32| hs[((z - az + 1) * hw + (x - ax + 1)) as usize];
    let mut a = Area::new(ax, az, w, d);
    let mut biomes = vec![Biome::Sea; (w * d) as usize];
    for z in az..az + d {
        for x in ax..ax + w {
            let h = h_at(x, z);
            let slope = [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().map(|(dx, dz)| (h_at(x + dx, z + dz) - h).abs()).max().unwrap();
            let b = biome_with(seed, x as f32, z as f32, h);
            biomes[((z - az) * w + (x - ax)) as usize] = b;
            let inside = x >= x0 && x < x0 + cw && z >= z0 && z < z0 + cd;
            // the margin only needs its top, for trees to stand on
            lay(&mut a, seed, x, z, h, slope, b, if inside { MIN_Y } else { h - 4 });
        }
    }
    let biome_of = |x: i32, z: i32| biomes[((z - az) * w + (x - ax)) as usize];
    // trees, each grown from its own seed in the same order everywhere
    for z in az..az + d {
        for x in ax..ax + w {
            // the island grows its own
            if (0..WX).contains(&x) && (0..WZ).contains(&z) {
                continue;
            }
            let b = biome_of(x, z);
            let Some(kind) = tree_at(seed, x, z, b) else { continue };
            let h = h_at(x, z);
            let ground = a.get(x, h, z);
            let footing = match kind {
                Tree::Cactus => ground == SAND && a.get(x, h + 1, z) == AIR,
                Tree::Pine if b == Biome::Snowfield || b == Biome::Mountain => matches!(ground, GRASS | SNOW),
                _ => ground == GRASS,
            };
            if !footing || h <= SEA {
                continue;
            }
            let mut r = Rng::new((dice(x, z, s + 203) * 4_294_967_295.0) as u32);
            let y0 = h + 1;
            match kind {
                Tree::Oak(big) => super::oak(&mut a, x, y0, z, &mut r, big),
                Tree::Birch => super::birch(&mut a, x, y0, z, &mut r),
                Tree::Pine => super::pine(&mut a, x, y0, z, &mut r),
                Tree::Cactus => {
                    let n = 2 + (r.next() * 2.4) as i32;
                    for y in 0..n {
                        a.put(x, y0 + y, z, CACTUS);
                    }
                }
            }
        }
    }
    // shrubs, grass, flowers and reeds on the columns' own ground
    let flowers = [FLOWER_RED, FLOWER_BLUE, FLOWER_WHITE, FLOWER_VIOLET];
    for z in z0..z0 + cd {
        for x in x0..x0 + cw {
            let h = h_at(x, z);
            let b = biome_of(x, z);
            let top = a.get(x, h, z);
            let above = a.get(x, h + 1, z);
            let v = dice(x, z, s + 210);
            let (fx, fz) = (x as f32, z as f32);
            // reeds stand at the water's edge, thickest in marshes
            let by_water = (h == SEA || h == SEA + 1) && [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().any(|(dx, dz)| h_at(x + dx, z + dz) < SEA);
            if by_water && above == AIR && matches!(top, GRASS | DIRT | SAND) && !matches!(b, Biome::Snowfield | Biome::Desert | Biome::Sea) && v < if b == Biome::Marsh { 0.6 } else { 0.1 } {
                a.put(x, h + 1, z, REEDS);
                continue;
            }
            if above != AIR {
                continue;
            }
            match (b, top) {
                (Biome::Desert, SAND) if v < 0.012 => a.put(x, h + 1, z, DRY_BUSH),
                (_, GRASS) => {
                    let shade = (1..12).any(|d| matches!(a.get(x, h + d, z), LEAVES | BIRCH_LEAVES | PINE));
                    let patch = fbm(fx / 7.0, fz / 7.0, s + 115, 3);
                    let lush = fbm(fx / 9.0, fz / 9.0, s + 114, 5);
                    let id = match b {
                        Biome::Savanna => {
                            if v < 0.004 {
                                Some(DRY_BUSH)
                            } else if v < 0.4 {
                                Some(TALLGRASS)
                            } else if patch > 0.72 && v < 0.5 {
                                Some(FLOWER_WHITE)
                            } else {
                                None
                            }
                        }
                        Biome::Marsh => {
                            if v < 0.2 {
                                Some(TALLGRASS)
                            } else if v < 0.3 {
                                Some(FERN)
                            } else {
                                None
                            }
                        }
                        Biome::Pinewood | Biome::Mountain => {
                            if v < 0.12 {
                                Some(FERN)
                            } else if v < 0.16 {
                                Some(BUSH)
                            } else if v < 0.2 {
                                Some(TALLGRASS)
                            } else {
                                None
                            }
                        }
                        _ => {
                            if shade && v < 0.16 {
                                Some(if dice(x, z, s + 211) < 0.22 { BUSH } else { FERN })
                            } else if patch > 0.64 && v < 0.5 {
                                Some(flowers[(fbm(fx / 23.0, fz / 23.0, s + 116, 2) * 7.99) as usize % 4])
                            } else if dice(x, z, s + 212) < 0.1 * (0.4 + lush) {
                                Some(TALLGRASS)
                            } else {
                                None
                            }
                        }
                    };
                    if let Some(id) = id {
                        a.put(x, h + 1, z, id);
                    } else if matches!(b, Biome::Meadow | Biome::Forest) && dice(x, z, s + 213) < 0.004 && (2..CHUNK - 2).contains(&(x & (CHUNK - 1))) && (2..CHUNK - 2).contains(&(z & (CHUNK - 1))) {
                        // (clear of the column's edge, which a shrub mustn't cross)
                        let mut r = Rng::new((dice(x, z, s + 214) * 4_294_967_295.0) as u32);
                        super::shrub(&mut a, x, h + 1, z, &mut r);
                    }
                }
                _ => {}
            }
        }
    }
    a
}
