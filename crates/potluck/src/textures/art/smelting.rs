//! What furnaces make and fire needs: charcoal, flint, the four ingots,
//! flint and steel, and cooked food (bread and roasted carrots). Icons
//! only: none of these is ever put down.

use super::ores::{chunk, clear, outline};
use super::*;

const CHARCOAL: &[u32] = &[0x120d0a, 0x1d1511, 0x2a1f18, 0x3a2c22, 0x4e3d30, 0x6a5444];
pub(super) const FLINT: &[u32] = &[0x16181e, 0x242833, 0x343a48, 0x48506a, 0x66708c, 0x9aa4bc];
pub(super) const COPPER_BAR: &[u32] = &[0x5a2610, 0x86391a, 0xb05024, 0xd06a30, 0xe98d4a, 0xfbb87a];
pub(super) const IRON_BAR: &[u32] = &[0x33363c, 0x4f535b, 0x6f747d, 0x939aa3, 0xb7bcc4, 0xdde1e6];
const GOLD_BAR: &[u32] = &[0x6a4508, 0x93640e, 0xbb8a18, 0xdcae2c, 0xf2cd52, 0xfde99a];
pub(super) const MOONSILVER_BAR: &[u32] = &[0x2c3a56, 0x46597e, 0x6782a8, 0x93aed0, 0xbdd2ec, 0xe6f0fb];
const OUTLINE: &[u32] = &[0x121014, 0x1c1a1e];
const CRUST: &[u32] = &[0x4e2a0e, 0x6f3e16, 0x91561f, 0xb0702c, 0xcb8d40, 0xe0ac60, 0xf0cc8e];
const CRUMB: &[u32] = &[0xc9a870, 0xdcbf88, 0xecd6a6];
const ROAST: &[u32] = &[0x3a1406, 0x62240a, 0x8c3a10, 0xb45218, 0xd46e26, 0xec923e];
const TOPS: &[u32] = &[0x1e2c10, 0x2e4218, 0x405a22, 0x55722e];

/// A metal bar seen from the front and a little above: a lit top face
/// over a front face in shade, each a trapezoid, as ingots are cast.
fn ingot(name: &'static str, pal: &'static [u32]) -> Tex {
    let s = seed_of(name);
    let mut a = Art::new(name, &[pal, OUTLINE]);
    a.fill(|x, y, _| {
        let (x, y) = (x + 0.5, y + 0.5);
        let sparkle = hash(x as i32, y as i32, s);
        // top face: narrow at the back (y 22), wide at the front (y 34)
        if (22.0..34.0).contains(&y) {
            let f = (y - 22.0) / 12.0;
            let (l, r) = (20.0 - 8.0 * f, 44.0 + 8.0 * f);
            if (l..r).contains(&x) {
                // a streak of light across it, and the cast's soft edge
                let streak = (-(((x - 0.6 * y) - 8.0) / 4.0).powi(2)).exp();
                let rim = if x - l < 1.5 || r - x < 1.5 || y < 23.5 { -0.12 } else { 0.0 };
                let t = 0.68 + 0.25 * streak + rim + if sparkle > 0.96 { 0.15 } else { 0.0 };
                return Px { pal: 0, t, h: 0.8, rough: 0.3, alpha: 1.0 };
            }
        }
        // front face: as wide as the top's front at its top, a little wider below
        if (34.0..46.0).contains(&y) {
            let f = (y - 34.0) / 12.0;
            let (l, r) = (12.0 - 2.0 * f, 52.0 + 2.0 * f);
            if (l..r).contains(&x) {
                let t = 0.42 - 0.12 * f + if y < 35.5 { 0.12 } else { 0.0 } + if sparkle > 0.97 { 0.12 } else { 0.0 };
                return Px { pal: 0, t, h: 0.55, rough: 0.35, alpha: 1.0 };
            }
        }
        clear()
    });
    outline(&mut a, 1);
    a.finish(1.2, 0.3)
}

/// A steel striker, a C of dark iron, and a chip of flint beside it.
fn flint_and_steel() -> Tex {
    let s = seed_of("flint_and_steel");
    let mut a = Art::new("flint_and_steel", &[IRON_BAR_DARK, FLINT, OUTLINE]);
    a.fill(|x, y, _| {
        let p = Vec2::new(x + 0.5, y + 0.5);
        // the striker: a ring round (38, 30), open on its left
        let d = p.distance(Vec2::new(38.0, 30.0));
        let open = p.x < 32.0 && (p.y - 30.0).abs() < 6.0;
        if (11.0..16.5).contains(&d) && !open {
            let lit = -((p.x - 38.0) + (p.y - 30.0)) / 30.0;
            let t = 0.45 + 0.35 * lit + if (d - 13.0).abs() < 0.8 { 0.15 } else { 0.0 };
            return Px { pal: 0, t, h: 0.7, rough: 0.35, alpha: 1.0 };
        }
        // the flint: a chipped wedge at the bottom left
        let q = p - Vec2::new(19.0, 47.0);
        let wedge = q.x.abs() * 0.8 + q.y.abs() * 1.1 + 2.0 * (hash((p.x / 3.0) as i32, (p.y / 3.0) as i32, s) - 0.5);
        if wedge < 10.0 {
            let facet = if hash((p.x / 4.0) as i32, (p.y / 5.0) as i32, s ^ 3) > 0.5 { 0.18 } else { -0.05 };
            let t = 0.4 + facet - q.y / 30.0 + if hash(p.x as i32, p.y as i32, s ^ 9) > 0.95 { 0.3 } else { 0.0 };
            return Px { pal: 1, t, h: 0.6, rough: 0.25, alpha: 1.0 };
        }
        clear()
    });
    outline(&mut a, 2);
    a.finish(1.2, 0.4)
}

