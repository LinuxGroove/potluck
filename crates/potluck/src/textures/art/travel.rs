//! Tiles for getting about: the hot air balloon's basket (block 72), woven
//! wicker under a rope rim. Its side tile is a picture of the model from the
//! side (the rim over the body), which is its icon too.

use super::*;

const WICKER: &[u32] = &[0x3f2a14, 0x5a3d1c, 0x775426, 0x936c33, 0xad8543, 0xc49d57, 0xd8b56e];
const ROPE: &[u32] = &[0x4a3520, 0x6b4e30, 0x8c6a45, 0xa9875d, 0xc2a277];

fn clear() -> Px {
    Px { pal: 0, t: 0.0, h: 0.0, rough: 1.0, alpha: 0.0 }
}

/// Woven wicker: weavers 4 px tall passing over and under stakes 8 px apart.
fn weave(x: f32, y: f32, s: u32) -> Px {
    let (xi, yi) = (x as i32, y as i32);
    let (row, col) = (yi.div_euclid(4), xi.div_euclid(8));
    let (across, along) = ((yi.rem_euclid(4) as f32 + 0.5) / 4.0, (xi.rem_euclid(8) as f32 + 0.5) / 8.0);
    let jitter = 0.08 * (hash(col, row, s) - 0.5);
    if (row + col).rem_euclid(2) == 0 {
        // a weaver over a stake: round across, dipping into shadow at its ends
        let round = 1.0 - (2.0 * across - 1.0).powi(2);
        let dip = 1.0 - (2.0 * along - 1.0).abs().powi(4);
        Px { pal: 0, t: 0.35 + 0.35 * round * dip + 0.1 * (1.0 - across) + jitter, h: 0.3 + 0.6 * round * dip, rough: 0.8, alpha: 1.0 }
    } else if (2..6).contains(&xi.rem_euclid(8)) {
        // the stake showing where the weaver goes behind it
        let round = 1.0 - ((xi.rem_euclid(8) as f32 - 3.5) / 2.5).powi(2);
        Px { pal: 0, t: 0.4 + 0.3 * round + jitter, h: 0.35 + 0.45 * round, rough: 0.75, alpha: 1.0 }
    } else {
        // the weaver in the stake's shadow
        Px { pal: 0, t: 0.12 + 0.1 * across + jitter, h: 0.15, rough: 0.9, alpha: 1.0 }
    }
}

/// A twisted rope, its strands running at a slant.
fn rope(x: f32, y: f32, s: u32) -> Px {
    let twist = ((x + y * 1.6) * 0.9).sin();
    let t = 0.5 + 0.3 * twist + 0.08 * (fbm(x, y, 8, 2, s) - 0.5);
    Px { pal: 1, t, h: 0.5 + 0.35 * twist, rough: 0.85, alpha: 1.0 }
}

/// The basket from the side: the rim (2 sixteenths) over the body (10).
fn basket() -> Tex {
    let s = seed_of("basket");
    let mut a = Art::new("basket", &[WICKER, ROPE]);
    a.fill(|x, y, _| {
        let (xi, yi) = (x as i32, y as i32);
        if (4..60).contains(&xi) && (16..24).contains(&yi) {
            let mut p = rope(x, y, s);
            if yi == 23 {
                p.t -= 0.25;
            }
            return p;
        }
        if (8..56).contains(&xi) && (24..64).contains(&yi) {
            let mut p = weave(x, y, s);
            // darker under the rim and toward the ground, lighter round the middle
            p.t -= 0.2 * smooth(27.0, 24.0, y) + 0.12 * smooth(54.0, 64.0, y);
            p.t -= 0.1 * ((x - 31.5).abs() / 24.0).powi(2);
            if xi == 8 || xi == 55 {
                p.t -= 0.15;
            }
            return p;
        }
        clear()
    });
    a.finish(1.6, 0.45)
}

/// The basket from above: the rope rim round a woven floor in shadow.
fn basket_top() -> Tex {
    let s = seed_of("basket_top");
    let mut a = Art::new("basket_top", &[WICKER, ROPE]);
    a.fill(|x, y, _| {
        let d = (x - 31.5).abs().max((y - 31.5).abs());
        if d >= 24.0 {
            // the rim, twisting round
            let along = if (x - 31.5).abs() > (y - 31.5).abs() { y } else { x };
            let mut p = rope(along, d * 2.0, s);
            if d < 25.0 {
                p.t -= 0.2;
            }
            return p;
        }
        let mut p = weave(y, x, s ^ 5);
        // the inside is in the walls' shadow
        p.t = p.t * 0.7 - 0.1 * smooth(14.0, 24.0, d);
        p
    });
    a.finish(1.4, 0.4)
}

/// The basket's tiles.
pub(super) fn tiles() -> Vec<(&'static str, Tex)> {
    vec![("basket", basket()), ("basket_top", basket_top())]
}
