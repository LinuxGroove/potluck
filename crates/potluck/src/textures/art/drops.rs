//! What hostile creatures leave, and meat cooked: silk from Cave Crawlers,
//! leather, bristles and meat from Bramble Boars, embers from Ember Imps,
//! a sentinel core from a Stone Sentinel and gloom dust from Gloomlings.
//! Icons only, with the same dark outline as the rest.

use super::super::*;
use super::{clear, outline};

const OUTLINE: &[u32] = &[0x121014, 0x1c1a1e];
const SILK: &[u32] = &[0x8a8a96, 0xaeaebc, 0xceced8, 0xe6e6ee, 0xf8f8fc];
const LEATHER: &[u32] = &[0x3a1e0e, 0x56301a, 0x744426, 0x925a34, 0xae7446, 0xc8905c];
const BRISTLE: &[u32] = &[0x1a1612, 0x2c2620, 0x403830, 0x584e44, 0x726658];
const MEAT: &[u32] = &[0x5a1218, 0x82202a, 0xa83440, 0xc8505a, 0xe27a80, 0xf4a8aa];
const FAT: &[u32] = &[0xc8b4a4, 0xe0d0c2, 0xf4eae0];
const COOKED: &[u32] = &[0x2e140a, 0x4a2212, 0x6a361c, 0x8a4c28, 0xa86636, 0xc4844a];
const BONE: &[u32] = &[0x9a9284, 0xc2baa8, 0xe0d8c8, 0xf6f0e4];
const EMBER: &[u32] = &[0x1c0c06, 0x3a1408, 0x6a200a, 0xa8320c, 0xe05a12, 0xfc9a2a, 0xfede7a];
const STONE: &[u32] = &[0x2c2e34, 0x44464e, 0x5c5f68, 0x767a84, 0x9498a2];
const RUNE: &[u32] = &[0x1e4a6a, 0x2e74a0, 0x4aa4d4, 0x7ed0f4, 0xc4f0ff];
const GLOOM: &[u32] = &[0x0e0a1a, 0x1c1630, 0x2c2448, 0x403664, 0x5a4c86, 0x8a7ab8];
const SPARK: &[u32] = &[0xb0a060, 0xe4d48a, 0xfff2b8];

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

/// A skein of silk: loops of pale thread wound round, with a loose end.
fn silk() -> Tex {
    let mut a = Art::new("silk", &[SILK, OUTLINE]);
    a.fill(|x, y, _| {
        if let Some(d) = oval(x, y, 32.0, 32.0, 20.0, 14.0, -0.5) {
            // the hole through the middle of the loops
            if d < 0.42 {
                return clear();
            }
            let (dx, dy) = (x + 0.5 - 32.0, y + 0.5 - 32.0);
            let wound = ((dx * 0.8 - dy * 1.3) * 0.9).sin() * 0.5 + 0.5;
            let round = 1.0 - ((d - 0.71) / 0.29).abs();
            return px(0, 0.35 + 0.45 * round + 0.15 * wound, 0.3 + 0.6 * round, 0.6);
        }
        // the loose end, trailing down
        let t = (y + 0.5 - 42.0) / 16.0;
        if (0.0..1.0).contains(&t) && (x + 0.5 - (44.0 + 6.0 * (t * 3.0).sin())).abs() <= 1.0 {
            return px(0, 0.8, 0.5, 0.6);
        }
        clear()
    });
    outline(&mut a, 1);
    a.finish(1.2, 0.5)
}

