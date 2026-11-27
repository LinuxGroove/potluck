//! Tiles for the lands past the island: slate (the deep stone), ice over
//! cold water, cactus in the desert, reeds at the water's edge and dry
//! scrub. Painted like the rest (`art.rs`), from its stone and sand
//! palettes where they share a look.

use super::*;

const SLATE: &[u32] = &[0x1d1f26, 0x272a33, 0x323641, 0x3e434f, 0x4b515e, 0x5a606d];
const ICE: &[u32] = &[0x6f9fc4, 0x82b0d2, 0x96c1de, 0xabd0e8, 0xc2def0, 0xdbecf7];
const CACTUS: &[u32] = &[0x1f3f1f, 0x2a5228, 0x356533, 0x41793e, 0x4f8c49, 0x62a057];
const SPINE: &[u32] = &[0xb8a77a, 0xd7c99a, 0xefe4bd];
const REED: &[u32] = &[0x3f5a24, 0x52702d, 0x678737, 0x7f9e43, 0x99b453];
const HEAD: &[u32] = &[0x3d2414, 0x55331c, 0x6d4426, 0x855631];
const SCRUB: &[u32] = &[0x4f3a22, 0x6a4f2f, 0x86683f, 0xa18352, 0xb99d68];

fn clear() -> Px {
    Px { pal: 0, t: 0.0, h: 0.0, rough: 1.0, alpha: 0.0 }
}

/// Slate: darker than stone, in thin layers that split along the grain.
fn slate() -> Tex {
    let s = seed_of("slate");
    let mut a = Art::new("slate", &[SLATE]);
    a.fill(|x, y, _| {
        let bend = 5.0 * fbm(x, y, 2, 2, s ^ 3);
        let layer = ((y + bend) / 5.0).floor();
        let within = ((y + bend) / 5.0).fract();
        let m = fbm(x, y, 8, 3, s);
        let mut t = 0.45 + 0.25 * (hash(layer as i32, 0, s) - 0.5) + 0.35 * (m - 0.5);
        let mut h = 0.5 + 0.3 * m;
        // the split between layers, open in places
        if within < 0.18 && fbm(x, y, 4, 2, s ^ 7) > 0.45 {
            t = 0.05 + 0.1 * m;
            h = 0.05;
        } else if within > 0.82 {
            t += 0.1;
        }
        if hash(x as i32, y as i32, s ^ 11) < 0.02 {
            t += 0.2;
        }
        px(0, t, h, 0.7 + 0.2 * (1.0 - h))
    });
    a.finish(2.5, 0.9)
}

/// Ice: clear blue with pale cracks and bubbles.
fn ice() -> Tex {
    let s = seed_of("ice");
    let mut a = Art::new("ice", &[ICE]);
    a.fill(|x, y, _| {
        let c = cell(x, y, 4, 4, 0.9, s);
        let m = fbm(x, y, 4, 3, s ^ 5);
        let mut t = 0.45 + 0.3 * (m - 0.5) + 0.1 * (c.rand(1) - 0.5);
        if c.edge < 0.9 && fbm(x, y, 4, 2, s ^ 9) > 0.4 {
            t = 0.95;
        }
        if hash(x as i32, y as i32, s ^ 3) < 0.012 {
            t = 0.85;
        }
        // a sheen across it, lighter to the top left
        t += 0.12 * (1.0 - (x + y) / 128.0);
        px(0, t, 0.5 + 0.1 * m, 0.15)
    });
    a.finish(1.0, 0.3)
}

/// A cactus's side: ribs running up it with spines along them.
fn cactus_side() -> Tex {
    let s = seed_of("cactus_side");
    let mut a = Art::new("cactus_side", &[CACTUS, SPINE]);
    a.fill(|x, y, _| {
        let rib = (x / 8.0).fract();
        let round = 1.0 - (2.0 * rib - 1.0).powi(2);
        let (xi, yi) = (x as i32, y as i32);
        if (xi % 8 == 4) && (yi + (xi / 8) * 3) % 7 == 0 {
            return px(1, 0.7 + 0.3 * hash(xi, yi, s), 0.9, 0.6);
        }
        let m = fbm(x, y, 4, 2, s);
        px(0, 0.2 + 0.6 * round + 0.15 * (m - 0.5), 0.3 + 0.6 * round, 0.7)
    });
    a.finish(2.0, 0.7)
}

