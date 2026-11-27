//! Textures for the blocks made of boxes (`blocks::Cuboid`) and the blocks
//! that go with them. A model's boxes show the parts of its tiles they
//! cover, so each side tile is painted as a picture of the model from the
//! side, laid out on the same sixteenths as its boxes (four pixels each),
//! with nothing where no box reaches; that picture is its icon too.

use super::*;

const IRON: &[u32] = &[0x16171a, 0x222428, 0x30333a, 0x40444b, 0x53585f, 0x6a6f76, 0x858a90];
const BRASS: &[u32] = &[0x3d2a0f, 0x5a3f16, 0x7c5a1f, 0x9f772b, 0xc0953b, 0xd9b257, 0xeccd7d];
// red enough that the glow map (red plus green over 300) leaves it out
const WAX: &[u32] = &[0x3e1010, 0x581715, 0x721d1a, 0x8a241f, 0xa02d25, 0xb43a2d, 0xc24a37];
const FLAME: &[u32] = &[0x9c2a0c, 0xd44a12, 0xf3781e, 0xffa232, 0xffc95a, 0xffe7a0, 0xfff8e0];
const COALS: &[u32] = &[0x150b09, 0x2a120c, 0x4d1a0e, 0x8a2a10, 0xc94a16, 0xf07d22, 0xffb244, 0xffe08a];
const CLAY: &[u32] = &[0x4a2416, 0x64301c, 0x7e3d24, 0x974c2d, 0xae5e39, 0xc27449, 0xd38c5f];
const CLAY_PAINT: &[u32] = &[0x2b2a33, 0x3b3b47, 0xd9cbb0];
const SANDSTONE: &[u32] = &[0x7d6440, 0x947a4f, 0xa98e5f, 0xbca26f, 0xcdb481, 0xdbc595, 0xe7d6ab];
const MARBLE: &[u32] = &[0x5d5c58, 0x75736e, 0x8e8c86, 0xa7a49d, 0xbdbab2, 0xd1cec6, 0xe3e0d9];
const STRAW_GOLD: &[u32] = &[0x5e4310, 0x7d5a16, 0x9c731e, 0xb98d2a, 0xd2a73c, 0xe4c05a, 0xf0d683];
const STEM: &[u32] = &[0x3c4a1a, 0x52611f, 0x6a7926, 0x84902f, 0x9ea43c];
const CARROT: &[u32] = &[0x6e2a08, 0x96390c, 0xbc4f12, 0xdc6a1c, 0xf08a32, 0xf8a856];
const BOOKS: [&[u32]; 5] = [
    &[0x3a0f0f, 0x561716, 0x72211d, 0x8c2e25, 0xa63f31],
    &[0x111a33, 0x1a2747, 0x25365e, 0x334a78, 0x466191],
    &[0x13301c, 0x1c4327, 0x275733, 0x346c40, 0x45834f],
    &[0x3a2512, 0x52351a, 0x6b4723, 0x845a2e, 0x9c6e3b],
    &[0x6b5d43, 0x85765a, 0x9e8f71, 0xb7a889, 0xcdbfa1],
];

/// Nothing: a texel no box shows (but the icon does).
fn clear() -> Px {
    Px { pal: 0, t: 0.0, h: 0.0, rough: 1.0, alpha: 0.0 }
}

/// Whether `(x, y)` is inside the box `x0..x1` by `y0..y1`, in pixels.
fn inside(x: f32, y: f32, x0: i32, x1: i32, y0: i32, y1: i32) -> bool {
    let (x, y) = (x as i32, y as i32);
    x >= x0 && x < x1 && y >= y0 && y < y1
}

/// Wood running along `x` (boards across) or down (`!across`), in its own shade.
fn wood(x: f32, y: f32, across: bool, s: u32, shade: f32) -> Px {
    let (along, over) = if across { (x, y) } else { (y, x) };
    let warp = 2.0 * fbm(x, y, 4, 2, s ^ 6);
    let grain = ((over + warp) * 1.3 + 3.0 * hash(0, (along / 21.0) as i32, s)).sin();
    let t = 0.5 + shade + 0.12 * grain + 0.1 * (vn(x, y, 16, 4, s ^ 8) - 0.5);
    px(0, t, 0.7 + 0.05 * grain, 0.72 + 0.08 * grain)
}

