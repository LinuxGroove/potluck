//! Places built into the island: a palisade camp on the meadow by the
//! south-east shore, a terraced farm on the hill's east slope and a
//! sandstone villa on the west coast with a colonnade, a library and a
//! roof terrace. They show off the blocks made of boxes, and the gallery's
//! views (`tools/gallery.sh`) look at them.

use bevy::prelude::*;

use super::{Voxels, WX, WZ};
use crate::blocks::{self, *};
use crate::noise::Rng;

/// The camp's middle and the radius of its palisade.
pub const CAMP: (i32, i32, i32) = (268, 326, 10);
/// The farm's terraces, west to east and north to south.
pub const FARM: (i32, i32, i32, i32) = (206, 236, 296, 322);
/// The villa's terrace, west to east and north to south.
pub const VILLA: (i32, i32, i32, i32) = (38, 64, 194, 218);

/// Whether trees and plants should stay off a spot (the places' grounds).
pub fn keep_clear(x: i32, z: i32) -> bool {
    let (cx, cz, r) = CAMP;
    let camp = (x - cx).pow(2) + (z - cz).pow(2) <= (r + 4).pow(2);
    let inside = |(x0, x1, z0, z1): (i32, i32, i32, i32), m: i32| x >= x0 - m && x <= x1 + m && z >= z0 - m && z <= z1 + m;
    camp || inside(FARM, 3) || inside(VILLA, 4)
}

pub(super) fn build(w: &mut Voxels, hs: &[i32], r: &mut Rng) {
    let ground = |x: i32, z: i32| hs[(z.clamp(0, WZ - 1) * WX + x.clamp(0, WX - 1)) as usize];
    camp(w, &ground, r);
    farm(w, &ground, r);
    villa(w, &ground);
}

fn fill(w: &mut Voxels, x: std::ops::RangeInclusive<i32>, y: std::ops::RangeInclusive<i32>, z: std::ops::RangeInclusive<i32>, id: BlockId) {
    for zz in z {
        for yy in y.clone() {
            for xx in x.clone() {
                w.put(xx, yy, zz, id);
            }
        }
    }
}

/// Sets a column's ground to `top`: filled with dirt (stone deeper down)
/// up to it, `cap` on it, and nothing above for `clear` blocks.
fn level(w: &mut Voxels, x: i32, z: i32, top: i32, cap: BlockId, clear: i32) {
    for y in 1..top {
        let b = w.get(x, y, z);
        if matches!(b, AIR | WATER) || blocks::shape(b) == Shape::Cross || matches!(b, LEAVES | BIRCH_LEAVES | PINE | LOG | BIRCH) {
            w.put(x, y, z, if y < top - 3 { STONE } else { DIRT });
        }
    }
    w.put(x, top, z, cap);
    for y in top + 1..=top + clear {
        w.put(x, y, z, AIR);
    }
}

// ---------- the camp ----------