/// A cactus's top: the ribs meeting in a star, a pale tuft in the middle.
fn cactus_top() -> Tex {
    let s = seed_of("cactus_top");
    let mut a = Art::new("cactus_top", &[CACTUS, SPINE]);
    a.fill(|x, y, _| {
        let (dx, dy) = (x - 31.5, y - 31.5);
        let r = dx.hypot(dy);
        if r < 4.0 {
            return px(1, 0.6 + 0.3 * hash(x as i32, y as i32, s), 0.8, 0.7);
        }
        let ang = dy.atan2(dx);
        let rib = ((ang / std::f32::consts::TAU * 8.0).fract() * 2.0 - 1.0).abs();
        let m = fbm(x, y, 4, 2, s);
        px(0, 0.25 + 0.5 * (1.0 - rib) + 0.15 * (m - 0.5) - 0.15 * smooth(20.0, 32.0, r), 0.4 + 0.4 * (1.0 - rib), 0.7)
    });
    a.finish(2.0, 0.7)
}

/// Reeds: tall thin stalks, a few with brown heads, the gaps clear.
fn reeds() -> Tex {
    let s = seed_of("reeds");
    let mut a = Art::new("reeds", &[REED, HEAD]);
    a.fill(|_, _, _| clear());
    for i in 0..11 {
        let x0 = 3.0 + i as f32 * 5.6 + (hash(i, 0, s) - 0.5) * 3.0;
        let top = 2.0 + hash(i, 1, s) * 20.0;
        let lean = (hash(i, 2, s) - 0.5) * 8.0;
        let light = 0.3 + 0.6 * hash(i, 3, s);
        let foot = Vec2::new(x0, 64.0);
        let tip = Vec2::new(x0 + lean, top);
        a.line(foot, tip, |f, p| *p = Px { pal: 0, t: light * (0.6 + 0.4 * f), h: 0.6, rough: 0.8, alpha: 1.0 });
        a.line(foot + Vec2::X, tip + Vec2::X, |f, p| *p = Px { pal: 0, t: light * (0.4 + 0.3 * f), h: 0.5, rough: 0.8, alpha: 1.0 });
        if hash(i, 4, s) < 0.45 {
            // a seed head a little below the tip
            let at = tip.lerp(foot, 0.08);
            for dy in 0..9 {
                for dx in 0..3 {
                    let q = at + Vec2::new(dx as f32 - 0.5 + lean * dy as f32 / 64.0, dy as f32);
                    let t = 0.3 + 0.5 * (1.0 - (dx as f32 - 1.0).abs() * 0.5) - 0.2 * (dy as f32 / 9.0);
                    *a.at(q.x as i32, q.y as i32) = Px { pal: 1, t, h: 0.8, rough: 0.9, alpha: 1.0 };
                }
            }
        }
    }
    a.finish(1.5, 0.5)
}

/// Dry scrub: a tangle of bare twigs from one root.
fn dry_bush() -> Tex {
    let s = seed_of("dry_bush");
    let mut a = Art::new("dry_bush", &[SCRUB]);
    a.fill(|_, _, _| clear());
    let mut twigs = vec![(Vec2::new(32.0, 63.0), -std::f32::consts::FRAC_PI_2, 22.0, 0)];
    let mut n = 0u32;
    while let Some((p, ang, len, depth)) = twigs.pop() {
        n += 1;
        let q = p + Vec2::from_angle(ang) * len;
        let light = 0.3 + 0.15 * depth as f32 + 0.3 * hash(n as i32, 0, s);
        a.line(p, q, |_, px| *px = Px { pal: 0, t: light, h: 0.6, rough: 0.9, alpha: 1.0 });
        if depth < 4 && n < 60 {
            for k in 0..2 {
                let turn = (hash(n as i32, k + 1, s) - 0.5) * 1.4 + if k == 0 { -0.45 } else { 0.45 };
                twigs.push((q, ang + turn, len * (0.55 + 0.2 * hash(n as i32, k + 3, s)), depth + 1));
            }
        }
    }
    a.finish(1.5, 0.5)
}

pub(super) fn tiles() -> Vec<(&'static str, Tex)> {
    vec![
        ("slate", slate()),
        ("ice", ice()),
        ("cactus_side", cactus_side()),
        ("cactus_top", cactus_top()),
        ("reeds", reeds()),
        ("dry_bush", dry_bush()),
    ]
}