/// Staves bound with two iron hoops, rounder toward the edges.
fn barrel_side() -> Tex {
    let s = seed_of("barrel_side");
    let mut a = Art::new("barrel_side", &[PLANK, IRON]);
    a.fill(|x, y, _| {
        if !inside(x, y, 4, 60, 0, 64) {
            return clear();
        }
        let xi = x as i32;
        let round = ((x - 31.5) / 28.0).powi(2);
        let stave = (xi - 4) / 7;
        if (xi - 4) % 7 == 0 {
            return px(0, 0.08, 0.1, 1.0);
        }
        let hoop = |y0: i32| y as i32 >= y0 && (y as i32) < y0 + 5;
        if hoop(8) || hoop(51) {
            let edge = y as i32 == 8 || y as i32 == 51;
            let rivet = (xi - 6) % 10 == 0 && (y as i32 == 10 || y as i32 == 53);
            let t = 0.55 - 0.4 * round + if edge { 0.2 } else { 0.0 } + if rivet { 0.35 } else { 0.0 };
            return px(1, t, 0.9, 0.45);
        }
        let mut p = wood(x, y, false, s ^ stave as u32, 0.08 * (hash(stave, 1, s) - 0.5) - 0.35 * round);
        if y < 2.0 || y > 61.0 {
            p.t -= 0.2;
        }
        p
    });
    a.finish(2.5, 0.6)
}

/// The barrel's lid: boards across in an iron rim.
fn barrel_top() -> Tex {
    let s = seed_of("barrel_top");
    let mut a = Art::new("barrel_top", &[PLANK, IRON]);
    a.fill(|x, y, _| {
        // the lid's outline is the two boxes' tops: 8 to 56 one way, 4 to 60 the other
        let (xi, yi) = (x as i32, y as i32);
        let d = |v: i32| (v - 4).min(59 - v);
        let edge = d(xi).min(d(yi)).min((d(xi) + d(yi)) / 2 + 2);
        if edge < 0 {
            return clear();
        }
        if edge < 3 {
            return px(1, 0.45 + if edge == 0 { 0.2 } else { 0.0 }, 0.9, 0.45);
        }
        let board = (yi - 4) / 9;
        if (yi - 4) % 9 == 0 {
            return px(0, 0.1, 0.1, 1.0);
        }
        if Vec2::new(x - 40.0, y - 26.0).length() < 3.0 {
            return px(0, 0.2, 0.3, 0.9);
        }
        wood(x, y, true, s ^ board as u32, 0.06 * (hash(board, 2, s) - 0.5) + 0.05)
    });
    a.finish(2.5, 0.6)
}

/// A slatted crate: a frame round the edge, a brace corner to corner, nails.
fn crate_tex() -> Tex {
    let s = seed_of("crate");
    let mut a = Art::new("crate", &[PLANK, NAIL]);
    a.fill(|x, y, _| {
        let (xi, yi) = (x as i32, y as i32);
        let frame = xi < 7 || xi > 56 || yi < 7 || yi > 56;
        let brace = (xi - yi).abs() < 5 && !frame;
        let nail = [(3, 3), (60, 3), (3, 60), (60, 60), (3, 31), (60, 31)].iter().any(|&(nx, ny)| xi == nx && yi == ny);
        if nail {
            return px(1, 0.7, 1.0, 0.4);
        }
        if frame {
            let edge = xi == 0 || yi == 0 || xi == 63 || yi == 63 || xi == 7 || yi == 7 || xi == 56 || yi == 56;
            let mut p = wood(x, y, yi < 7 || yi > 56, s, 0.12);
            if edge {
                p.t -= 0.3;
                p.h = 0.2;
            }
            return p;
        }
        if brace {
            let edge = (xi - yi).abs() == 4;
            let mut p = wood(x + y, x - y, true, s ^ 3, 0.08);
            if edge {
                p.t -= 0.3;
                p.h = 0.3;
            }
            return p;
        }
        // the slats behind, in shadow
        let slat = (xi - 7) / 10;
        let mut p = wood(x, y, false, s ^ slat as u32, -0.18 + 0.06 * hash(slat, 0, s));
        p.h = 0.35;
        if (xi - 7) % 10 == 0 {
            p.t = 0.02;
            p.h = 0.0;
        }
        p
    });
    a.finish(2.5, 0.7)
}

