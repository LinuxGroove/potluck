//! Food and what farm animals give, for Farming, the furnace's cooking pot,
//! hens, goats and bees: seeds, berries, an apple, a baked potato, pies,
//! stews, porridge, honey, a honey cake, a feather and a fried egg. Icons
//! only, with the same dark outline as the rest.

use super::ores::{clear, outline};
use super::*;

const OUTLINE: &[u32] = &[0x121014, 0x1c1a1e];
const SEED: &[u32] = &[0x5a4220, 0x7f6030, 0xa88446, 0xc9a866, 0xe4cc94];
const BERRY: &[u32] = &[0x2a0a1e, 0x4a1032, 0x721a48, 0x9c2a5e, 0xc8487a, 0xea7ea4];
const LEAF: &[u32] = &[0x1c3412, 0x2c4e1a, 0x3f6a24, 0x568a32, 0x74a848];
const APPLE: &[u32] = &[0x4a0c0c, 0x761414, 0xa01e1a, 0xc83224, 0xe25a3a, 0xf49a70];
const STEM: &[u32] = &[0x2e1c0e, 0x4a2e18, 0x684424];
const SKIN: &[u32] = &[0x4a2c14, 0x6a4220, 0x8c5c30, 0xac7a44, 0xc89a5e];
const FLESH: &[u32] = &[0xc8a860, 0xe0c67e, 0xf2e0a4, 0xfcf2c8];
const BUTTER: &[u32] = &[0xe8c040, 0xf6dc6a, 0xfff0a0];
const PASTRY: &[u32] = &[0x5a3212, 0x7e4a1e, 0xa2662c, 0xc28640, 0xdca85e, 0xf0c88a];
const PUMPKIN: &[u32] = &[0x6a2c06, 0x9a440c, 0xc45e14, 0xe07a22, 0xf2983a, 0xfcb860];
const BOWL: &[u32] = &[0x2e1a0c, 0x4a2c16, 0x6a4222, 0x8a5a30, 0xa87442, 0xc49058];
const STEW: &[u32] = &[0x3a1c0a, 0x5a2e12, 0x7a421c, 0x9a5a2a, 0xb4743c];
const BEAN: &[u32] = &[0x3a120c, 0x5e1e14, 0x82301e, 0xa4462c, 0xc06a4a];
const CREAM: &[u32] = &[0x9a8460, 0xbaa47e, 0xd6c29e, 0xeadcbe, 0xf8f0dc];
const FISH: &[u32] = &[0x9a5a48, 0xc07a64, 0xe0a088, 0xf2c4b0];
const CARROT: &[u32] = &[0xb04a10, 0xd46e26, 0xec923e];
const HONEY: &[u32] = &[0x5a2e04, 0x8a4a06, 0xb86c0c, 0xdc9018, 0xf2b434, 0xfcd870];
const JAR: &[u32] = &[0x5a6a70, 0x8a9aa0, 0xb8c6cc, 0xe2ecf0];
const SPONGE: &[u32] = &[0x8a5a22, 0xae7a36, 0xcc9a50, 0xe4bc74, 0xf4d89a];
const FEATHER: &[u32] = &[0x8a8478, 0xb0aa9c, 0xd0cabc, 0xe8e4d8, 0xfaf8f0];
const WHITE: &[u32] = &[0xb8b0a0, 0xd8d2c4, 0xf0ece2, 0xfcfaf4];
const YOLK: &[u32] = &[0xc06a08, 0xe08e14, 0xf4b028, 0xfcd456];

fn px(pal: usize, t: f32, h: f32, rough: f32) -> Px {
    Px { pal, t: t.clamp(0.0, 1.0), h, rough, alpha: 1.0 }
}

/// How far into an ellipse round (`cx`, `cy`), `a` by `b`, turned `turn`
/// radians, a point is: 0 at its middle to 1 at its edge, or None outside.
fn oval(x: f32, y: f32, cx: f32, cy: f32, a: f32, b: f32, turn: f32) -> Option<f32> {
    let (dx, dy) = (x + 0.5 - cx, y + 0.5 - cy);
    let (c, s) = (turn.cos(), turn.sin());
    let (u, v) = ((dx * c + dy * s) / a, (-dx * s + dy * c) / b);
    let d = u * u + v * v;
    (d <= 1.0).then_some(d.sqrt())
}