/// A palisade of sharpened stakes round tents, a watchtower and a fire.
fn camp(w: &mut Voxels, ground: &dyn Fn(i32, i32) -> i32, r: &mut Rng) {
    let (cx, cz, rad) = CAMP;
    // the camp stands level, at the height most of its ground is
    let mut hs: Vec<i32> = (-rad..=rad).flat_map(|z| (-rad..=rad).map(move |x| (x, z))).filter(|(x, z)| x * x + z * z <= rad * rad).map(|(x, z)| ground(cx + x, cz + z)).collect();
    hs.sort();
    let y0 = hs[hs.len() / 2];
    for z in -rad - 3..=rad + 3 {
        for x in -rad - 3..=rad + 3 {
            let d = ((x * x + z * z) as f32).sqrt();
            if d > rad as f32 + 3.0 {
                continue;
            }
            // inside, flat and trodden; outside, easing back to the meadow
            let g = ground(cx + x, cz + z);
            let top = if d <= rad as f32 + 0.5 { y0 } else { (y0 + g) / 2 };
            let worn = d < 4.0 || (z.abs() <= 1 && x > 0);
            level(w, cx + x, cz + z, top, if worn && r.next() < 0.6 { GRAVEL } else { GRASS }, 14);
        }
    }
    // the palisade, two stakes high, with a gate to the east
    let mut seen = std::collections::HashSet::new();
    for k in 0..360 {
        let a = (k as f32).to_radians();
        let (x, z) = ((a.cos() * rad as f32).round() as i32, (a.sin() * rad as f32).round() as i32);
        if !seen.insert((x, z)) || (x > 0 && z.abs() <= 1) {
            continue;
        }
        w.put(cx + x, y0 + 1, cz + z, STAKE);
        // a few stand a little shorter
        if (x * 7 + z * 13).rem_euclid(9) != 0 {
            w.put(cx + x, y0 + 2, cz + z, STAKE);
        }
    }
    for z in [-2, 2] {
        for y in 1..=4 {
            w.put(cx + rad, y0 + y, cz + z, LOG);
        }
    }
    for z in -2..=2 {
        w.put(cx + rad, y0 + 5, cz + z, LOG);
    }
    w.put(cx + rad + 1, y0 + 4, cz - 2, LANTERN);
    w.put(cx + rad + 1, y0 + 4, cz + 2, LANTERN);
    // the fire in the middle, with stools round it
    w.put(cx, y0 + 1, cz, BRAZIER);
    for (x, z) in [(2, 0), (-2, 0), (0, 2), (0, -2)] {
        w.put(cx + x, y0 + 1, cz + z, STOOL);
    }
    // a watchtower by the west wall: four posts, a deck with a rail, a thatched roof
    let (tx, tz) = (cx - 6, cz - 2);
    for (x, z) in [(-1, -1), (1, -1), (-1, 1), (1, 1)] {
        for y in 1..=9 {
            w.put(tx + x, y0 + y, tz + z, LOG);
        }
    }
    fill(w, tx - 2..=tx + 2, y0 + 6..=y0 + 6, tz - 2..=tz + 2, PLANKS);
    for x in -2i32..=2 {
        for z in -2i32..=2 {
            if (x.abs() == 2 || z.abs() == 2) && !(x == 2 && z == 0) {
                w.put(tx + x, y0 + 7, tz + z, FENCE);
            }
        }
    }
    w.put(tx, y0 + 7, tz, BRAZIER);
    for y in 0..=1 {
        let s = 2 - y;
        fill(w, tx - s..=tx + s, y0 + 10 + y..=y0 + 10 + y, tz - s..=tz + s, HAY);
    }
    // two tents
    for (ox, oz, c) in [(-1, 5, WOOL_CREAM), (3, -6, WOOL_RED)] {
        tent(w, cx + ox, y0, cz + oz, c);
    }
    // stores: barrels and crates against the wall, hay, a table with a candle
    for (x, z, id) in [(5, 6, BARREL), (6, 5, BARREL), (5, 5, CRATE), (6, 6, CRATE), (-3, -7, BARREL), (-2, -8, CRATE), (-7, 4, HAY), (-6, 5, BARREL)] {
        w.put(cx + x, y0 + 1, cz + z, id);
    }
    w.put(cx + 5, y0 + 2, cz + 5, CRATE);
    w.put(cx + 4, y0 + 1, cz - 3, TABLE);
    w.put(cx + 4, y0 + 2, cz - 3, CANDLE);
    w.put(cx + 3, y0 + 1, cz - 3, STOOL);
    w.put(cx + 6, y0 + 1, cz + 3, POT);
    // grass and flowers outside the wall
    for _ in 0..90 {
        let a = r.next() * std::f32::consts::TAU;
        let d = rad as f32 + 1.0 + r.next() * 2.5;
        let (x, z) = (cx + (a.cos() * d) as i32, cz + (a.sin() * d) as i32);
        let y = w.top(x, z);
        if w.get(x, y, z) == GRASS && w.get(x, y + 1, z) == AIR {
            w.put(x, y + 1, z, if r.next() < 0.7 { TALLGRASS } else { FLOWER_WHITE });
        }
    }
}

/// An A-frame tent of wool along z, open at both ends.
fn tent(w: &mut Voxels, x: i32, y0: i32, z: i32, c: BlockId) {
    for dz in -2i32..=2 {
        for (dy, dx) in [(1, 2), (2, 1), (3, 0)] {
            let id = if dz.rem_euclid(2) == 0 { c } else { WOOL_CREAM };
            w.put(x + dx, y0 + dy, z + dz, id);
            w.put(x - dx, y0 + dy, z + dz, id);
        }
    }
}