/// Two shelves of books, every spine its own colour and height.
fn bookshelf() -> Tex {
    let s = seed_of("bookshelf");
    let mut a = Art::new("bookshelf", &[PLANK, BOOKS[0], BOOKS[1], BOOKS[2], BOOKS[3], BOOKS[4]]);
    a.fill(|x, y, _| wood(x, y, true, s, 0.0));
    for (y0, y1) in [(4, 30), (34, 60)] {
        let mut x = 3;
        while x < 61 {
            let w = 3 + (a.r.next() * 4.0) as i32;
            let w = w.min(61 - x);
            let top = y0 + 1 + (a.r.next() * 7.0) as i32;
            let pal = 1 + (a.r.next() * 5.0) as usize;
            let lean = a.r.next() < 0.08;
            for yy in y0..y1 {
                for xx in x..x + w {
                    let p = a.at(xx, yy);
                    if yy < top || (lean && xx == x + w - 1 && yy < top + 3) {
                        *p = px(0, 0.02, 0.0, 1.0);
                        continue;
                    }
                    let side = if xx == x { -0.25 } else if xx == x + w - 1 { -0.15 } else if xx == x + 1 { 0.12 } else { 0.0 };
                    let band = yy == top + 3 || yy == y1 - 4;
                    *p = Px { pal, t: 0.55 + side + if band { 0.3 } else { 0.0 }, h: 0.6, rough: 0.8, alpha: 1.0 };
                }
            }
            x += w + if a.r.next() < 0.12 { 2 } else { 0 };
        }
    }
    // the shelves and the case's sides over them
    for y in 0..N {
        for x in 0..N {
            let shelf = y < 4 || (30..34).contains(&y) || y >= 60 || x < 3 || x >= 61;
            if shelf {
                let mut p = wood(x as f32, y as f32, y < 4 || (30..34).contains(&y) || y >= 60, s ^ 9, 0.1);
                if y == 3 || y == 33 || y == 63 || x == 2 || x == 61 {
                    p.t -= 0.3;
                }
                p.h = 0.9;
                *a.at(x, y) = p;
            }
        }
    }
    a.finish(2.5, 0.5)
}

/// A red candle on a brass dish, with its flame (which glows on its own tile).
fn candle() -> Tex {
    // the wax's picture runs from the dish up: boxes 1 to 7 are rows 36 to 60
    let mut a = Art::new("candle", &[WAX, BRASS, FLAME]);
    a.fill(|x, y, _| {
        let (xi, yi) = (x as i32, y as i32);
        if inside(x, y, 28, 36, 36, 60) {
            let t = 0.62 - 0.08 * (xi - 28) as f32 + if yi < 38 { 0.15 } else { 0.0 } + if xi == 33 && yi < 45 { 0.2 } else { 0.0 };
            return px(0, t, 0.7, 0.5);
        }
        if inside(x, y, 24, 40, 60, 64) {
            return px(1, 0.7 - 0.12 * (yi - 60) as f32, 0.6, 0.35);
        }
        if let Some(p) = flame_px(x, y) {
            return p;
        }
        clear()
    });
    a.finish(1.5, 0.3)
}

/// A flame in the box 7 to 9 across and 7 to 11 up (pixels 28 to 36 and
/// rows 20 to 36): a wick, a white core and orange round it.
fn flame_px(x: f32, y: f32) -> Option<Px> {
    let (cx, top, bottom) = (31.5, 20.0, 35.5);
    if !(top..=bottom).contains(&y) {
        return None;
    }
    let f = (y - top) / (bottom - top);
    // narrow at the tip, widest two thirds down, rounded at the base
    let half = 0.6 + 3.6 * (f * 1.5).min(1.0).sqrt() * (1.0 - smooth(0.75, 1.0, f) * 0.45);
    let d = (x - cx).abs();
    if d > half {
        return None;
    }
    if f > 0.82 && d < 0.6 {
        return Some(Px { pal: 0, t: 0.0, h: 0.5, rough: 1.0, alpha: 1.0 });
    }
    let core = 1.0 - d / half;
    let t = 0.15 + 0.55 * core * smooth(0.0, 0.5, f) + 0.3 * smooth(0.25, 0.75, f) * core;
    Some(Px { pal: 2, t, h: 0.5, rough: 1.0, alpha: 1.0 })
}