/// A hide of leather, its edges ragged, with a stitched patch.
fn leather() -> Tex {
    let s = seed_of("leather");
    let mut a = Art::new("leather", &[LEATHER, OUTLINE]);
    a.fill(|x, y, _| {
        let (dx, dy) = (x + 0.5 - 32.0, y + 0.5 - 33.0);
        let ang = dy.atan2(dx);
        // four legs' worth of corners, and a ragged edge
        let edge = 19.0 + 4.0 * (2.0 * ang).cos().powi(2) + 2.0 * (vn(ang * 8.0 + 20.0, 6.0, 16, 4, s) - 0.5) * 2.0;
        let d = (dx * dx + dy * dy).sqrt();
        if d <= edge {
            let k = d / edge;
            let grain = vn(x * 0.9, y * 0.9, 16, 3, s ^ 5) * 0.2;
            let stitch = (dx.abs() < 7.0 && (dy - 6.0).abs() < 0.8 && (x as i32) % 3 == 0) || ((dx.abs() - 7.0).abs() < 0.8 && (-1.0..7.0).contains(&dy) && (y as i32) % 3 == 0);
            if stitch {
                return px(0, 0.95, 0.7, 0.9);
            }
            return px(0, 0.7 - 0.35 * k + grain, 0.3 + 0.3 * (1.0 - k), 0.75);
        }
        clear()
    });
    outline(&mut a, 1);
    a.finish(1.3, 0.5)
}

/// A bundle of stiff dark bristles tied in the middle.
fn bristles() -> Tex {
    let s = seed_of("bristles");
    let mut a = Art::new("bristles", &[BRISTLE, LEATHER, OUTLINE]);
    a.fill(|x, y, _| {
        let (x, y) = (x + 0.5, y + 0.5);
        // a fan of bristles, narrow at the tie and splaying at both ends
        let t = (y - 32.0) / 22.0;
        if t.abs() <= 1.0 {
            let half = 5.0 + 9.0 * t.abs();
            let dx = x - 32.0;
            if dx.abs() <= half {
                if (y - 32.0).abs() < 3.0 {
                    return px(1, 0.6 + 0.2 * (1.0 - dx.abs() / half), 0.7, 0.8);
                }
                // each bristle a line from the tie outward
                let k = dx / half;
                let line = hash((k * 9.0).round() as i32, 0, s);
                let tip = if t.abs() > 0.92 && hash(x as i32, y as i32, s ^ 3) > 0.5 { -0.2 } else { 0.0 };
                return px(0, 0.25 + 0.5 * line + 0.2 * (1.0 - k.abs()) + tip, 0.5, 0.9);
            }
        }
        clear()
    });
    outline(&mut a, 2);
    a.finish(1.2, 0.5)
}

/// A slab of meat with a round of bone through it, raw or cooked.
fn meat(name: &'static str, cooked: bool) -> Tex {
    let s = seed_of(name);
    let flesh = if cooked { COOKED } else { MEAT };
    let mut a = Art::new(name, &[flesh, FAT, BONE, OUTLINE]);
    a.fill(|x, y, _| {
        let bone = ((x + 0.5 - 40.0).powi(2) + (y + 0.5 - 28.0).powi(2)).sqrt();
        if let Some(d) = oval(x, y, 32.0, 34.0, 22.0, 15.0, -0.35) {
            if bone < 3.0 {
                return px(1, 0.3, 0.2, 0.6);
            }
            if bone < 6.0 {
                return px(2, 0.85 - 0.3 * (bone - 3.0) / 3.0, 0.8, 0.5);
            }
            // a rind of fat round the edge
            if d > 0.84 {
                return px(1, if cooked { 0.1 } else { 0.7 - 0.4 * (d - 0.84) / 0.16 }, 0.5, 0.6);
            }
            let marble = vn(x * 1.2, y * 1.2, 16, 3, s) * 0.25;
            let sear = if cooked { 0.15 * ((x * 0.7 + y * 0.4).sin() > 0.7) as i32 as f32 } else { 0.0 };
            return px(0, 0.75 - 0.35 * d + marble - sear, 0.4 + 0.4 * (1.0 - d), if cooked { 0.7 } else { 0.35 });
        }
        clear()
    });
    outline(&mut a, 3);
    a.finish(1.3, 0.5)
}