// ---------- the farm ----------

/// Terraces stepping down the hill's east slope: rows of wheat and carrots
/// on ploughed soil, a channel of water down each, stone walls between,
/// braziers along the walls and a scarecrow.
fn farm(w: &mut Voxels, ground: &dyn Fn(i32, i32) -> i32, r: &mut Rng) {
    let (x0, x1, z0, z1) = FARM;
    const WIDE: i32 = 6;
    let strips = (x1 - x0 + 1) / WIDE;
    for s in 0..strips {
        let sx = x0 + s * WIDE;
        // each terrace sits at its strip's average height, never above the one uphill
        let avg = (sx..sx + WIDE).flat_map(|x| (z0..=z1).map(move |z| (x, z))).map(|(x, z)| ground(x, z)).sum::<i32>() / (WIDE * (z1 - z0 + 1));
        let top = avg;
        let crop = if s % 2 == 0 { WHEAT } else { CARROTS };
        for z in z0..=z1 {
            for i in 0..WIDE {
                let x = sx + i;
                let edge = z == z0 || z == z1;
                if i == WIDE - 1 {
                    // the wall down to the next terrace, its top a path
                    level(w, x, z, top, COBBLE, 14);
                    for y in top - 4..top {
                        w.put(x, y, z, COBBLE);
                    }
                    continue;
                }
                if edge {
                    level(w, x, z, top, GRASS, 14);
                    w.put(x, top + 1, z, FENCE);
                    continue;
                }
                if i == 2 {
                    level(w, x, z, top, WATER, 14);
                    w.put(x, top - 1, z, DIRT);
                    continue;
                }
                level(w, x, z, top, FARMLAND, 14);
                if r.next() < 0.93 {
                    w.put(x, top + 1, z, crop);
                }
            }
            // braziers along the wall's path
            if (z - z0) % 8 == 4 {
                w.put(sx + WIDE - 1, top + 1, z, BRAZIER);
            }
        }
        // the ground beyond the farm's ends eases back down
        for z in [z0 - 1, z1 + 1] {
            for x in sx..sx + WIDE {
                let g = ground(x, z);
                level(w, x, z, (g + top) / 2, GRASS, 14);
            }
        }
        if s == strips / 2 {
            scarecrow(w, sx + 1, top, (z0 + z1) / 2);
        }
    }
}

/// A fence post in a hay coat, with a wool hat.
fn scarecrow(w: &mut Voxels, x: i32, y: i32, z: i32) {
    w.put(x, y + 1, z, FENCE);
    w.put(x, y + 2, z, HAY);
    w.put(x, y + 3, z, WOOL_CREAM);
    w.put(x, y + 4, z, WOOL_BLUE);
    w.put(x, y + 2, z - 1, FENCE);
    w.put(x, y + 2, z + 1, FENCE);
}

// ---------- the villa ----------