/// A round thing lit from the top left: how light it is at a point inside
/// it, `d` in from its edge.
fn lit(x: f32, y: f32, cx: f32, cy: f32, r: f32, d: f32) -> f32 {
    let toward = ((cx - r * 0.35 - x).powi(2) + (cy - r * 0.35 - y).powi(2)).sqrt() / r;
    0.85 - 0.45 * toward - 0.2 * d
}

/// A scatter of pale seeds.
fn seeds() -> Tex {
    let mut a = Art::new("seeds", &[SEED, OUTLINE]);
    let lie = [(22.0, 26.0, 0.6), (36.0, 22.0, -0.4), (44.0, 34.0, 1.2), (28.0, 38.0, -1.0), (38.0, 46.0, 0.3), (20.0, 48.0, 0.9), (48.0, 48.0, -0.7)];
    a.fill(|x, y, _| {
        for &(cx, cy, turn) in &lie {
            if let Some(d) = oval(x, y, cx, cy, 6.0, 3.2, turn) {
                let seam = oval(x, y, cx, cy, 4.0, 0.8, turn).is_some() as i32 as f32;
                return px(0, lit(x, y, cx, cy, 6.0, d) - 0.25 * seam, 0.4 + 0.5 * (1.0 - d), 0.8);
            }
        }
        clear()
    });
    outline(&mut a, 1);
    a.finish(1.4, 0.5)
}

/// A bunch of berries under two leaves.
fn berries() -> Tex {
    let mut a = Art::new("berries", &[BERRY, LEAF, STEM, OUTLINE]);
    let bunch = [(24.0, 30.0), (38.0, 28.0), (31.0, 40.0), (45.0, 40.0), (20.0, 44.0), (36.0, 52.0)];
    a.fill(|x, y, _| {
        for &(cx, cy) in bunch.iter().rev() {
            let d = ((x + 0.5 - cx).powi(2) + (y + 0.5 - cy).powi(2)).sqrt() / 8.0;
            if d <= 1.0 {
                let shine = ((x + 0.5 - (cx - 3.0)).powi(2) + (y + 0.5 - (cy - 3.0)).powi(2)) < 2.5;
                return px(0, if shine { 1.0 } else { lit(x, y, cx, cy, 8.0, d) }, 0.4 + 0.6 * (1.0 - d), 0.3);
            }
        }
        for &(cx, cy, turn) in &[(24.0, 15.0, -0.5), (42.0, 14.0, 0.5)] {
            if let Some(d) = oval(x, y, cx, cy, 10.0, 4.5, turn) {
                let vein = oval(x, y, cx, cy, 9.0, 0.6, turn).is_some() as i32 as f32;
                return px(1, 0.75 - 0.4 * d - 0.3 * vein, 0.5, 0.7);
            }
        }
        if (x - 33.0).abs() <= 1.0 && (16.0..24.0).contains(&y) {
            return px(2, 0.6, 0.5, 0.9);
        }
        clear()
    });
    outline(&mut a, 3);
    a.finish(1.4, 0.5)
}

/// A red apple with a stalk and a leaf.
fn apple() -> Tex {
    let mut a = Art::new("apple", &[APPLE, LEAF, STEM, OUTLINE]);
    a.fill(|x, y, _| {
        let (cx, cy) = (32.0, 38.0);
        // two lobes, and a dip at the top where the stalk goes in
        let l = ((x + 0.5 - 25.0).powi(2) + (y + 0.5 - cy).powi(2)).sqrt() / 17.0;
        let r = ((x + 0.5 - 39.0).powi(2) + (y + 0.5 - cy).powi(2)).sqrt() / 17.0;
        let d = l.min(r);
        let dip = ((x + 0.5 - cx).powi(2) + (y + 0.5 - 20.0).powi(2)).sqrt() < 5.0;
        if d <= 1.0 && !dip && y > 21.0 {
            let shine = ((x + 0.5 - 24.0).powi(2) + (y + 0.5 - 30.0).powi(2)) < 7.0;
            let streak = 0.08 * ((x * 1.3).sin() * (y * 0.2).cos());
            return px(0, if shine { 1.0 } else { lit(x, y, cx, cy, 20.0, d) + streak }, 0.4 + 0.6 * (1.0 - d), 0.35);
        }
        if (x - 32.5 - (24.0 - y) * 0.3).abs() <= 1.2 && (12.0..24.0).contains(&y) {
            return px(2, 0.5, 0.6, 0.9);
        }
        if let Some(d) = oval(x, y, 41.0, 15.0, 8.0, 3.5, -0.4) {
            return px(1, 0.75 - 0.45 * d, 0.5, 0.7);
        }
        clear()
    });
    outline(&mut a, 3);
    a.finish(1.4, 0.5)
}