fn flame() -> Tex {
    let mut a = Art::new("flame", &[FLAME]);
    a.fill(|x, y, _| {
        let mut p = flame_px(x, y).unwrap_or_else(clear);
        p.pal = 0;
        p
    });
    let mut t = a.finish(0.0, 0.0);
    t.glow = true;
    t
}

fn candle_dish() -> Tex {
    let mut a = Art::new("candle_dish", &[BRASS]);
    a.fill(|x, y, _| {
        let d = Vec2::new(x - 31.5, y - 31.5).length();
        px(0, 0.6 - 0.02 * d + if (d - 6.0).abs() < 0.8 { 0.25 } else { 0.0 }, 0.6, 0.35)
    });
    a.finish(1.5, 0.5)
}

/// Lumps of coal glowing at their hearts.
fn coals() -> Tex {
    let s = seed_of("coals");
    let mut a = Art::new("coals", &[COALS]);
    a.fill(|x, y, _| {
        let c = cell(x, y, 6, 6, 0.9, s);
        let heat = c.rand(1) * smooth(0.5, 4.0, c.edge) + 0.25 * fbm(x, y, 8, 2, s ^ 3);
        if c.edge < 1.0 {
            return px(0, 0.05, 0.0, 1.0);
        }
        px(0, 0.1 + 0.95 * heat, 0.3 + 0.5 * smooth(1.0, 4.0, c.edge), 0.9)
    });
    let mut t = a.finish(1.5, 0.4);
    t.glow = true;
    t
}

/// An iron fire bowl on four legs, hammered and riveted.
fn brazier() -> Tex {
    let s = seed_of("brazier");
    let mut a = Art::new("brazier", &[IRON]);
    a.fill(|x, y, _| {
        let (xi, yi) = (x as i32, y as i32);
        let ham = fbm(x, y, 16, 2, s);
        if inside(x, y, 8, 56, 20, 40) {
            let round = ((x - 31.5) / 24.0).powi(2);
            let rivet = (xi - 12) % 8 == 0 && (yi == 23 || yi == 36);
            let t = 0.55 - 0.35 * round + 0.15 * (ham - 0.5) + if yi < 22 { 0.25 } else { 0.0 } + if yi > 37 { -0.15 } else { 0.0 } + if rivet { 0.3 } else { 0.0 };
            return px(0, t, 0.5 + 0.3 * ham + if rivet { 0.4 } else { 0.0 }, 0.5);
        }
        if inside(x, y, 12, 20, 40, 64) || inside(x, y, 44, 52, 40, 64) {
            let left = xi == 12 || xi == 44;
            return px(0, 0.4 + if left { 0.2 } else { 0.0 } + 0.1 * (ham - 0.5), 0.6, 0.5);
        }
        clear()
    });
    a.finish(2.0, 0.6)
}

fn brazier_top() -> Tex {
    let s = seed_of("brazier_top");
    let mut a = Art::new("brazier_top", &[IRON]);
    a.fill(|x, y, _| {
        let d = ((x - 31.5).abs()).max((y - 31.5).abs());
        px(0, 0.5 + 0.15 * (fbm(x, y, 16, 2, s) - 0.5) + if d > 22.0 { 0.15 } else { 0.0 }, 0.6, 0.5)
    });
    a.finish(2.0, 0.6)
}

/// A post sharpened to a point: bark below, cut wood where the knife went.
fn stake() -> Tex {
    let s = seed_of("stake");
    let mut a = Art::new("stake", &[BARK, RING]);
    a.fill(|x, y, _| {
        let (xi, yi) = (x as i32, y as i32);
        let bark = inside(x, y, 16, 48, 16, 64);
        let point = inside(x, y, 20, 44, 8, 16) || inside(x, y, 24, 40, 4, 8) || inside(x, y, 28, 36, 0, 4);
        if bark {
            let furrow = (std::f32::consts::TAU * (x + 3.0 * fbm(x, y, 2, 2, s)) / 6.0).sin();
            let round = ((x - 31.5) / 16.0).powi(2);
            let t = 0.45 + 0.2 * furrow - 0.3 * round + 0.12 * (vn(x, y, 8, 8, s ^ 1) - 0.5);
            return px(0, t, 0.5 + 0.3 * furrow, 0.95);
        }
        if point {
            // facets cut on a slant, lighter where the cut faces the light
            let facet = ((xi + yi * 2) / 5) % 2;
            let t = 0.5 + 0.18 * facet as f32 + 0.1 * (vn(x, y, 16, 16, s ^ 2) - 0.5);
            return px(1, t, 0.6 + 0.2 * facet as f32, 0.8);
        }
        clear()
    });
    a.finish(2.5, 0.8)
}