/// A sandstone villa facing the sea: a colonnade on the west, a library
/// behind it with a skylight, and a roof terrace with a parapet.
fn villa(w: &mut Voxels, ground: &dyn Fn(i32, i32) -> i32) {
    let (x0, x1, z0, z1) = VILLA;
    // the terrace it stands on
    let base = 25;
    for z in z0 - 3..=z1 + 3 {
        for x in x0 - 3..=x1 + 3 {
            let out = x < x0 || x > x1 || z < z0 || z > z1;
            if out {
                // a slope of sand and grass down from the terrace
                let g = ground(x, z);
                let d = (x0 - x).max(x - x1).max(z0 - z).max(z - z1);
                let top = (base - d).max(g);
                level(w, x, z, top, if top <= 20 { SAND } else { GRASS }, 12);
                continue;
            }
            level(w, x, z, base, SANDSTONE, 16);
            if x == x0 || x == x1 || z == z0 || z == z1 {
                for y in base - 5..=base {
                    if w.get(x, y, z) != BEDROCK {
                        w.put(x, y, z, SANDSTONE_BRICKS);
                    }
                }
            }
        }
    }
    // the house: walls from 46 to 62 across, 198 to 214 down, five high
    let (hx0, hx1, hz0, hz1) = (x0 + 8, x1 - 2, z0 + 4, z1 - 4);
    let (fy, top) = (base + 1, base + 5);
    fill(w, hx0..=hx1, base..=base, hz0..=hz1, PLANKS);
    for z in hz0..=hz1 {
        for x in hx0..=hx1 {
            let wall = x == hx0 || x == hx1 || z == hz0 || z == hz1;
            if wall {
                fill(w, x..=x, fy..=top, z..=z, SANDSTONE_BRICKS);
            }
        }
    }
    // a door into the library from the colonnade, and high windows north and south
    fill(w, hx0..=hx0, fy..=fy + 2, (hz0 + hz1) / 2 - 1..=(hz0 + hz1) / 2 + 1, AIR);
    for x in (hx0 + 2..hx1).step_by(3) {
        w.put(x, top - 1, hz0, GLASS);
        w.put(x, top - 1, hz1, GLASS);
    }
    // the roof, with a skylight down the middle, and a parapet round it
    fill(w, hx0 - 6..=hx1, top + 1..=top + 1, hz0..=hz1, SANDSTONE);
    fill(w, hx0 + 3..=hx1 - 3, top + 1..=top + 1, (hz0 + hz1) / 2 - 1..=(hz0 + hz1) / 2 + 1, GLASS);
    for z in hz0..=hz1 {
        for x in hx0 - 6..=hx1 {
            let rim = x == hx0 - 6 || x == hx1 || z == hz0 || z == hz1;
            if rim && (x + z) % 4 != 1 {
                w.put(x, top + 2, z, SANDSTONE_BRICKS);
            }
        }
    }
    // the colonnade: columns along the west edge holding up the roof's overhang
    let cx = hx0 - 5;
    for z in (hz0..=hz1).step_by(3) {
        fill(w, cx..=cx, fy..=top, z..=z, COLUMN);
    }
    // in the colonnade: a table with a candle, stools, pots and a barrel
    let mid = (hz0 + hz1) / 2;
    w.put(cx + 2, fy, mid - 3, TABLE);
    w.put(cx + 2, fy + 1, mid - 3, CANDLE);
    w.put(cx + 2, fy, mid - 2, STOOL);
    w.put(cx + 2, fy, mid - 4, STOOL);
    for z in [hz0 + 1, hz1 - 1] {
        w.put(cx + 1, fy, z, POT);
        w.put(cx + 3, fy, z, POT);
    }
    w.put(hx0 - 1, fy, hz1 - 1, BARREL);
    // the library: shelves three high along the north and south walls, a
    // long table down the middle with candles and stools
    for x in hx0 + 1..hx1 {
        for z in [hz0 + 1, hz1 - 1] {
            fill(w, x..=x, fy..=fy + 2, z..=z, BOOKSHELF);
        }
    }
    for x in hx0 + 3..=hx1 - 3 {
        w.put(x, fy, mid, TABLE);
        if (x - hx0) % 3 == 0 {
            w.put(x, fy + 1, mid, CANDLE);
        }
        if (x - hx0) % 2 == 0 {
            w.put(x, fy, mid - 1, STOOL);
            w.put(x, fy, mid + 1, STOOL);
        }
    }
    w.put(hx1 - 1, fy, hz0 + 2, CRATE);
    w.put(hx1 - 1, fy, hz1 - 2, BARREL);
    // the roof terrace: pots, a table and stools, barrels and braziers at the corners
    let ry = top + 2;
    for (x, z, id) in [
        (hx0 - 4, hz0 + 1, BRAZIER),
        (hx0 - 4, hz1 - 1, BRAZIER),
        (hx1 - 1, hz0 + 1, BARREL),
        (hx1 - 2, hz0 + 1, CRATE),
        (hx1 - 1, hz1 - 1, POT),
        (hx0 - 2, mid + 4, TABLE),
        (hx0 - 1, mid + 4, STOOL),
        (hx0 - 3, mid + 4, STOOL),
        (hx0 - 4, mid - 4, POT),
        (hx0 - 4, mid - 5, POT),
    ] {
        w.put(x, ry, z, id);
    }
    w.put(hx0 - 2, ry + 1, mid + 4, CANDLE);
}