/// A potato baked in its skin, split open with butter melting in it.
fn baked_potato() -> Tex {
    let s = seed_of("baked_potato");
    let mut a = Art::new("baked_potato", &[SKIN, FLESH, BUTTER, OUTLINE]);
    a.fill(|x, y, _| {
        if (29.0..36.0).contains(&x) && (29.0..34.0).contains(&y) {
            return px(2, 0.4 + 0.3 * ((x + y) * 0.7).sin().abs(), 0.9, 0.3);
        }
        if let Some(d) = oval(x, y, 32.0, 32.0, 15.0, 6.0, -0.1) {
            return px(1, 0.85 - 0.5 * d + 0.1 * (hash(x as i32, y as i32, s) - 0.5), 0.6 - 0.3 * (1.0 - d), 0.8);
        }
        if let Some(d) = oval(x, y, 32.0, 37.0, 25.0, 15.0, -0.1) {
            let spot = if hash((x / 3.0) as i32, (y / 3.0) as i32, s ^ 1) > 0.82 { -0.2 } else { 0.0 };
            return px(0, lit(x, y, 32.0, 37.0, 22.0, d) + spot, 0.4 + 0.5 * (1.0 - d), 0.85);
        }
        clear()
    });
    outline(&mut a, 3);
    a.finish(1.4, 0.5)
}

/// A whole pie seen from a little above: its filling, a crust round the
/// edge (and across it, for a berry pie) and its side.
fn pie(name: &'static str, filling: &'static [u32], lattice: bool) -> Tex {
    let s = seed_of(name);
    let mut a = Art::new(name, &[PASTRY, filling, OUTLINE]);
    let (cx, cy, rx, ry, deep) = (32.0, 28.0, 26.0, 13.0, 11.0);
    a.fill(|x, y, _| {
        if let Some(d) = oval(x, y, cx, cy, rx, ry, 0.0) {
            if d > 0.8 {
                // the crimped rim
                let crimp = (((x + 0.5 - cx).atan2(y + 0.5 - cy)) * 14.0).sin() * 0.15;
                return px(0, 0.7 + crimp - 0.2 * (d - 0.8) * 5.0, 0.8, 0.8);
            }
            let strip = |k: f32| (k.rem_euclid(9.0) - 4.5).abs() < 1.6;
            if lattice && (strip(x + y * 1.6) || strip(x - y * 1.6)) {
                return px(0, 0.75, 0.9, 0.8);
            }
            let speck = 0.1 * (hash(x as i32, y as i32, s) - 0.5);
            return px(1, 0.65 - 0.3 * d + speck, 0.4, 0.4);
        }
        // its side, down to the dish
        let below = oval(x, y, cx, cy + deep, rx, ry, 0.0).is_some();
        if below || ((x + 0.5 - cx).abs() <= rx && (cy..cy + deep).contains(&(y + 0.5))) {
            let round = ((x + 0.5 - cx) / rx).abs();
            let flute = ((x * 0.9).sin() * 0.5 + 0.5) * 0.12;
            return px(0, 0.55 - 0.35 * round * round + flute - 0.15 * ((y - cy) / (deep + ry)), 0.5, 0.85);
        }
        clear()
    });
    outline(&mut a, 2);
    a.finish(1.4, 0.5)
}