fn stake_top() -> Tex {
    let s = seed_of("stake_top");
    let mut a = Art::new("stake_top", &[RING]);
    a.fill(|x, y, _| {
        let d = Vec2::new(x - 31.5, y - 31.5).length() + 2.0 * fbm(x, y, 4, 2, s);
        let ring = (d * 1.2).sin();
        px(0, 0.55 + 0.12 * ring, 0.6 + 0.05 * ring, 0.8)
    });
    a.finish(1.5, 0.4)
}

/// A clay pot: a painted band round its belly, a lip at the top.
fn pot() -> Tex {
    let s = seed_of("pot");
    let mut a = Art::new("pot", &[CLAY, CLAY_PAINT]);
    a.fill(|x, y, _| {
        let (xi, yi) = (x as i32, y as i32);
        let m = fbm(x, y, 8, 2, s);
        if inside(x, y, 12, 52, 24, 32) {
            let round = ((x - 31.5) / 20.0).powi(2);
            return px(0, 0.7 - 0.3 * round + if yi == 24 { 0.15 } else if yi == 31 { -0.25 } else { 0.0 } + 0.1 * (m - 0.5), 0.8, 0.7);
        }
        if inside(x, y, 16, 48, 32, 64) {
            let round = ((x - 31.5) / 16.0).powi(2);
            if (44..48).contains(&yi) {
                let zig = ((xi + if yi < 46 { 0 } else { 2 }) / 2) % 2 == 0;
                return px(1, if zig { 1.0 } else { 0.4 }, 0.5, 0.6);
            }
            return px(0, 0.55 - 0.35 * round - 0.1 * ((yi - 32) as f32 / 32.0) + 0.1 * (m - 0.5), 0.55, 0.75);
        }
        clear()
    });
    a.finish(1.5, 0.4)
}

fn pot_top() -> Tex {
    let s = seed_of("pot_top");
    let mut a = Art::new("pot_top", &[CLAY, DIRT]);
    a.fill(|x, y, _| {
        let d = ((x - 31.5).abs()).max((y - 31.5).abs());
        if d > 16.0 {
            return px(0, 0.7 - 0.02 * (d - 16.0) * 4.0, 0.8, 0.7);
        }
        px(1, 0.3 + 0.4 * fbm(x, y, 16, 2, s), 0.3, 0.95)
    });
    a.finish(2.0, 0.6)
}

/// A table seen from the side: a thick top and two legs.
fn table() -> Tex {
    let s = seed_of("table");
    let mut a = Art::new("table", &[PLANK]);
    a.fill(|x, y, _| {
        let (xi, yi) = (x as i32, y as i32);
        if yi < 12 {
            let mut p = wood(x, y, true, s, 0.1);
            if yi == 0 {
                p.t += 0.2;
            } else if yi == 11 {
                p.t -= 0.3;
            }
            return p;
        }
        if inside(x, y, 4, 12, 12, 64) || inside(x, y, 52, 60, 12, 64) {
            let mut p = wood(x, y, false, s ^ 1, -0.02);
            if xi == 4 || xi == 52 {
                p.t += 0.15;
            } else if xi == 11 || xi == 59 {
                p.t -= 0.2;
            }
            if yi < 15 {
                p.t -= 0.2;
            }
            return p;
        }
        clear()
    });
    a.finish(2.0, 0.6)
}