/// An ember: a lump of coal glowing orange through its cracks.
fn ember() -> Tex {
    let s = seed_of("ember");
    let mut a = Art::new("ember", &[EMBER, OUTLINE]);
    a.fill(|x, y, _| {
        let (dx, dy) = (x + 0.5 - 32.0, y + 0.5 - 34.0);
        let ang = dy.atan2(dx);
        let edge = 17.0 + 3.0 * (vn(ang * 6.0 + 10.0, 4.0, 16, 3, s) - 0.5) * 2.0;
        let d = (dx * dx + dy * dy).sqrt();
        if d <= edge {
            let k = d / edge;
            let crack = (vn(x * 0.5, y * 0.5, 16, 3, s ^ 9) - 0.5).abs() < 0.06;
            if crack {
                return px(0, 0.95, 0.2, 0.5);
            }
            // hotter in the middle, dark crust at the edge
            let heat = (1.0 - k).powf(1.5);
            return px(0, 0.1 + 0.6 * heat + 0.1 * vn(x, y, 8, 2, s ^ 4), 0.5 + 0.4 * (1.0 - k), 0.8);
        }
        clear()
    });
    outline(&mut a, 1);
    a.finish(1.3, 0.5)
}

/// A sentinel core: a heavy stone, cut in eight sides, with a glowing rune.
fn sentinel_core() -> Tex {
    let mut a = Art::new("sentinel_core", &[STONE, RUNE, OUTLINE]);
    a.fill(|x, y, _| {
        let (dx, dy) = (x + 0.5 - 32.0, y + 0.5 - 32.0);
        let oct = dx.abs().max(dy.abs()).max((dx.abs() + dy.abs()) * 0.72);
        if oct <= 21.0 {
            // the rune: a ring with a bar through it
            let r = (dx * dx + dy * dy).sqrt();
            if (r - 8.0).abs() < 1.6 || (dx.abs() < 1.4 && dy.abs() < 12.0) {
                return px(1, 0.85 - 0.3 * (r / 12.0).min(1.0), 0.3, 0.2);
            }
            let bevel = oct > 17.0;
            let lit = 0.55 - 0.02 * (dx + dy) * 0.5;
            return px(0, if bevel { lit + 0.15 * (-dx - dy).signum() } else { lit }, if bevel { 0.5 } else { 0.8 }, 0.85);
        }
        clear()
    });
    outline(&mut a, 2);
    a.finish(1.3, 0.5)
}

/// A pinch of gloom dust: a little dark heap with pale sparks in it.
fn gloom_dust() -> Tex {
    let s = seed_of("gloom_dust");
    let mut a = Art::new("gloom_dust", &[GLOOM, SPARK, OUTLINE]);
    a.fill(|x, y, _| {
        let (x, y) = (x + 0.5, y + 0.5);
        // a heap: a low mound on a wider spill
        let mound = 46.0 - 22.0 * (1.0 - ((x - 32.0) / 22.0).powi(2)).max(0.0).sqrt();
        let spill = (y - 48.0).abs() < 3.0 && (x - 32.0).abs() < 24.0;
        if (y >= mound && y <= 50.0 && (x - 32.0).abs() <= 22.0) || spill {
            if hash(x as i32, y as i32, s) > 0.93 {
                return px(1, 0.6 + 0.4 * hash(x as i32, y as i32, s ^ 1), 0.6, 0.2);
            }
            let k = ((y - mound) / (50.0 - mound).max(1.0)).clamp(0.0, 1.0);
            return px(0, 0.75 - 0.45 * k + 0.15 * vn(x, y, 6, 2, s ^ 2), 0.4, 0.95);
        }
        // a few sparks floating over it
        for &(sx, sy) in &[(24.0, 20.0), (39.0, 16.0), (31.0, 10.0)] {
            if (x - sx).abs() + (y - sy).abs() < 2.2 {
                return px(1, 0.9, 0.5, 0.2);
            }
        }
        clear()
    });
    outline(&mut a, 2);
    a.finish(1.2, 0.5)
}

/// Each one's icon.
pub(super) fn tiles() -> Vec<(&'static str, Tex)> {
    vec![
        ("silk", silk()),
        ("leather", leather()),
        ("bristles", bristles()),
        ("raw_meat", meat("raw_meat", false)),
        ("cooked_meat", meat("cooked_meat", true)),
        ("ember", ember()),
        ("sentinel_core", sentinel_core()),
        ("gloom_dust", gloom_dust()),
    ]
}
