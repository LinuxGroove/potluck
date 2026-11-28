//! Block textures in the stylised look Ken picked (64 px pixel art with
//! normal and roughness maps), painted from code. Each material is a few
//! shapes (plates of rock, cobbles, boards, bricks, straw) laid out on a
//! 64 px tile that wraps, coloured from a short palette so it stays crisp
//! pixel art, with a height for the normal map and a roughness per texel.
//!
//! Every pixel gets a palette, a place on it (`t`, 0 dark to 1 light), a
//! height and a roughness; `Art::finish` then lights the heights a little
//! from the top left into the colour, as painted pixel art does, and picks
//! each colour from its palette.

use bevy::math::Vec2;

use super::{hex, Tex, ART};

mod food;
mod ores;
mod props;
mod smelting;
mod tools;
mod travel;
mod wilds;
use crate::noise::{seed_of, Rng};

const N: i32 = ART as i32;

/// A number in `0.0..1.0` for a grid point and a seed.
fn hash(x: i32, y: i32, s: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x8da6_b343) ^ (y as u32).wrapping_mul(0xd816_3841) ^ s.wrapping_mul(0xcb1a_b31f);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^= h >> 15;
    (h & 0xff_ffff) as f32 / 16_777_216.0
}

/// Smooth noise that repeats across the tile, `cx` by `cy` cells.
fn vn(x: f32, y: f32, cx: i32, cy: i32, s: u32) -> f32 {
    let (fx, fy) = (x * cx as f32 / N as f32, y * cy as f32 / N as f32);
    let (ix, iy) = (fx.floor() as i32, fy.floor() as i32);
    let (tx, ty) = (fx - ix as f32, fy - iy as f32);
    let (sx, sy) = (tx * tx * (3.0 - 2.0 * tx), ty * ty * (3.0 - 2.0 * ty));
    let g = |a: i32, b: i32| hash(a.rem_euclid(cx), b.rem_euclid(cy), s);
    let top = g(ix, iy) + (g(ix + 1, iy) - g(ix, iy)) * sx;
    let bot = g(ix, iy + 1) + (g(ix + 1, iy + 1) - g(ix, iy + 1)) * sx;
    top + (bot - top) * sy
}

/// Noise in octaves, starting at `cells` across, about 0..1.
fn fbm(x: f32, y: f32, cells: i32, octaves: u32, s: u32) -> f32 {
    let (mut sum, mut amp, mut total, mut c) = (0.0, 1.0, 0.0, cells);
    for o in 0..octaves {
        sum += vn(x, y, c, c, s.wrapping_add(o * 101)) * amp;
        total += amp;
        amp *= 0.5;
        c *= 2;
    }
    sum / total
}