fn stool() -> Tex {
    let s = seed_of("stool");
    let mut a = Art::new("stool", &[PLANK]);
    a.fill(|x, y, _| {
        let (xi, yi) = (x as i32, y as i32);
        if inside(x, y, 12, 52, 28, 36) {
            let mut p = wood(x, y, true, s, 0.12);
            if yi == 28 {
                p.t += 0.2;
            } else if yi == 35 {
                p.t -= 0.3;
            }
            return p;
        }
        if inside(x, y, 16, 24, 36, 64) || inside(x, y, 40, 48, 36, 64) {
            let mut p = wood(x, y, false, s ^ 1, -0.02);
            if xi == 16 || xi == 40 {
                p.t += 0.15;
            }
            return p;
        }
        clear()
    });
    a.finish(2.0, 0.6)
}

/// A post with two rails either side.
fn fence() -> Tex {
    let s = seed_of("fence");
    let mut a = Art::new("fence", &[PLANK]);
    a.fill(|x, y, _| {
        let (xi, yi) = (x as i32, y as i32);
        if inside(x, y, 24, 40, 0, 64) {
            let mut p = wood(x, y, false, s, -0.05);
            if xi == 24 {
                p.t += 0.15;
            } else if xi == 39 {
                p.t -= 0.2;
            }
            if yi < 2 {
                p.t += 0.1;
            }
            return p;
        }
        if inside(x, y, 0, 64, 8, 20) || inside(x, y, 0, 64, 28, 40) {
            let mut p = wood(x, y, true, s ^ 2, 0.08);
            if yi == 8 || yi == 28 {
                p.t += 0.18;
            } else if yi == 19 || yi == 39 {
                p.t -= 0.25;
            }
            return p;
        }
        clear()
    });
    a.finish(2.0, 0.6)
}

/// Sandstone in thick layers, worn soft along each one.
fn sandstone() -> Tex {
    let s = seed_of("sandstone");
    let mut a = Art::new("sandstone", &[SANDSTONE]);
    a.fill(|x, y, _| {
        let wave = 2.0 * fbm(x, y, 2, 2, s);
        let band = ((y + wave) / 8.0).floor();
        let within = (y + wave).rem_euclid(8.0);
        let m = fbm(x, y, 8, 2, s ^ 1);
        let mut t = 0.55 + 0.25 * (hash(band as i32, 0, s) - 0.5) + 0.2 * (m - 0.5);
        let seam = fbm(x, y, 4, 2, s ^ 7) > 0.45;
        if within < 1.0 && seam {
            t -= 0.16;
        } else if within < 2.0 {
            t += 0.08;
        }
        if hash(x as i32, y as i32, s ^ 5) < 0.05 {
            t -= 0.15;
        }
        px(0, t, 0.5 + 0.3 * smooth(0.0, 3.0, within) + 0.1 * m, 0.92)
    });
    a.finish(2.0, 0.6)
}

fn sandstone_top() -> Tex {
    let s = seed_of("sandstone_top");
    let mut a = Art::new("sandstone_top", &[SANDSTONE]);
    a.fill(|x, y, _| {
        let m = fbm(x, y, 4, 3, s);
        let speck = hash(x as i32, y as i32, s ^ 2);
        px(0, 0.62 + 0.25 * (m - 0.5) + if speck < 0.04 { -0.18 } else if speck > 0.97 { 0.12 } else { 0.0 }, 0.5 + 0.2 * m, 0.92)
    });
    a.finish(1.5, 0.4)
}

fn sandstone_bricks() -> Tex {
    let s = seed_of("sandstone_bricks");
    let mut a = Art::new("sandstone_bricks", &[SANDSTONE]);
    a.fill(|x, y, _| {
        let (xi, yi) = (x as i32, y as i32);
        let row = yi / 16;
        let bx = (xi + (row % 2) * 16).rem_euclid(32);
        let by = yi % 16;
        let brick = hash((xi + (row % 2) * 16).div_euclid(32).rem_euclid(2), row, s);
        let m = fbm(x, y, 8, 2, s ^ 1);
        if bx == 0 || by == 0 {
            return px(0, 0.12, 0.0, 1.0);
        }
        let mut t = 0.55 + 0.2 * (brick - 0.5) + 0.2 * (m - 0.5);
        if bx == 1 || by == 1 {
            t += 0.15;
        } else if bx == 31 || by == 15 {
            t -= 0.15;
        }
        px(0, t, 0.7 + 0.1 * m, 0.9)
    });
    a.finish(2.0, 0.7)
}