/// The iron of a striker: darker than an ingot's.
const IRON_BAR_DARK: &[u32] = &[0x24262b, 0x363940, 0x4c5058, 0x666b74, 0x868b94, 0xaab0b8];

/// A round loaf with three cuts across its top.
fn bread() -> Tex {
    let s = seed_of("bread");
    let mut a = Art::new("bread", &[CRUST, CRUMB, OUTLINE]);
    a.fill(|x, y, _| {
        let (dx, dy) = ((x + 0.5 - 32.0) / 22.0, (y + 0.5 - 36.0) / 14.0);
        let d = dx * dx + dy * dy;
        // a flat bottom: the dome stops at y 44
        if d > 1.0 || y > 44.0 {
            return clear();
        }
        let dome = (1.0 - d).sqrt();
        let lit = -(dx + dy * 1.5) * 0.25;
        // the cuts, slanting, showing crumb
        let cut = ((x - 0.5 * y + 4.0).rem_euclid(13.0) - 6.5).abs();
        if cut < 1.2 && dy < 0.2 && d < 0.75 {
            return Px { pal: 1, t: 0.4 + 0.4 * dome, h: 0.4, rough: 0.9, alpha: 1.0 };
        }
        let speck = if hash(x as i32, y as i32, s) > 0.9 { 0.08 } else { 0.0 };
        Px { pal: 0, t: 0.25 + 0.45 * dome + lit + speck, h: 0.3 + 0.6 * dome, rough: 0.85, alpha: 1.0 }
    });
    outline(&mut a, 2);
    a.finish(1.5, 0.5)
}

/// Two carrots roasted dark at the edges, with their tops gone limp.
fn roasted_carrots() -> Tex {
    let s = seed_of("roasted_carrots");
    let mut a = Art::new("roasted_carrots", &[ROAST, TOPS, OUTLINE]);
    // each: its top end, which way it points, length and half width at the top
    let carrots = [(Vec2::new(18.0, 18.0), Vec2::new(0.62, 0.78), 38.0, 6.0), (Vec2::new(30.0, 14.0), Vec2::new(0.45, 0.89), 40.0, 6.5)];
    a.fill(|x, y, _| {
        let p = Vec2::new(x + 0.5, y + 0.5);
        for &(top, dir, len, half) in carrots.iter().rev() {
            let side = Vec2::new(-dir.y, dir.x);
            let (u, v) = ((p - top).dot(dir), (p - top).dot(side));
            let w = half * (1.0 - u / len).max(0.0).sqrt();
            if (0.0..len).contains(&u) && v.abs() < w {
                let round = 1.0 - (v / w).abs();
                let ring = if ((u * 0.9).sin()).abs() < 0.15 { -0.15 } else { 0.0 };
                let char_ = if hash((p.x / 3.0) as i32, (p.y / 3.0) as i32, s) > 0.78 { -0.3 } else { 0.0 };
                return Px { pal: 0, t: 0.35 + 0.45 * round + ring + char_, h: 0.4 + 0.4 * round, rough: 0.7, alpha: 1.0 };
            }
            // limp tops, curled over
            let q = p - (top - dir * 3.0);
            if q.length() < 4.5 && hash(p.x as i32, p.y as i32, s ^ 5) > 0.25 {
                return Px { pal: 1, t: 0.3 + 0.4 * hash(p.x as i32, p.y as i32, s ^ 6), h: 0.5, rough: 0.9, alpha: 1.0 };
            }
        }
        clear()
    });
    outline(&mut a, 2);
    a.finish(1.4, 0.5)
}

pub(super) fn tiles() -> Vec<(&'static str, Tex)> {
    vec![
        ("charcoal", chunk("charcoal", CHARCOAL, false)),
        ("flint", chunk("flint", FLINT, true)),
        ("copper_ingot", ingot("copper_ingot", COPPER_BAR)),
        ("iron_ingot", ingot("iron_ingot", IRON_BAR)),
        ("gold_ingot", ingot("gold_ingot", GOLD_BAR)),
        ("moonsilver_ingot", ingot("moonsilver_ingot", MOONSILVER_BAR)),
        ("flint_and_steel", flint_and_steel()),
        ("bread", bread()),
        ("roasted_carrots", roasted_carrots()),
    ]
}