/// A wooden bowl full of something, with bits in it: each bit a palette
/// and where it floats.
fn bowl(name: &'static str, inside: &'static [u32], bits: &[(&'static [u32], &[(f32, f32)])]) -> Tex {
    let s = seed_of(name);
    let mut pals: Vec<&'static [u32]> = vec![BOWL, inside, OUTLINE];
    pals.extend(bits.iter().map(|b| b.0));
    let mut a = Art::new(name, &pals);
    let (cx, top, rx, ry, deep) = (32.0, 28.0, 25.0, 8.0, 22.0);
    a.fill(|x, y, _| {
        if let Some(d) = oval(x, y, cx, top, rx - 3.0, ry - 2.0, 0.0) {
            for (i, (_, at)) in bits.iter().enumerate() {
                for &(bx, by) in at.iter() {
                    if let Some(e) = oval(x, y, bx, by, 3.2, 2.2, 0.4 * bx) {
                        return px(3 + i, 0.8 - 0.5 * e, 0.8, 0.5);
                    }
                }
            }
            let swirl = 0.1 * ((x * 0.5 + y * 1.1 + 3.0 * fbm(x, y, 4, 2, s)).sin());
            return px(1, 0.7 - 0.35 * d + swirl, 0.5, 0.3);
        }
        if oval(x, y, cx, top, rx, ry, 0.0).is_some() {
            return px(0, 0.85, 0.7, 0.7);
        }
        let k = (y + 0.5 - top) / deep;
        if (0.0..=1.0).contains(&k) {
            let half = rx * (1.0 - k * k).sqrt();
            if (x + 0.5 - cx).abs() <= half {
                let round = ((x + 0.5 - cx) / half.max(1.0)).abs();
                let grain = 0.08 * ((y * 1.7 + 2.0 * fbm(x, y, 4, 2, s ^ 3) * 4.0).sin());
                return px(0, 0.6 - 0.4 * round * round - 0.25 * k + grain, 0.5 + 0.3 * (1.0 - round), 0.8);
            }
        }
        clear()
    });
    outline(&mut a, 2);
    a.finish(1.4, 0.5)
}

/// A jar of honey with a cloth tied over it.
fn honey() -> Tex {
    let mut a = Art::new("honey", &[JAR, HONEY, WOOL_RED, TWINE, OUTLINE]);
    a.fill(|x, y, _| {
        let (cx, x0) = (32.0, 15.0);
        let round = ((x + 0.5 - cx) / x0).abs();
        // the cloth over the lid, flaring out, and its string
        if (13.0..21.0).contains(&y) && round <= 1.05 + (21.0 - y) * 0.02 {
            if (17.0..19.0).contains(&y) {
                return px(3, 0.6, 0.7, 0.9);
            }
            return px(2, 0.75 - 0.3 * round + 0.1 * ((x * 1.2).sin()), 0.7, 0.9);
        }
        if (21.0..55.0).contains(&y) && round <= 1.0 {
            let corner = (y > 50.0) && round > 0.75;
            if corner && ((round - 0.75) * 4.0).powi(2) + ((y - 50.0) / 5.0).powi(2) > 1.0 {
                return clear();
            }
            let glint = (round - 0.55).abs() < 0.08 && y < 46.0 && x < cx;
            if glint {
                return px(0, 1.0, 0.8, 0.1);
            }
            if y > 27.0 {
                return px(1, 0.75 - 0.45 * round * round - 0.15 * ((y - 27.0) / 28.0), 0.6, 0.15);
            }
            return px(0, 0.55 - 0.2 * round, 0.6, 0.1);
        }
        clear()
    });
    outline(&mut a, 4);
    a.finish(1.3, 0.5)
}

/// A slab of honey cake, its top glazed and honey running down its front.
fn honey_cake() -> Tex {
    let mut a = Art::new("honey_cake", &[SPONGE, HONEY, OUTLINE]);
    a.fill(|x, y, _| {
        let (x, y) = (x + 0.5, y + 0.5);
        // the top, going back to the right; the front; and the right side
        let lean = (30.0 - y) * 0.8;
        let on_top = (20.0..30.0).contains(&y) && x >= 10.0 + lean && x <= 46.0 + lean;
        let front = (10.0..=46.0).contains(&x) && (30.0..=50.0).contains(&y);
        let side = x > 46.0 && x <= 54.0 && y >= 30.0 - (x - 46.0) * 1.25 && y <= 50.0 - (x - 46.0) * 1.25;
        let drip = |c: f32, len: f32| (x - c).abs() < 2.0 && y < 30.0 + len;
        if on_top {
            return px(1, 0.85 - 0.2 * ((y - 20.0) / 10.0), 0.8, 0.2);
        }
        if front {
            if drip(16.0, 9.0) || drip(27.0, 5.0) || drip(38.0, 11.0) || y < 32.5 {
                return px(1, 0.7, 0.9, 0.2);
            }
            let layer = (y - 41.0).abs() < 1.5;
            return px(if layer { 1 } else { 0 }, if layer { 0.5 } else { 0.65 - 0.25 * ((y - 30.0) / 20.0) }, 0.5, 0.9);
        }
        if side {
            return px(0, 0.35, 0.4, 0.9);
        }
        clear()
    });
    outline(&mut a, 2);
    a.finish(1.3, 0.5)
}

/// A white feather, corner to corner, with a pale quill.
fn feather() -> Tex {
    let mut a = Art::new("feather", &[FEATHER, OUTLINE]);
    a.fill(|x, y, _| {
        let (dx, dy) = (x + 0.5 - 32.0, y + 0.5 - 32.0);
        let (u, v) = ((dx - dy) * std::f32::consts::FRAC_1_SQRT_2, (dx + dy) * std::f32::consts::FRAC_1_SQRT_2);
        if v.abs() <= 0.9 && (-27.0..27.0).contains(&u) {
            return px(0, 0.95, 0.9, 0.4);
        }
        if (-16.0..27.0).contains(&u) {
            let k = (u + 16.0) / 43.0;
            let w = 9.0 * (std::f32::consts::PI * k).sin().powf(0.6) * if v < 0.0 { 1.0 } else { 0.8 };
            // a nick or two where the barbs part
            let nick = ((u - 6.0).abs() < 1.0 && v > 0.0 && v > w * 0.5) || ((u + 4.0).abs() < 1.0 && v < 0.0 && -v > w * 0.6);
            if v.abs() <= w && !nick {
                let barbs = ((u * 1.4 + v.abs() * 1.1).sin() * 0.5 + 0.5) * 0.15;
                return px(0, 0.75 - 0.3 * (v.abs() / w) + barbs, 0.5, 0.95);
            }
        }
        clear()
    });
    outline(&mut a, 1);
    a.finish(1.3, 0.5)
}

/// An egg fried: a wobbly white and a round yolk.
fn fried_egg() -> Tex {
    let s = seed_of("fried_egg");
    let mut a = Art::new("fried_egg", &[WHITE, YOLK, OUTLINE]);
    a.fill(|x, y, _| {
        let (dx, dy) = (x + 0.5 - 32.0, y + 0.5 - 33.0);
        let yolk = ((x + 0.5 - 29.0).powi(2) + (y + 0.5 - 30.0).powi(2)).sqrt() / 9.0;
        if yolk <= 1.0 {
            let shine = ((x + 0.5 - 26.0).powi(2) + (y + 0.5 - 27.0).powi(2)) < 4.0;
            return px(1, if shine { 1.0 } else { lit(x, y, 29.0, 30.0, 9.0, yolk) }, 0.5 + 0.5 * (1.0 - yolk), 0.3);
        }
        let ang = dy.atan2(dx);
        let edge = 21.0 + 3.5 * (vn(ang * 10.2 + 32.0, 8.0, 16, 4, s) - 0.5) * 2.0 + 1.5 * (3.0 * ang).sin();
        let d = (dx * dx + dy * dy).sqrt();
        if d <= edge {
            let k = d / edge;
            let brown = if k > 0.88 && hash(x as i32, y as i32, s ^ 2) > 0.5 { -0.35 } else { 0.0 };
            return px(0, 0.85 - 0.25 * k + brown, 0.25 + 0.1 * (1.0 - k), 0.5);
        }
        clear()
    });
    outline(&mut a, 2);
    a.finish(1.3, 0.5)
}

/// Each one's icon.
pub(super) fn tiles() -> Vec<(&'static str, Tex)> {
    vec![
        ("seeds", seeds()),
        ("berries", berries()),
        ("apple", apple()),
        ("baked_potato", baked_potato()),
        ("pumpkin_pie", pie("pumpkin_pie", PUMPKIN, false)),
        ("berry_pie", pie("berry_pie", BERRY, true)),
        ("bean_stew", bowl("bean_stew", STEW, &[(BEAN, &[(24.0, 27.0), (36.0, 25.0), (31.0, 30.0), (43.0, 29.0)]), (CARROT, &[(28.0, 24.0), (40.0, 31.0)])])),
        ("fish_stew", bowl("fish_stew", CREAM, &[(FISH, &[(25.0, 26.0), (37.0, 28.0), (31.0, 24.0)]), (CARROT, &[(42.0, 25.0), (22.0, 29.0)])])),
        ("porridge", bowl("porridge", CREAM, &[(BERRY, &[(30.0, 26.0), (36.0, 28.0)])])),
        ("honey", honey()),
        ("honey_cake", honey_cake()),
        ("feather", feather()),
        ("fried_egg", fried_egg()),
    ]
}