/// Fluted stone: shallow grooves running up, each with a lit edge.
fn flutes(x: f32, y: f32, s: u32) -> Px {
    let u = (x - 12.0).rem_euclid(5.0);
    let m = fbm(x, y, 8, 2, s);
    let groove = match u as i32 {
        0 => 0.18,
        1 => -0.05,
        2 => -0.15,
        3 => -0.08,
        _ => 0.05,
    };
    px(0, 0.6 + groove + 0.12 * (m - 0.5), 0.6 + groove * 1.5, 0.6)
}

/// A column from the side: capital, fluted shaft, base.
fn column() -> Tex {
    let s = seed_of("column");
    let mut a = Art::new("column", &[MARBLE]);
    a.fill(|x, y, _| {
        let yi = y as i32;
        let m = fbm(x, y, 8, 2, s);
        let moulding = |r: i32| px(0, 0.65 + [0.2, 0.05, -0.15, 0.1][(r % 4) as usize] + 0.1 * (m - 0.5), 0.7, 0.6);
        if yi < 12 {
            return if inside(x, y, 4, 60, 0, 12) { moulding(yi) } else { clear() };
        }
        if yi >= 52 {
            return if inside(x, y, 4, 60, 52, 64) { moulding(63 - yi) } else { clear() };
        }
        if inside(x, y, 12, 52, 12, 52) {
            return flutes(x, y, s);
        }
        clear()
    });
    a.finish(2.0, 0.6)
}

fn column_shaft() -> Tex {
    let s = seed_of("column");
    let mut a = Art::new("column_shaft", &[MARBLE]);
    a.fill(|x, y, _| flutes(x, y, s));
    a.finish(2.0, 0.6)
}

fn column_top() -> Tex {
    let s = seed_of("column_top");
    let mut a = Art::new("column_top", &[MARBLE]);
    a.fill(|x, y, _| {
        let d = ((x - 31.5).abs()).max((y - 31.5).abs());
        let m = fbm(x, y, 8, 2, s);
        px(0, 0.62 + 0.12 * (m - 0.5) + if (d - 20.0).abs() < 1.0 { -0.15 } else { 0.0 }, 0.6, 0.6)
    });
    a.finish(1.5, 0.5)
}

/// Ploughed soil in ridges and furrows, dark where it's damp.
fn farmland() -> Tex {
    let s = seed_of("farmland");
    let mut a = Art::new("farmland", &[DIRT, PEBBLE]);
    a.fill(|x, y, _| {
        let v = (y + 1.5 * fbm(x, y, 4, 2, s)).rem_euclid(16.0);
        let ridge = (std::f32::consts::PI * v / 16.0).sin();
        let m = fbm(x, y, 8, 2, s ^ 1);
        if hash(x as i32, y as i32, s ^ 4) < 0.03 {
            return px(1, 0.5, 0.8, 0.9);
        }
        px(0, 0.18 + 0.5 * ridge + 0.2 * (m - 0.5), 0.2 + 0.7 * ridge, 0.75 + 0.2 * ridge)
    });
    a.finish(3.0, 1.0)
}