fn smooth(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The cell of a wrapping Voronoi pattern a point is in: `cx` by `cy` cells,
/// each with one point jittered by up to `jitter` of a cell.
struct Cell {
    /// The same for every pixel of a cell.
    id: u32,
    /// How far the point is from the cell's centre point, px.
    d: f32,
    /// How far it is from the cell's edge, px.
    edge: f32,
}

impl Cell {
    /// A number in `0.0..1.0` that's the same across the cell.
    fn rand(&self, k: u32) -> f32 {
        hash(self.id as i32, k as i32, 0x51ed)
    }
}

fn cell(x: f32, y: f32, cx: i32, cy: i32, jitter: f32, s: u32) -> Cell {
    let (w, h) = (N as f32 / cx as f32, N as f32 / cy as f32);
    let p = Vec2::new(x + 0.5, y + 0.5);
    let (gx, gy) = ((p.x / w).floor() as i32, (p.y / h).floor() as i32);
    let site = |i: i32, j: i32| {
        let (wi, wj) = (i.rem_euclid(cx), j.rem_euclid(cy));
        let o = Vec2::new(0.5 + (hash(wi, wj, s) - 0.5) * jitter, 0.5 + (hash(wi, wj, s ^ 0x9e37) - 0.5) * jitter);
        (Vec2::new((i as f32 + o.x) * w, (j as f32 + o.y) * h), (wj * cx + wi) as u32)
    };
    let mut near = (Vec2::ZERO, 0, f32::MAX);
    for j in gy - 2..=gy + 2 {
        for i in gx - 2..=gx + 2 {
            let (q, id) = site(i, j);
            let d = q.distance(p);
            if d < near.2 {
                near = (q, id, d);
            }
        }
    }
    let mut edge = f32::MAX;
    for j in gy - 2..=gy + 2 {
        for i in gx - 2..=gx + 2 {
            let (q, _) = site(i, j);
            if q.distance(near.0) < 1e-3 {
                continue;
            }
            let mid = (q + near.0) * 0.5;
            edge = edge.min((p - mid).dot((q - near.0).normalize()).abs());
        }
    }
    Cell { id: near.1.wrapping_add(s), d: near.2, edge }
}

/// One texel before colouring.
#[derive(Clone, Copy)]
struct Px {
    pal: usize,
    t: f32,
    h: f32,
    rough: f32,
    alpha: f32,
}

/// A 64 px tile being painted, in palette terms.
struct Art {
    pals: Vec<&'static [u32]>,
    px: Vec<Px>,
    r: Rng,
}

impl Art {
    fn new(name: &str, pals: &[&'static [u32]]) -> Self {
        Art { pals: pals.to_vec(), px: vec![Px { pal: 0, t: 0.5, h: 0.5, rough: 0.9, alpha: 1.0 }; ART * ART], r: Rng::new(seed_of(name)) }
    }

    fn at(&mut self, x: i32, y: i32) -> &mut Px {
        &mut self.px[(y.rem_euclid(N) * N + x.rem_euclid(N)) as usize]
    }

    fn fill(&mut self, mut f: impl FnMut(f32, f32, &mut Rng) -> Px) {
        for y in 0..N {
            for x in 0..N {
                let p = f(x as f32, y as f32, &mut self.r);
                *self.at(x, y) = p;
            }
        }
    }

    /// A line one texel wide from `a` to `b`, wrapping round the tile;
    /// `f` gets how far along it is (0 to 1) and the texel to change.
    fn line(&mut self, a: Vec2, b: Vec2, mut f: impl FnMut(f32, &mut Px)) {
        let steps = (a.distance(b) * 1.5).ceil().max(1.0) as usize;
        let mut last = (i32::MIN, i32::MIN);
        for i in 0..=steps {
            let t = i as f32 / steps as f32;
            let p = a.lerp(b, t);
            let q = (p.x.floor() as i32, p.y.floor() as i32);
            if q != last {
                f(t, self.at(q.0, q.1));
                last = q;
            }
        }
    }

    /// Colours the tile: each texel's place on its palette, lit from the top
    /// left by `emboss` times the slope of the heights, picked to the
    /// nearest of the palette's colours.
    fn finish(self, bump: f32, emboss: f32) -> Tex {
        let h = |x: i32, y: i32| self.px[(y.rem_euclid(N) * N + x.rem_euclid(N)) as usize].h;
        let mut rgba = Vec::with_capacity(ART * ART);
        let mut height = Vec::with_capacity(ART * ART);
        let mut rough = Vec::with_capacity(ART * ART);
        for y in 0..N {
            for x in 0..N {
                let p = self.px[(y * N + x) as usize];
                let light = (h(x - 1, y - 1) - h(x + 1, y + 1)) * emboss;
                let c = pick(self.pals[p.pal], p.t + light);
                rgba.push([c[0], c[1], c[2], p.alpha * 255.0]);
                height.push(p.h);
                rough.push(p.rough);
            }
        }
        Tex { size: ART, rgba, height, bump, glow: false, rough: Some(rough) }
    }
}

/// The palette colour nearest `t` (0 the first, 1 the last).
fn pick(pal: &[u32], t: f32) -> [f32; 3] {
    let i = (t.clamp(0.0, 1.0) * (pal.len() - 1) as f32).round() as usize;
    hex(pal[i])
}

// Palettes run dark to light, with shadows a little cooler and lights a
// little warmer, as painted pixel art does.
const STONE: &[u32] = &[0x34373f, 0x464a53, 0x5a5e67, 0x6f747b, 0x868a90, 0x9ea1a5, 0xb5b7b8];
const COBBLE: &[u32] = &[0x3a3c42, 0x4d5057, 0x62656b, 0x787b80, 0x8f9195, 0xa6a8a9];
const COBBLE_WARM: &[u32] = &[0x433b36, 0x564d46, 0x6c6158, 0x82766b, 0x988c7f, 0xaea294];
const COBBLE_MOSS: &[u32] = &[0x37402f, 0x48543b, 0x5b6849, 0x6f7c59, 0x84916b];
const MORTAR: &[u32] = &[0x1f1f22, 0x2a2a2d, 0x37363a, 0x46443f];
const DIRT: &[u32] = &[0x2f1f15, 0x41291a, 0x553620, 0x6a4428, 0x7f5533, 0x93673f];
const PEBBLE: &[u32] = &[0x5b524b, 0x726960, 0x8a8177, 0xa29a8f];
const GRASS: &[u32] = &[0x2a5320, 0x356627, 0x42792d, 0x528d34, 0x649f3b, 0x7ab247, 0x93c457];
const SAND: &[u32] = &[0xa48d5c, 0xb29c69, 0xbfaa78, 0xcbb888, 0xd6c598, 0xe1d3aa];
const GRAVEL_GREY: &[u32] = &[0x3c3d40, 0x55565a, 0x6e6f72, 0x88888a, 0xa2a1a0];
const GRAVEL_BROWN: &[u32] = &[0x45372c, 0x5d4b3d, 0x766150, 0x8f7864, 0xa78f79];
const GRAVEL_PALE: &[u32] = &[0x5c564d, 0x787166, 0x948c80, 0xafa79a, 0xc8c0b2];
const BARK: &[u32] = &[0x22180f, 0x312317, 0x412f1f, 0x523c28, 0x644a32, 0x775a3d];
const RING: &[u32] = &[0x6f5130, 0x86633b, 0x9c7647, 0xb08954, 0xc29b62, 0xd2ad73];
const BIRCH: &[u32] = &[0x9d978a, 0xb4aea1, 0xc8c3b7, 0xd9d5cb, 0xe8e5dd, 0xf4f2ec];
const BIRCH_MARK: &[u32] = &[0x1d1b19, 0x2e2b27, 0x45403a, 0x5f5850];
const PLANK: &[u32] = &[0x3b2614, 0x553820, 0x6b482a, 0x805833, 0x95693e, 0xa97a4a, 0xbb8b57];
const NAIL: &[u32] = &[0x2b2c30, 0x55575c, 0x8a8c90];
const BRICK: &[u32] = &[0x3e4142, 0x515454, 0x646766, 0x787a77, 0x8c8d89, 0xa09f9a];
const MOSS: &[u32] = &[0x2a4220, 0x385726, 0x486d2e, 0x5a8337, 0x6f9a42, 0x86b04f];
const SNOW: &[u32] = &[0x9fb2cb, 0xb5c6db, 0xcad8e8, 0xdce6f1, 0xebf1f8, 0xf8fbfe];
const HAY: &[u32] = &[0x6e4f12, 0x8a6519, 0xa67d22, 0xc0982f, 0xd5b143, 0xe5c75e, 0xf1db86];
const TWINE: &[u32] = &[0x3d1f14, 0x5a2e1c, 0x7a4026, 0x965433];
const BEDROCK: &[u32] = &[0x111214, 0x1b1d20, 0x26292d, 0x33363c, 0x42464d];
pub(super) const WOOL_RED: &[u32] = &[0x5e1518, 0x7a1f21, 0x962a2a, 0xae3934, 0xc24b41, 0xd36152];
pub(super) const WOOL_CREAM: &[u32] = &[0x9e917a, 0xb5a98f, 0xc9bea5, 0xdbd2bb, 0xeae3d0, 0xf5f1e4];
pub(super) const WOOL_BLUE: &[u32] = &[0x18284f, 0x203563, 0x29447b, 0x335693, 0x406aab, 0x5281c2];
pub(super) const WOOL_YELLOW: &[u32] = &[0x8a5e12, 0xa87619, 0xc28f22, 0xd7a82f, 0xe7bf45, 0xf2d466];

fn px(pal: usize, t: f32, h: f32, rough: f32) -> Px {
    Px { pal, t, h, rough, alpha: 1.0 }
}

/// Natural rock: layered, mottled, with cracks that run part way between
/// big plates and the odd chipped facet.
fn stone_px(x: f32, y: f32, s: u32) -> Px {
    let c = cell(x, y, 3, 3, 0.9, s);
    let chip = cell(x, y, 8, 8, 0.9, s ^ 0x55);
    let m = fbm(x, y, 4, 3, s ^ 7);
    let layer = (std::f32::consts::TAU * (y + 7.0 * fbm(x, y, 2, 2, s ^ 3)) / 16.0).sin();
    let mut t = 0.5 + 0.14 * (c.rand(1) - 0.5) + 0.4 * (m - 0.5) + 0.1 * layer;
    let mut h = 0.4 + 0.3 * m + 0.08 * layer;
    // a chipped facet: its own shade, raised a little so it catches the light
    if chip.rand(2) < 0.18 && chip.edge > 0.8 {
        t += 0.12 * (chip.rand(3) - 0.3);
        h += 0.15 * smooth(0.8, 3.0, chip.edge);
    }
    // cracks between the plates, only where the noise lets them through
    let open = fbm(x, y, 4, 2, s ^ 13);
    if c.edge < 1.0 && open > 0.42 {
        t = 0.08 + 0.12 * m;
        h = 0.05;
    } else if c.edge < 2.0 && open > 0.42 {
        t -= 0.08;
    }
    if hash(x as i32, y as i32, s ^ 99) < 0.03 {
        t += 0.16;
    }
    px(0, t, h, 0.8 + 0.15 * (1.0 - h))
}

fn stone() -> Tex {
    let s = seed_of("stone2");
    let mut a = Art::new("stone", &[STONE]);
    a.fill(|x, y, _| stone_px(x, y, s));
    a.finish(2.5, 1.0)
}

/// Rounded stones set in dark mortar, each stone its own shade.
fn cobble() -> Tex {
    let s = seed_of("cobble2");
    let mut a = Art::new("cobble", &[COBBLE, COBBLE_WARM, COBBLE_MOSS, MORTAR]);
    a.fill(|x, y, _| {
        let c = cell(x, y, 5, 5, 0.8, s);
        let m = fbm(x, y, 8, 2, s ^ 3);
        if c.edge < 1.3 {
            return px(3, 0.3 + 0.4 * m, 0.0, 1.0);
        }
        let dome = smooth(1.3, 6.0, c.edge).sqrt();
        let k = c.rand(3);
        let pal = if k < 0.62 { 0 } else if k < 0.9 { 1 } else { 2 };
        px(pal, 0.42 + 0.3 * (c.rand(4) - 0.5) + 0.3 * (m - 0.5) + 0.18 * dome, 0.25 + 0.75 * dome, 0.7 + 0.2 * (1.0 - dome))
    });
    a.finish(3.0, 1.6)
}

/// Soil: lumpy, with crevices between clods and a scatter of pebbles.
fn dirt_px(x: f32, y: f32, s: u32) -> Px {
    let clod = cell(x, y, 6, 6, 0.9, s);
    let m = fbm(x, y, 8, 3, s ^ 11);
    let mut t = 0.5 + 0.55 * (m - 0.5) + 0.12 * (clod.rand(1) - 0.5) - 0.14 * (1.0 - smooth(0.0, 1.4, clod.edge));
    let mut h = 0.3 + 0.2 * smooth(0.0, 3.0, clod.edge) + 0.4 * m;
    let mut pal = 0;
    let mut rough = 0.95;
    let peb = cell(x, y, 9, 9, 0.9, s ^ 0x77);
    let r = 1.0 + 1.6 * peb.rand(2);
    if peb.rand(3) < 0.22 && peb.d < r {
        pal = 1;
        let dome = 1.0 - peb.d / r;
        t = 0.35 + 0.5 * peb.rand(4) + 0.3 * dome;
        h = 0.75 + 0.25 * dome;
        rough = 0.7;
    }
    if hash(x as i32, y as i32, s ^ 5) < 0.04 {
        t -= 0.2;
    }
    px(pal, t, h, rough)
}

fn dirt() -> Tex {
    let s = seed_of("dirt2");
    let mut a = Art::new("dirt", &[DIRT, PEBBLE]);
    a.fill(|x, y, _| dirt_px(x, y, s));
    a.finish(2.5, 0.6)
}

/// Grass seen from above: clumps of light and shade, and short blades.
fn grass_top() -> Tex {
    let s = seed_of("grass2");
    let mut a = Art::new("grass_top", &[GRASS]);
    a.fill(|x, y, _| {
        let low = fbm(x, y, 4, 2, s);
        let mid = fbm(x, y, 8, 2, s ^ 1);
        px(0, 0.42 + 0.55 * (low - 0.5) + 0.35 * (mid - 0.5), 0.3 + 0.4 * low + 0.2 * mid, 0.9)
    });
    for _ in 0..170 {
        let p = Vec2::new(a.r.next() * 64.0, a.r.next() * 64.0);
        let d = Vec2::from_angle(-1.9 + a.r.next() * 0.8 + if a.r.next() < 0.3 { 3.14 } else { 0.0 }) * (2.0 + a.r.next() * 3.0);
        let lift = 0.15 + a.r.next() * 0.2;
        // its shadow first, a texel down and right, then the blade
        a.line(p + Vec2::new(1.0, 1.0), p + d + Vec2::new(1.0, 1.0), |_, q| {
            q.t -= 0.12;
            q.h -= 0.05;
        });
        a.line(p, p + d, |f, q| {
            q.t += lift * (0.5 + f);
            q.h = (q.h + 0.25).min(1.0);
        });
    }
    a.finish(2.0, 0.5)
}

/// Soil with grass hanging over its top edge.
/// Earth with a cap of snow over its top, for grass that snow lies on.
fn grass_snow_side() -> Tex {
    let d = seed_of("dirt2");
    let s = seed_of("snow2");
    let mut a = Art::new("grass_snow_side", &[DIRT, PEBBLE, SNOW]);
    let edge: Vec<f32> = (0..N).map(|x| 9.0 + 4.0 * vn(x as f32, 0.0, 6, 1, s ^ 15) + 3.0 * smooth(0.6, 0.9, vn(x as f32, 0.0, 12, 1, s ^ 16))).collect();
    a.fill(|x, y, _| {
        let e = edge[x as usize];
        if y < e {
            let m = fbm(x, y, 8, 2, s);
            return px(2, 0.8 + 0.2 * (m - 0.5) - 0.45 * (y / e).powf(2.0), 0.9 - 0.2 * (y / e), 0.6);
        }
        let mut p = dirt_px(x, y, d);
        if y < e + 2.0 {
            p.t -= 0.25;
        }
        p
    });
    a.finish(2.5, 0.7)
}

fn grass_side() -> Tex {
    let s = seed_of("dirt2");
    let g = seed_of("grass2");
    let mut a = Art::new("grass_side", &[DIRT, PEBBLE, GRASS]);
    let edge: Vec<f32> = (0..N).map(|x| 8.0 + 5.0 * vn(x as f32, 0.0, 8, 1, g ^ 21) + if hash(x, 0, g ^ 22) < 0.3 { 2.0 + 6.0 * hash(x, 1, g) } else { 0.0 }).collect();
    a.fill(|x, y, _| {
        let e = edge[x as usize];
        if y < e {
            let m = fbm(x, y, 8, 2, g ^ 1);
            // lit on top, in its own shade where it hangs
            return px(2, 0.62 + 0.3 * (m - 0.5) - 0.45 * (y / e).powf(1.5), 0.85 - 0.3 * (y / e), 0.9);
        }
        let mut p = dirt_px(x, y, s);
        if y < e + 2.0 {
            p.t -= 0.35;
            p.h *= 0.6;
        }
        p
    });
    a.finish(2.5, 0.6)
}

/// Sand in soft ripples, with a few darker and lighter grains.
fn sand() -> Tex {
    let s = seed_of("sand2");
    let mut a = Art::new("sand", &[SAND]);
    a.fill(|x, y, _| {
        let m = fbm(x, y, 4, 3, s);
        let w = (std::f32::consts::TAU * (y / 10.0 + 1.6 * fbm(x, y, 2, 2, s ^ 4))).sin();
        let mut t = 0.52 + 0.4 * (m - 0.5) + 0.08 * w;
        let k = hash(x as i32, y as i32, s ^ 8);
        if k < 0.03 {
            t -= 0.25;
        } else if k > 0.975 {
            t += 0.2;
        }
        px(0, t, 0.5 + 0.08 * w + 0.3 * (m - 0.5), 0.95)
    });
    a.finish(1.0, 0.5)
}

/// Small stones of a few colours packed together.
fn gravel() -> Tex {
    let s = seed_of("gravel2");
    let mut a = Art::new("gravel", &[GRAVEL_GREY, GRAVEL_BROWN, GRAVEL_PALE, MORTAR]);
    a.fill(|x, y, _| {
        let c = cell(x, y, 8, 8, 0.95, s);
        if c.edge < 0.9 {
            return px(3, 0.2, 0.0, 1.0);
        }
        let dome = smooth(0.9, 3.5, c.edge).sqrt();
        let k = c.rand(1);
        let pal = if k < 0.5 { 0 } else if k < 0.8 { 1 } else { 2 };
        px(pal, 0.35 + 0.4 * c.rand(2) + 0.25 * dome, 0.2 + 0.8 * dome, 0.8)
    });
    a.finish(3.0, 1.4)
}

/// Bark: long plates between wavy furrows running up the trunk, each plate
/// broken across here and there, with streaks along it.
fn log_side() -> Tex {
    let s = seed_of("bark2");
    let mut a = Art::new("log_side", &[BARK]);
    a.fill(|x, y, _| {
        let wx = x + 5.0 * (vn(x, y, 2, 3, s ^ 9) - 0.5) + 2.0 * (vn(x, y, 4, 8, s ^ 10) - 0.5);
        let band = wx.rem_euclid(64.0) / 8.0;
        let col = band.floor() as i32;
        let across = band.fract();
        // how far from the furrow either side, in px
        let from = across.min(1.0 - across) * 8.0;
        let len = 14.0 + 12.0 * hash(col.rem_euclid(8), 0, s);
        let along = (y + hash(col.rem_euclid(8), 1, s) * 64.0) / len;
        let piece = along.floor() as i32;
        let breaks = (along.fract() * len) < 1.0 && hash(col.rem_euclid(8), piece, s ^ 2) < 0.7;
        let streak = vn(x, y, 32, 2, s ^ 3);
        let m = fbm(x, y, 4, 2, s ^ 5);
        if from < 1.1 || breaks {
            return px(0, 0.03 + 0.1 * m, 0.0, 1.0);
        }
        let ridge = smooth(1.1, 3.5, from);
        let plate = hash(col.rem_euclid(8), piece, s ^ 4);
        px(0, 0.38 + 0.2 * (plate - 0.5) + 0.3 * (streak - 0.5) + 0.2 * (m - 0.5) + 0.18 * ridge, 0.25 + 0.6 * ridge + 0.15 * streak, 0.9)
    });
    a.finish(3.0, 1.1)
}

/// A sawn end: growth rings round the pith, a couple of cracks, and bark round the rim.
fn log_top() -> Tex {
    let s = seed_of("rings2");
    let mut a = Art::new("log_top", &[RING, BARK]);
    let cracks = [0.6f32, 2.9, 4.4];
    a.fill(|x, y, _| {
        let (dx, dy) = (x - 31.5, y - 31.5);
        let r = (dx * dx + dy * dy).sqrt() + 2.0 * (vn(x, y, 4, 4, s) - 0.5);
        let ang = dy.atan2(dx).rem_euclid(std::f32::consts::TAU);
        let m = fbm(x, y, 8, 2, s ^ 2);
        if r > 28.5 {
            return px(1, 0.35 + 0.4 * (m - 0.5) + 0.2 * vn(x, y, 16, 16, s ^ 6), 0.8, 0.95);
        }
        if r > 26.8 {
            return px(0, 0.0, 0.5, 0.9);
        }
        let ring = (r / 3.3).fract();
        let mut t = 0.62 + 0.2 * (m - 0.5) - 0.12 * (r / 28.0) - if ring < 0.28 { 0.3 } else { 0.0 };
        let mut h = 0.6 + if ring < 0.28 { -0.08 } else { 0.0 };
        if r < 1.6 {
            t = 0.15;
        }
        if r > 5.0 && r < 24.0 && cracks.iter().any(|c| (ang - c).abs() * r < 0.7) {
            t = 0.0;
            h = 0.2;
        }
        px(0, t, h, 0.85)
    });
    a.finish(1.5, 0.6)
}

/// Birch: pale bark with dark marks across it and the odd dark patch.
fn birch_side() -> Tex {
    let s = seed_of("birch2");
    let mut a = Art::new("birch_side", &[BIRCH, BIRCH_MARK]);
    a.fill(|x, y, _| {
        let m = fbm(x, y, 4, 3, s);
        let v = vn(x, y, 8, 2, s ^ 4);
        px(0, 0.62 + 0.4 * (m - 0.5) + 0.2 * (v - 0.5), 0.6 + 0.2 * m, 0.7)
    });
    for i in 0..34 {
        let p = Vec2::new(a.r.next() * 64.0, a.r.next() * 64.0);
        let len = if i < 6 { 9.0 + a.r.next() * 10.0 } else { 2.0 + a.r.next() * 6.0 };
        let thick = if i < 6 { 2 + (a.r.next() * 2.0) as i32 } else { 1 };
        for k in 0..thick {
            // ragged ends: each row a little shorter or longer
            let (l0, l1) = (a.r.next() * 2.0, len - a.r.next() * 2.0);
            let y = p.y + k as f32;
            a.line(Vec2::new(p.x + l0, y), Vec2::new(p.x + l1, y), |f, q| {
                q.pal = 1;
                q.t = 0.15 + 0.5 * (1.0 - (f - 0.5).abs() * 2.0) * 0.4;
                q.h = 0.25;
                q.rough = 0.95;
            });
        }
        // a faint shadow under the long ones
        if i < 6 {
            let y = p.y + thick as f32;
            a.line(Vec2::new(p.x + 1.0, y), Vec2::new(p.x + len - 1.0, y), |_, q| {
                if q.pal == 0 {
                    q.t -= 0.15;
                }
            });
        }
    }
    a.finish(2.0, 0.5)
}

/// Four boards across, their ends in different places, with grain, a knot
/// or two and nails at the ends.
fn planks() -> Tex {
    let s = seed_of("planks2");
    let mut a = Art::new("planks", &[PLANK, NAIL]);
    let seams: Vec<i32> = (0..4).map(|row| (hash(row, 0, s) * 64.0) as i32).collect();
    let knots: Vec<Vec2> = (0..4).map(|row| Vec2::new(hash(row, 3, s) * 64.0, row as f32 * 16.0 + 5.0 + hash(row, 4, s) * 6.0)).collect();
    a.fill(|x, y, _| {
        let (xi, yi) = (x as i32, y as i32);
        let row = yi / 16;
        let yy = yi % 16;
        let xr = (xi - seams[row as usize]).rem_euclid(N);
        let board = hash(row, if xr < 32 { 1 } else { 2 }, s);
        if yy == 15 || xr == 0 {
            return px(0, 0.0, 0.0, 1.0);
        }
        if (xr == 2 || xr == 62) && (yy == 4 || yy == 11) {
            return px(1, 0.6, 0.9, 0.4);
        }
        let k = knots[row as usize];
        let kd = Vec2::new((x - k.x + 32.0).rem_euclid(64.0) - 32.0, (y - k.y) * 1.6).length();
        let warp = 2.5 * fbm(x, y, 4, 2, s ^ 6) + if kd < 6.0 { (6.0 - kd) * 0.6 } else { 0.0 };
        let grain = ((yy as f32 + warp) * 1.25 + board * 6.0).sin();
        let mut t = 0.52 + 0.22 * (board - 0.5) + 0.12 * grain + 0.12 * (vn(x, y, 16, 4, s ^ 8) - 0.5);
        if kd < 1.8 {
            t = 0.12;
        } else if kd < 2.6 {
            t -= 0.2;
        }
        if yy == 0 {
            t += 0.15;
        } else if yy == 14 {
            t -= 0.15;
        }
        px(0, t, 0.7 + 0.04 * grain, 0.7 + 0.08 * grain)
    });
    a.finish(2.5, 0.6)
}

/// Stone bricks in a running bond, moss growing over their tops and in the joints.
fn mossy_brick() -> Tex {
    let s = seed_of("brick2");
    let mut a = Art::new("mossy_brick", &[BRICK, MOSS, MORTAR]);
    a.fill(|x, y, r| {
        let (xi, yi) = (x as i32, y as i32);
        let row = yi / 8;
        let bx = (xi + (row % 2) * 8).rem_euclid(16);
        let by = yi % 8;
        let col = (xi + (row % 2) * 8).div_euclid(16);
        let brick = hash(col.rem_euclid(4), row, s);
        let m = fbm(x, y, 8, 2, s ^ 2);
        let mortar = bx == 0 || by == 0;
        let moss = fbm(x, y, 4, 3, s ^ 9) + 0.18 * (1.0 - by as f32 / 8.0) + if mortar { 0.1 } else { 0.0 };
        if moss > 0.74 + 0.08 * r.next() {
            return px(1, 0.25 + (moss - 0.74) * 2.6 + 0.25 * (m - 0.5), 0.75 + 0.2 * m, 0.95);
        }
        if mortar {
            return px(2, 0.4 + 0.3 * m, 0.0, 1.0);
        }
        let mut t = 0.48 + 0.3 * (brick - 0.5) + 0.3 * (m - 0.5);
        if bx == 1 || by == 1 {
            t += 0.12;
        } else if bx == 15 || by == 7 {
            t -= 0.12;
        }
        if hash(xi, yi, s ^ 4) < 0.04 {
            t -= 0.18;
        }
        px(0, t, 0.65 + 0.2 * m, 0.85)
    });
    a.finish(2.5, 0.7)
}

fn snow() -> Tex {
    let s = seed_of("snow2");
    let mut a = Art::new("snow", &[SNOW]);
    a.fill(|x, y, _| {
        let m = fbm(x, y, 4, 3, s);
        if hash(x as i32, y as i32, s ^ 3) < 0.015 {
            return px(0, 1.0, 0.6, 0.25);
        }
        px(0, 0.7 + 0.45 * (m - 0.5) + 0.1 * (vn(x, y, 16, 16, s ^ 2) - 0.5), m, 0.6)
    });
    a.finish(1.5, 0.6)
}

/// Rock with a cap of snow over its top, rounded where it hangs.
fn snow_side() -> Tex {
    let st = seed_of("stone2");
    let s = seed_of("snow2");
    let mut a = Art::new("snow_side", &[STONE, SNOW]);
    let edge: Vec<f32> = (0..N).map(|x| 11.0 + 4.0 * vn(x as f32, 0.0, 6, 1, s ^ 5) + 4.0 * smooth(0.6, 0.9, vn(x as f32, 0.0, 12, 1, s ^ 6))).collect();
    a.fill(|x, y, _| {
        let e = edge[x as usize];
        if y < e {
            let m = fbm(x, y, 8, 2, s);
            return px(1, 0.8 + 0.2 * (m - 0.5) - 0.45 * (y / e).powf(2.0), 0.9 - 0.2 * (y / e), 0.6);
        }
        let mut p = stone_px(x, y, st);
        if y < e + 2.0 {
            p.t -= 0.3;
        }
        p
    });
    a.finish(2.5, 0.8)
}

/// Straw in strands, lying mostly one way, with shadow under each.
fn straw(name: &str, n: usize, across: bool, bands: bool) -> Tex {
    let s = seed_of(name);
    let mut a = Art::new(name, &[HAY, TWINE]);
    a.fill(|x, y, _| px(0, 0.25 + 0.25 * fbm(x, y, 8, 2, s), 0.2, 0.9));
    for _ in 0..n {
        let p = Vec2::new(a.r.next() * 64.0, a.r.next() * 64.0);
        let ang = if across { (a.r.next() - 0.5) * 0.5 } else { a.r.next() * std::f32::consts::TAU };
        let d = Vec2::from_angle(ang) * (5.0 + a.r.next() * 11.0);
        let t = 0.45 + a.r.next() * 0.5;
        a.line(p + Vec2::new(0.0, 1.0), p + d + Vec2::new(0.0, 1.0), |_, q| {
            q.t = (q.t - 0.15).max(0.1);
            q.h *= 0.8;
        });
        a.line(p, p + d, |f, q| {
            q.t = t + 0.12 * (1.0 - (f - 0.5).abs() * 2.0);
            q.h = 0.6 + 0.4 * t;
            q.rough = 0.8;
        });
    }
    if bands {
        for x0 in [14, 46] {
            for x in x0..x0 + 3 {
                for y in 0..N {
                    let twist = ((y + x) % 4 < 2) as i32 as f32;
                    *a.at(x, y) = Px { pal: 1, t: 0.35 + 0.35 * twist - if x == x0 + 2 { 0.25 } else { 0.0 }, h: 0.95, rough: 0.9, alpha: 1.0 };
                }
            }
            // the straw bunches up either side of the twine
            for y in 0..N {
                a.at(x0 - 1, y).t -= 0.2;
                a.at(x0 + 3, y).t -= 0.2;
            }
        }
    }
    a.finish(2.5, 0.7)
}

/// Knitted wool: rows of V-shaped stitches, a little fuzzy.
pub(super) fn wool(name: &str, pal: &'static [u32]) -> Tex {
    let s = seed_of(name);
    let mut a = Art::new(name, &[pal]);
    a.fill(|x, y, _| {
        let (u, v) = (x as i32 % 4, y as i32 % 4);
        // each stitch is two legs leaning in to the middle
        let leg = if u < 2 { v - u } else { v - (3 - u) };
        let lit = match leg.rem_euclid(4) {
            0 => 0.18,
            1 => 0.08,
            3 => -0.12,
            _ => -0.02,
        };
        let m = fbm(x, y, 4, 3, s);
        let fuzz = (hash(x as i32, y as i32, s ^ 2) - 0.5) * 0.12;
        px(0, 0.5 + lit + 0.35 * (m - 0.5) + fuzz, 0.5 + lit * 1.5 + 0.2 * m, 1.0)
    });
    a.finish(1.5, 0.3)
}

fn bedrock() -> Tex {
    let s = seed_of("bedrock2");
    let mut a = Art::new("bedrock", &[BEDROCK]);
    a.fill(|x, y, _| {
        let c = cell(x, y, 5, 5, 0.9, s);
        let m = fbm(x, y, 8, 2, s ^ 1);
        if c.edge < 1.2 {
            return px(0, 0.0, 0.0, 1.0);
        }
        px(0, 0.35 + 0.4 * (c.rand(1) - 0.5) + 0.4 * (m - 0.5), 0.3 + 0.5 * smooth(1.2, 5.0, c.edge) + 0.2 * m, 0.9)
    });
    a.finish(3.0, 1.4)
}

/// Surf along a shore, for the strips laid on the water beside the land
/// (`mesh::shore_foam`): a scalloped band of foam at the top edge, which
/// faces the shore, broken by bubbles, and specks drifting out from it.
/// It repeats across, so neighbouring strips run on into each other.
fn foam() -> Tex {
    const FOAM: &[u32] = &[0x4f7480, 0x6f939d, 0x96b6bd, 0xbdd5d8, 0xdcecec, 0xf3fafa, 0xffffff];
    let s = seed_of("foam");
    let mut a = Art::new("foam", &[FOAM]);
    a.fill(|x, y, _| {
        let wave = 3.5 * (x * std::f32::consts::TAU / 32.0 + 1.3 * vn(x, 0.0, 4, 1, s)).sin() + 4.0 * vn(x, y, 8, 8, s ^ 1);
        let band = 11.0 + wave;
        let bubble = vn(x, y, 16, 16, s ^ 2);
        if y < band {
            // the band: whiter toward the shore, holes where bubbles burst
            if bubble > 0.74 && y > 3.0 {
                return Px { pal: 0, t: 0.0, h: 0.0, rough: 1.0, alpha: 0.0 };
            }
            let t = 0.62 + 0.38 * (1.0 - y / band) - 0.18 * smooth(0.6, 0.74, bubble);
            return px(0, t, 0.5 + 0.5 * (1.0 - y / band), 0.55);
        }
        // past it, specks of foam thinning out to open water
        let out = (y - band) / (40.0 - band).max(1.0);
        let speck = hash(x as i32, y as i32, s ^ 3);
        if out < 1.0 && speck > 0.9 + 0.09 * out && bubble > 0.45 {
            return px(0, 0.55 + 0.3 * speck, 0.4, 0.6);
        }
        Px { pal: 0, t: 0.0, h: 0.0, rough: 1.0, alpha: 0.0 }
    });
    a.finish(1.5, 0.4)
}

/// Every block texture in the new look, by tile name.
pub(super) fn blocks() -> Vec<(&'static str, Tex)> {
    vec![
        ("grass_top", grass_top()),
        ("grass_side", grass_side()),
        ("dirt", dirt()),
        ("stone", stone()),
        ("sand", sand()),
        ("gravel", gravel()),
        ("log_side", log_side()),
        ("log_top", log_top()),
        ("birch_side", birch_side()),
        ("planks", planks()),
        ("mossy_brick", mossy_brick()),
        ("cobble", cobble()),
        ("snow", snow()),
        ("snow_side", snow_side()),
        ("grass_snow_side", grass_snow_side()),
        ("hay_side", straw("hay_side", 420, true, true)),
        ("hay_top", straw("hay_top", 300, false, false)),
        ("wool_red", wool("wool_red", WOOL_RED)),
        ("wool_cream", wool("wool_cream", WOOL_CREAM)),
        ("wool_blue", wool("wool_blue", WOOL_BLUE)),
        ("wool_yellow", wool("wool_yellow", WOOL_YELLOW)),
        ("bedrock", bedrock()),
        ("foam", foam()),
    ]
    .into_iter()
    .chain(props::tiles())
    .chain(tools::tiles())
    .chain(travel::tiles())
    .chain(wilds::tiles())
    .chain(ores::tiles())
    .chain(smelting::tiles())
    .chain(food::tiles())
    .collect()
}