/// Ripe wheat: stalks with heavy heads, see-through between them.
fn wheat() -> Tex {
    let mut a = Art::new("wheat", &[STEM, STRAW_GOLD]);
    a.fill(|_, _, _| clear());
    for i in 0..15 {
        let x = 2.0 + i as f32 * 4.2 + a.r.next() * 2.0;
        let top = 3.0 + a.r.next() * 10.0;
        let lean = (a.r.next() - 0.5) * 6.0;
        let foot = Vec2::new(x, 64.0);
        let neck = Vec2::new(x + lean, top + 14.0);
        a.line(foot, neck, |f, q| *q = Px { pal: 0, t: 0.35 + 0.45 * f, h: 0.5, rough: 0.9, alpha: 1.0 });
        a.line(foot + Vec2::new(1.0, 0.0), neck.lerp(foot, 0.5) + Vec2::new(1.0, 0.0), |f, q| *q = Px { pal: 0, t: 0.2 + 0.3 * f, h: 0.4, rough: 0.9, alpha: 1.0 });
        // a leaf off the stalk
        let mid = foot.lerp(neck, 0.4 + 0.2 * a.r.next());
        let side = if a.r.next() < 0.5 { -1.0 } else { 1.0 };
        a.line(mid, mid + Vec2::new(side * 5.0, -7.0), |f, q| *q = Px { pal: 0, t: 0.4 + 0.3 * f, h: 0.5, rough: 0.9, alpha: 1.0 });
        // the head: grains either side of the tip, lighter toward the light
        for k in 0..7 {
            let at = neck + Vec2::new(lean * 0.25 * k as f32 / 7.0, -(k as f32) * 2.0);
            for side in [-1.0f32, 1.0, 2.0] {
                let p = at + Vec2::new(side, -0.5);
                let q = a.at(p.x as i32, p.y as i32);
                *q = Px { pal: 1, t: 0.5 + 0.2 * side.clamp(0.0, 1.0) - 0.25 * (side - 1.0).max(0.0) - 0.03 * k as f32, h: 0.8, rough: 0.8, alpha: 1.0 };
            }
            let q = a.at(at.x as i32, at.y as i32);
            *q = Px { pal: 1, t: 0.85, h: 0.9, rough: 0.8, alpha: 1.0 };
        }
        // a whisker or two off the tip
        let tip = neck + Vec2::new(lean * 0.25, -14.0);
        a.line(tip, tip + Vec2::new(lean * 0.3 + 1.0, -4.0), |_, q| *q = Px { pal: 1, t: 0.95, h: 0.8, rough: 0.8, alpha: 1.0 });
    }
    a.finish(1.0, 0.2)
}

/// Feathery carrot tops, with the orange shoulders showing at the soil.
fn carrots() -> Tex {
    let mut a = Art::new("carrots", &[GRASS, CARROT]);
    a.fill(|_, _, _| clear());
    for i in 0..5 {
        let x = 6.0 + i as f32 * 12.5 + a.r.next() * 3.0;
        for yy in 54..64 {
            let w = if yy < 57 { 2 } else { 3 };
            for xx in -w..=w {
                let q = a.at(x as i32 + xx, yy);
                *q = Px { pal: 1, t: 0.6 - 0.1 * xx.abs() as f32 + if yy == 54 { 0.2 } else { 0.0 }, h: 0.7, rough: 0.8, alpha: 1.0 };
            }
        }
        for _ in 0..6 {
            let ang = -std::f32::consts::FRAC_PI_2 + (a.r.next() - 0.5) * 1.6;
            let len = 14.0 + a.r.next() * 22.0;
            let from = Vec2::new(x, 54.0);
            let to = from + Vec2::from_angle(ang) * len;
            a.line(from, to, |f, q| *q = Px { pal: 0, t: 0.3 + 0.4 * f, h: 0.6, rough: 0.9, alpha: 1.0 });
            // little leaflets along it
            for k in 1..6 {
                let p = from.lerp(to, k as f32 / 6.0);
                for d in [-1.0f32, 1.0] {
                    let q = a.at((p.x + d * 1.5) as i32, (p.y - 1.0) as i32);
                    *q = Px { pal: 0, t: 0.45 + 0.1 * d, h: 0.6, rough: 0.9, alpha: 1.0 };
                }
            }
        }
    }
    a.finish(1.0, 0.2)
}

/// Every tile for the blocks made of boxes and the blocks that go with them.
pub(super) fn tiles() -> Vec<(&'static str, Tex)> {
    vec![
        ("barrel_side", barrel_side()),
        ("barrel_top", barrel_top()),
        ("crate", crate_tex()),
        ("bookshelf", bookshelf()),
        ("candle", candle()),
        ("candle_dish", candle_dish()),
        ("flame", flame()),
        ("coals", coals()),
        ("brazier", brazier()),
        ("brazier_top", brazier_top()),
        ("stake", stake()),
        ("stake_top", stake_top()),
        ("pot", pot()),
        ("pot_top", pot_top()),
        ("table", table()),
        ("stool", stool()),
        ("fence", fence()),
        ("sandstone", sandstone()),
        ("sandstone_top", sandstone_top()),
        ("sandstone_bricks", sandstone_bricks()),
        ("column", column()),
        ("column_shaft", column_shaft()),
        ("column_top", column_top()),
        ("farmland", farmland()),
        ("wheat", wheat()),
        ("carrots", carrots()),
    ]
}
