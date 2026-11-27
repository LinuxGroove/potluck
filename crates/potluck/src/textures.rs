//! The base game's block textures, drawn from code: albedo, a normal map
//! from each texture's height, roughness and glow. The blocks themselves are
//! 64 px pixel art painted in `art`; plants, leaves and glowing tiles are
//! drawn here at 64 px, and the rest at 32. They go into one atlas with its
//! own mip levels, made tile by tile so neighbours never bleed together, each
//! tile in a gutter of itself wrapped round (see `GUTTER`).
//!
//! Blocks baked by `tools/blocks.py` (in `assets/blocks`, built in by
//! build.rs) take the place of the 32 px drawings, with their own normal,
//! roughness and see-through maps; every tile is scaled (nearest) to the
//! bakes' size. `POTLUCK_DUMP_TEX=<dir> cargo test dump_textures` writes
//! every drawn texture out to look at.

use std::collections::HashMap;

use bevy::{
    asset::RenderAssetUsages,
    image::{ImageSampler, ImageSamplerDescriptor},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};

use crate::noise::{seed_of, tfbm, tnoise, Rng};

mod art;
pub mod skins;

/// How big the drawn textures are.
pub const TILE: usize = 32;
pub const GRID: usize = 16; // 16 x 16 tiles
pub const SIZE: usize = TILE * GRID;
/// Each tile sits in a cell with a gutter a tile's width over this all round,
/// filled with the tile wrapped round. Multisampling shades a pixel that only
/// touches a face's edge as if at the pixel's middle, a little past the face,
/// and there it finds more of the same texture rather than the next tile
/// (which showed as seams along block edges and specks on far hills).
const GUTTER: usize = 8;
/// How rough drawn blocks are (0 is a mirror).
const ROUGH: f32 = 0.92;

// pub const BAKED: &[(&str, &[u8])]: each baked map's file name and PNG
include!(concat!(env!("OUT_DIR"), "/baked_blocks.rs"));

/// One drawn texture: colours with alpha, and a height for the normal map,
/// `size` px square.
struct Tex {
    size: usize,
    rgba: Vec<[f32; 4]>,
    height: Vec<f32>,
    bump: f32,
    glow: bool,
    /// Roughness per texel (0 is a mirror); `ROUGH` all over without.
    rough: Option<Vec<f32>>,
}

#[derive(Resource, Clone)]
pub struct BlockAtlas {
    pub color: Handle<Image>,
    pub normal: Handle<Image>,
    /// Roughness in green, as `StandardMaterial` reads it.
    pub rough: Handle<Image>,
    pub glow: Handle<Image>,
    /// How much ambient light a face gets for the sky it sees
    /// (`mesh::sky_ambient`), read by the chunk materials as their
    /// occlusion texture through the second UVs.
    pub sky: Handle<Image>,
    tiles: HashMap<&'static str, u16>,
    /// Each tile's colour at full size, for item icons in the UI.
    pub icons: HashMap<&'static str, Handle<Image>>,
}

impl BlockAtlas {
    pub fn tile(&self, name: &str) -> u16 {
        *self.tiles.get(name).unwrap_or(&0)
    }

    /// Whether a tile of this name was drawn.
    pub fn has(&self, name: &str) -> bool {
        self.tiles.contains_key(name)
    }

    /// The UV rectangle of a tile: `(u0, v0, u1, v1)`, inside its gutter.
    pub fn uv(tile: u16) -> [f32; 4] {
        let (tx, ty) = ((tile as usize % GRID) as f32, (tile as usize / GRID) as f32);
        let s = 1.0 / GRID as f32;
        let g = s / (GUTTER + 2) as f32;
        [tx * s + g, ty * s + g, (tx + 1.0) * s - g, (ty + 1.0) * s - g]
    }
}

fn hex(h: u32) -> [f32; 3] {
    [((h >> 16) & 255) as f32, ((h >> 8) & 255) as f32, (h & 255) as f32]
}

/// A palette lookup, quantised to three steps between colours to keep it
/// looking like pixel art.
fn ramp(cols: &[u32], t: f32) -> [f32; 3] {
    let t = t.clamp(0.0, 0.999) * (cols.len() - 1) as f32;
    let i = t.floor() as usize;
    let q = ((t - i as f32) * 3.0).round() / 3.0;
    let (a, b) = (hex(cols[i]), hex(cols[(i + 1).min(cols.len() - 1)]));
    [a[0] + (b[0] - a[0]) * q, a[1] + (b[1] - a[1]) * q, a[2] + (b[2] - a[2]) * q]
}

fn px(c: [f32; 3], h: f32) -> ([f32; 4], f32) {
    ([c[0], c[1], c[2], 255.0], h)
}

fn lum(c: [f32; 3]) -> f32 {
    (c[0] * 0.3 + c[1] * 0.55 + c[2] * 0.15) / 255.0
}

fn speck(r: &mut Rng, t: f32, amount: f32) -> f32 {
    t + (r.next() - 0.5) * amount
}

fn make(name: &str, bump: f32, mut f: impl FnMut(f32, f32, &mut Rng) -> ([f32; 4], f32)) -> Tex {
    let mut r = Rng::new(seed_of(name));
    let mut rgba = Vec::with_capacity(TILE * TILE);
    let mut height = Vec::with_capacity(TILE * TILE);
    for y in 0..TILE {
        for x in 0..TILE {
            let (c, h) = f(x as f32, y as f32, &mut r);
            rgba.push(c);
            height.push(h);
        }
    }
    Tex { size: TILE, rgba, height, bump, glow: false, rough: None }
}

/// New art is drawn at 64 px (the stylised look Ken picked) by stamping
/// shapes: blades, leaves, petals. Row 0 is the top of a plant.
const ART: usize = 64;

struct Canvas {
    rgba: Vec<[f32; 4]>,
    height: Vec<f32>,
}

impl Canvas {
    fn new() -> Self {
        Canvas { rgba: vec![[0.0; 4]; ART * ART], height: vec![0.0; ART * ART] }
    }

    /// Fills the texels whose middles are within `r` of `(x, y)`.
    fn dot(&mut self, x: f32, y: f32, r: f32, c: [f32; 3], h: f32) {
        let (x0, x1) = ((x - r).floor().max(0.0) as usize, ((x + r).ceil() as usize).min(ART - 1));
        let (y0, y1) = ((y - r).floor().max(0.0) as usize, ((y + r).ceil() as usize).min(ART - 1));
        for py in y0..=y1 {
            for px in x0..=x1 {
                let (dx, dy) = (px as f32 + 0.5 - x, py as f32 + 0.5 - y);
                if dx * dx + dy * dy <= r * r {
                    self.rgba[py * ART + px] = [c[0], c[1], c[2], 255.0];
                    self.height[py * ART + px] = h;
                }
            }
        }
    }

    /// A curved stroke from `a` to `b` bending through `m`, thinning from
    /// `r0` to `r1`, its colour going from `c0` to `c1`.
    fn stroke(&mut self, a: Vec2, m: Vec2, b: Vec2, (r0, r1): (f32, f32), c0: [f32; 3], c1: [f32; 3]) {
        let steps = ((a.distance(m) + m.distance(b)) * 3.0) as usize + 2;
        for i in 0..=steps {
            let t = i as f32 / steps as f32;
            let p = a.lerp(m, t).lerp(m.lerp(b, t), t);
            let c = [c0[0] + (c1[0] - c0[0]) * t, c0[1] + (c1[1] - c0[1]) * t, c0[2] + (c1[2] - c0[2]) * t];
            self.dot(p.x, p.y, r0 + (r1 - r0) * t, c, 0.3 + 0.7 * t);
        }
    }

    fn tex(self, bump: f32) -> Tex {
        Tex { size: ART, rgba: self.rgba, height: self.height, bump, glow: false, rough: None }
    }
}

fn shade(c: [f32; 3], k: f32) -> [f32; 3] {
    [c[0] * k, c[1] * k, c[2] * k]
}

/// Grass blades from the bottom edge: `n` of them, `tall` (min, max) px.
fn blades(cv: &mut Canvas, r: &mut Rng, n: usize, tall: (f32, f32), pal: &[u32]) -> Vec<Vec2> {
    let mut tips = Vec::new();
    for _ in 0..n {
        let bx = 3.0 + r.next() * 58.0;
        let h = tall.0 + (tall.1 - tall.0) * r.next().powf(1.4);
        let lean = (r.next() - 0.5) * 22.0 * (h / tall.1);
        let a = Vec2::new(bx, 64.5);
        let b = Vec2::new(bx + lean, 64.0 - h);
        let m = Vec2::new(bx + lean * 0.15, 64.0 - h * 0.55);
        let k = 0.85 + r.next() * 0.3;
        cv.stroke(a, m, b, (1.35 + r.next() * 0.4, 0.45), shade(ramp(pal, 0.1), k), shade(ramp(pal, 0.75 + r.next() * 0.25), k));
        tips.push(b);
    }
    tips
}

fn tuft(name: &str, n: usize, tall: (f32, f32), flowers: usize, seeds: usize) -> Tex {
    let mut r = Rng::new(seed_of(name));
    let mut cv = Canvas::new();
    let tips = blades(&mut cv, &mut r, n, tall, TUFT);
    for tip in tips.iter().take(seeds) {
        // a seed head: a little oval of straw at the blade's tip
        for i in 0..5 {
            let c = ramp(&[0xa88d4c, 0xc9b77a, 0xe0d29a], 0.3 + r.next() * 0.6);
            cv.dot(tip.x, tip.y + i as f32 * 1.2, 1.3, c, 1.0);
        }
    }
    let petals = [0xf6d24a, 0xf2efe6, 0xe88ab0, 0x9fbdf2, 0xf08a5c];
    for tip in tips.iter().rev().take(flowers) {
        let c = hex(petals[(r.next() * petals.len() as f32) as usize % petals.len()]);
        for k in 0..5 {
            let a = k as f32 * 1.2566 + r.next() * 0.3;
            cv.dot(tip.x + a.cos() * 2.0, tip.y + a.sin() * 2.0, 1.4, shade(c, 0.9 + r.next() * 0.15), 1.0);
        }
        cv.dot(tip.x, tip.y, 1.0, hex(0xf0b93a), 1.0);
    }
    cv.tex(1.0)
}

/// Leaves for a leaf block: little pointed leaves, each with a dark edge on
/// its lower side, lighter towards the top, with gaps to see the leaves
/// behind through. `n` leaves `len` px long; pine needles are thin.
fn leaf_art(name: &str, pal: &[u32], n: usize, len: (f32, f32), thin: bool) -> Tex {
    let mut r = Rng::new(seed_of(name));
    let mut cv = Canvas::new();
    let w = if thin { (0.9, 0.35) } else { (2.0, 0.5) };
    for _ in 0..n {
        let p = Vec2::new(r.next() * 64.0, r.next() * 64.0);
        let d = Vec2::from_angle(if thin { -1.2 + r.next() * 0.6 } else { r.next() * std::f32::consts::TAU }) * (len.0 + (len.1 - len.0) * r.next()) * 0.5;
        let bend = Vec2::new(-d.y, d.x) * (r.next() - 0.5) * 0.5;
        let light = (0.2 + 0.65 * r.next() + (32.0 - p.y) / 160.0).clamp(0.0, 1.0);
        let off = Vec2::new(0.8, 1.0);
        cv.stroke(p - d + off, p + bend + off, p + d + off, w, shade(ramp(pal, 0.0), 0.7), shade(ramp(pal, 0.05), 0.7));
        cv.stroke(p - d, p + bend, p + d, w, ramp(pal, light * 0.85), ramp(pal, light));
    }
    cv.tex(2.0)
}

/// Grass hanging over the top edge of a block's side, and down a little way.
fn grass_lip() -> Tex {
    let mut r = Rng::new(seed_of("grass_lip"));
    let mut cv = Canvas::new();
    for x in 0..ART {
        for y in 0..3 {
            cv.dot(x as f32 + 0.5, y as f32 + 0.5, 0.6, ramp(GRASS, 0.55 + r.next() * 0.2), 1.0);
        }
    }
    for _ in 0..70 {
        let x = r.next() * 64.0;
        let len = 4.0 + 22.0 * r.next().powf(1.8);
        let sway = (r.next() - 0.5) * 6.0;
        let a = Vec2::new(x, 1.0);
        let b = Vec2::new(x + sway, 1.0 + len);
        cv.stroke(a, a.lerp(b, 0.5) + Vec2::new(sway * 0.3, 0.0), b, (1.5, 0.5), ramp(GRASS, 0.6), ramp(GRASS, 0.25 + r.next() * 0.2));
    }
    cv.tex(1.0)
}

/// A fern: fronds arching out from the middle, with leaflets down each side.
fn fern() -> Tex {
    let mut r = Rng::new(seed_of("fern"));
    let mut cv = Canvas::new();
    for i in 0..7 {
        let side = i as f32 / 6.0 * 2.0 - 1.0;
        let a = Vec2::new(32.0 + side * 3.0, 64.0);
        let b = Vec2::new(32.0 + side * 30.0, 14.0 + side.abs() * 26.0 + r.next() * 6.0);
        let m = Vec2::new(32.0 + side * 8.0, 6.0 + side.abs() * 8.0);
        let k = 0.85 + r.next() * 0.3;
        let steps = 22;
        for j in 0..=steps {
            let t = j as f32 / steps as f32;
            let p = a.lerp(m, t).lerp(m.lerp(b, t), t);
            let q = a.lerp(m, t + 0.02).lerp(m.lerp(b, t + 0.02), t + 0.02);
            let along = (q - p).normalize_or_zero();
            let across = Vec2::new(-along.y, along.x);
            let len = 7.0 * (1.0 - t).powf(0.6) * t.powf(0.3) + 1.0;
            let c = shade(ramp(LEAF, 0.35 + t * 0.5), k);
            cv.dot(p.x, p.y, 0.9, shade(c, 0.8), 0.5);
            if j > 2 {
                for s in [-1.0, 1.0] {
                    let tip = p + across * len * s + along * len * 0.5;
                    cv.stroke(p, p.lerp(tip, 0.5) + along, tip, (1.2, 0.5), c, shade(c, 1.1));
                }
            }
        }
    }
    cv.tex(1.5)
}

/// A bush: a dome of small leaves, darker underneath and inside.
fn bush() -> Tex {
    let mut r = Rng::new(seed_of("bush"));
    let mut cv = Canvas::new();
    for i in 0..420 {
        // later leaves land on top, so the outside ones come last
        let out = (i as f32 / 420.0).sqrt();
        let ang = r.next() * std::f32::consts::TAU;
        let (x, y) = (32.0 + ang.cos() * 29.0 * out, 38.0 + ang.sin() * 25.0 * out);
        if y > 63.0 {
            continue;
        }
        let light = (0.25 + out * 0.45 + (38.0 - y) / 70.0 + (r.next() - 0.5) * 0.25).clamp(0.0, 1.0);
        let dir = Vec2::from_angle(r.next() * std::f32::consts::TAU) * 3.0;
        let p = Vec2::new(x, y);
        cv.stroke(p - dir, p, p + dir, (1.6, 0.8), ramp(LEAF, light * 0.8), ramp(LEAF, light));
    }
    cv.tex(2.0)
}

/// Like [`make`], with the height taken from the colour's brightness.
fn make_lum(name: &str, bump: f32, mut f: impl FnMut(f32, f32, &mut Rng) -> [f32; 3]) -> Tex {
    make(name, bump, |x, y, r| {
        let c = f(x, y, r);
        px(c, lum(c))
    })
}

const GRASS: &[u32] = &[0x2a5a24, 0x376f2a, 0x4a8731, 0x5f9c39, 0x78b146];
/// Ground cover, a little lighter than the ground so the tips catch the sun
/// (the mesh tints each tuft, lush or dry, see `mesh::ground_cover`).
const TUFT: &[u32] = &[0x274f20, 0x3a7429, 0x559336, 0x76b047, 0x9bc860];
const DIRT: &[u32] = &[0x4a3020, 0x664330, 0x7d563b, 0x936948];
const STONE: &[u32] = &[0x55595f, 0x6b7077, 0x80868c, 0x979ca1, 0xaeb2b5];
const SAND: &[u32] = &[0xb19d6b, 0xbfac7a, 0xcab989, 0xd3c59b];
const GRAVEL: &[u32] = &[0x6c6a66, 0x86817a, 0x9e978d, 0xb8b0a3];
const BARK: &[u32] = &[0x3b2a1c, 0x4e3824, 0x634830, 0x7a5a3c];
const RING: &[u32] = &[0x9c7a4c, 0xb48f5a, 0xc9a46b, 0xdcb97e];
const LEAF: &[u32] = &[0x1f4d22, 0x2b6a2b, 0x3d8a34, 0x56a63f, 0x74bd4c];
const PINE: &[u32] = &[0x15362a, 0x1d4a34, 0x275f3f, 0x34744a];
const BIRCH: &[u32] = &[0x4d7f2e, 0x66a03a, 0x86bb48, 0xa6d15c];
const PLANK: &[u32] = &[0x7a5230, 0x93653c, 0xa97748, 0xbd8a56];
const MOSS: &[u32] = &[0x4f5a52, 0x606d61, 0x6f7f6a, 0x5f8a4a, 0x76a254];
const SNOW: &[u32] = &[0xcfd9e3, 0xdee6ee, 0xeef3f8, 0xffffff];
const HAY: &[u32] = &[0xa6801f, 0xc29b2c, 0xd9b545, 0xead06a];

const NONE: ([f32; 4], f32) = ([0.0, 0.0, 0.0, 0.0], 0.0);

fn draw_all() -> Vec<(&'static str, Tex)> {
    let mut t: Vec<(&'static str, Tex)> = Vec::new();
    // Tile 0 is a plain magenta, so a missing texture is obvious.
    t.push(("missing", make("missing", 1.0, |_, _, _| px([255.0, 0.0, 255.0], 0.5))));
    t.push(("grass_top", make("grass_top", 2.2, |x, y, r| {
        let v = speck(r, tfbm(x, y, 3), 0.35);
        px(ramp(GRASS, v * 0.9 + 0.08), v)
    })));
    let edge: Vec<f32> = (0..32).map(|x| 4.0 + (tnoise(x as f32, 0.0, 8, 41) * 5.0).round()).collect();
    let dirt_px = |x: f32, y: f32, r: &mut Rng| {
        let v = speck(r, tfbm(x, y, 11), 0.35);
        let pebble = tnoise(x, y, 16, 77) > 0.78;
        if pebble { px(ramp(STONE, 0.5), 0.9) } else { px(ramp(DIRT, v), v * 0.6) }
    };
    t.push(("grass_side", make("grass_side", 2.2, |x, y, r| {
        let e = edge[x as usize];
        if y < e {
            px(ramp(GRASS, speck(r, 0.55 + 0.25 * tnoise(x, y, 8, 3), 0.3)), 0.8)
        } else if y == e {
            px(ramp(GRASS, 0.15), 0.6)
        } else {
            dirt_px(x, y, r)
        }
    })));
    t.push(("dirt", make("dirt", 2.2, dirt_px)));
    t.push(("stone", make("stone", 3.0, |x, y, r| {
        let v = tfbm(x, y, 21);
        let crack = (tnoise(x, y, 6, 23) - 0.5).abs() < 0.035;
        px(ramp(STONE, if crack { 0.05 } else { speck(r, v, 0.2) }), if crack { 0.1 } else { v })
    })));
    t.push(("sand", make_lum("sand", 2.2, |x, y, r| ramp(SAND, speck(r, tfbm(x, y, 31), 0.4)))));
    t.push(("gravel", make("gravel", 4.0, |x, y, r| {
        let v = tnoise(x, y, 10, 33);
        px(ramp(GRAVEL, speck(r, v * v * 1.3, 0.25)), v)
    })));
    t.push(("log_side", make("log_side", 3.0, |x, y, r| {
        let v = speck(r, 0.5 + 0.5 * (x * 1.7 + tnoise(x, y, 4, 51) * 6.0).sin() * 0.6, 0.25);
        px(ramp(BARK, v), v)
    })));
    t.push(("log_top", make("log_top", 2.2, |x, y, r| {
        let d = ((x - 15.5).powi(2) + (y - 15.5).powi(2)).sqrt();
        if d > 14.5 {
            return px(ramp(BARK, speck(r, 0.4, 0.3)), 0.3);
        }
        let ring = 0.5 + 0.5 * (d * 1.6 + tnoise(x, y, 4, 52) * 2.0).sin();
        px(ramp(RING, speck(r, ring * 0.8 + 0.1, 0.15)), ring)
    })));
    for (name, pal, holes) in [("leaves", LEAF, 0.22), ("pine", PINE, 0.18), ("birch_leaves", BIRCH, 0.24)] {
        let s = name.len() as i32;
        t.push((name, make(name, 3.0, |x, y, r| {
            let v = tfbm(x, y, s * 7);
            if tnoise(x, y, 16, s * 3) < holes {
                return NONE;
            }
            px(ramp(pal, speck(r, v, 0.45)), v)
        })));
    }
    t.push(("birch_side", make("birch_side", 2.2, |x, y, r| {
        if tnoise(x, y * 0.4, 8, 61) > 0.72 {
            return px([40.0, 38.0, 36.0], 0.2);
        }
        px(ramp(&[0xbab3a6, 0xd8d2c6, 0xeeeae2], speck(r, 0.85, 0.12)), 0.8)
    })));
    t.push(("planks", make("planks", 3.0, |x, y, r| {
        let row = (y / 8.0).floor();
        let seam = y as i32 % 8 == 7 || (x as i32 + row as i32 * 13) % 32 == 0;
        let v = if seam { 0.02 } else { speck(r, 0.45 + 0.35 * tnoise(x * 0.3, y, 4, 70 + row as i32), 0.18) };
        px(ramp(PLANK, v), if seam { 0.0 } else { 0.7 })
    })));
    t.push(("mossy_brick", make("mossy_brick", 3.5, |x, y, r| {
        let row = (y / 8.0).floor() as i32;
        let off = if row % 2 == 1 { 8 } else { 0 };
        let seam = y as i32 % 8 == 7 || (x as i32 + off) % 16 == 15;
        let mossy = tfbm(x, y, 88) > 0.58;
        let v = speck(r, tfbm(x, y, 87), 0.22);
        if seam {
            return px(ramp(if mossy { LEAF } else { &[0x3c403e, 0x4a4f4c] }, 0.3), 0.0);
        }
        px(ramp(if mossy { &MOSS[2..] } else { &MOSS[..3] }, v), 0.6 + v * 0.4)
    })));
    t.push(("cobble", make("cobble", 4.0, |x, y, r| {
        let v = tnoise(x, y, 7, 95);
        let edge = (tnoise(x, y, 7, 96) - 0.5).abs() < 0.05;
        px(ramp(STONE, if edge { 0.05 } else { speck(r, v * 0.9 + 0.1, 0.2) }), if edge { 0.0 } else { v })
    })));
    t.push(("snow", make_lum("snow", 2.2, |x, y, r| ramp(SNOW, speck(r, tfbm(x, y, 41), 0.25)))));
    t.push(("snow_side", make("snow_side", 2.2, |x, y, r| {
        if y < 7.0 + tnoise(x, 0.0, 8, 42) * 4.0 {
            return px(ramp(SNOW, speck(r, 0.6, 0.25)), 0.9);
        }
        let c = ramp(STONE, speck(r, tfbm(x, y, 21), 0.2));
        px(c, lum(c))
    })));
    t.push(("hay_side", make("hay_side", 3.0, |x, y, r| {
        let band = (12..=13).contains(&(y as i32 % 16));
        if band {
            return px([120.0, 70.0, 40.0], 0.9);
        }
        let v = speck(r, 0.5 + 0.4 * (x * 2.3 + tnoise(x, y, 8, 19) * 4.0).sin() * 0.5, 0.35);
        px(ramp(HAY, v), v)
    })));
    t.push(("hay_top", make_lum("hay_top", 2.2, |x, y, r| ramp(HAY, speck(r, tfbm(x, y, 19), 0.5)))));
    for (name, cols) in [
        ("wool_red", [0x8e2325, 0xb3332f, 0xcc463b, 0xde5d4b]),
        ("wool_cream", [0xc9bfa6, 0xddd3ba, 0xece4cd, 0xf6efdc]),
        ("wool_blue", [0x1f4f8a, 0x2c65a8, 0x3c7cc2, 0x5595d6]),
        ("wool_yellow", [0xc28a17, 0xdca424, 0xeebd3a, 0xf8d35a]),
    ] {
        let s = name.len() as i32 + cols[0] as i32 % 7;
        t.push((name, make(name, 1.5, |x, y, r| {
            let weave = if (x as i32 + y as i32) % 4 == 0 { -0.12 } else { 0.0 };
            let v = speck(r, 0.5 + 0.35 * (tnoise(x, y, 16, s) - 0.5) + weave, 0.15);
            px(ramp(&cols, v), v)
        })));
    }
    let mut lantern = make("lantern", 2.2, |x, y, r| {
        let (xi, yi) = (x as i32, y as i32);
        if xi < 3 || xi > 28 || yi < 3 || yi > 28 || xi == 15 || xi == 16 {
            return ([48.0, 40.0, 34.0, 255.0], 0.9);
        }
        let d = ((x - 15.5).powi(2) + (y - 15.5).powi(2)).sqrt() / 18.0;
        px(ramp(&[0xffd27a, 0xffb84d, 0xf08a2c], d + (r.next() - 0.5) * 0.1), 0.3)
    });
    lantern.glow = true;
    t.push(("lantern", lantern));
    t.push(("water", make("water", 1.2, |x, y, _| ([46.0, 110.0, 150.0, 200.0], tfbm(x, y, 101)))));
    t.push(("glass", make("glass", 1.0, |x, y, _| {
        let (xi, yi) = (x as i32, y as i32);
        let frame = xi < 1 || xi > 30 || yi < 1 || yi > 30;
        let glint = (xi - yi).abs() < 2 && (8..14).contains(&xi) || (xi - yi - 6).abs() < 1 && (14..18).contains(&xi);
        if frame { ([196.0, 214.0, 222.0, 255.0], 0.8) } else if glint { ([235.0, 245.0, 250.0, 255.0], 0.5) } else { NONE }
    })));
    t.push(("bedrock", make_lum("bedrock", 3.0, |x, y, r| ramp(&[0x1d1f22, 0x2c2f33, 0x3d4146], speck(r, tnoise(x, y, 8, 7), 0.4)))));
    // cross sprites
    t.push(("tallgrass", make("tallgrass", 1.0, |x, y, r| {
        let blade = [3.0, 7.0, 10.0, 14.0, 17.0, 21.0, 24.0, 28.0].iter().enumerate().any(|(i, bx)| {
            let top = 6.0 + ((i * 7) % 13) as f32;
            let lean = (y - 31.0) * ((i as f32 % 3.0) - 1.0) * 0.18;
            (x - (bx + lean)).abs() < 1.1 && y > top
        });
        if !blade {
            return NONE;
        }
        px(ramp(GRASS, speck(r, 0.25 + (31.0 - y) / 40.0, 0.2)), 0.5)
    })));
    for (name, petal, centre) in [
        ("flower_red", 0xe0453c, 0xf6d24a),
        ("flower_blue", 0x5c86e8, 0xf4f0d0),
        ("flower_white", 0xf2efe6, 0xf0b93a),
        ("flower_violet", 0xa46ad8, 0xf6e27a),
    ] {
        t.push((name, make(name, 1.0, |x, y, r| {
            let stem = (x - 16.0).abs() < 1.0 && y > 12.0;
            let leaf = ((x - 12.0 - (y - 22.0) * 0.6).abs() < 1.3 && y > 18.0 && y < 26.0)
                || ((x - 20.0 + (y - 24.0) * 0.6).abs() < 1.3 && y > 20.0 && y < 28.0);
            let d = ((x - 16.0).powi(2) + (y - 9.0).powi(2)).sqrt();
            if d < 2.0 {
                return px(hex(centre), 0.8);
            }
            let ang = ((y - 9.0).atan2(x - 16.0) * 5.0 / std::f32::consts::PI + 10.0) % 2.0;
            if d < 6.0 && ang < 1.6 {
                let k = 0.85 + r.next() * 0.15;
                let c = hex(petal);
                return px([c[0] * k, c[1] * k, c[2] * k], 0.6);
            }
            if stem || leaf {
                return px(ramp(GRASS, 0.3 + r.next() * 0.2), 0.4);
            }
            NONE
        })));
    }
    // ground cover and undergrowth, at 64 px
    t.push(("tuft", tuft("tuft", 34, (16.0, 46.0), 0, 0)));
    t.push(("tuft_tall", tuft("tuft_tall", 22, (30.0, 62.0), 0, 6)));
    t.push(("tuft_flowers", tuft("tuft_flowers", 26, (18.0, 50.0), 7, 0)));
    t.push(("grass_lip", grass_lip()));
    t.push(("fern", fern()));
    t.push(("bush", bush()));
    // leaves at 64 px, in place of the baked ones (see `build`)
    t.retain(|(n, _)| !matches!(*n, "leaves" | "pine" | "birch_leaves"));
    t.push(("leaves", leaf_art("leaves", LEAF, 330, (5.0, 8.0), false)));
    t.push(("birch_leaves", leaf_art("birch_leaves", BIRCH, 360, (4.0, 6.0), false)));
    t.push(("pine", leaf_art("pine", PINE, 700, (6.0, 10.0), true)));
    // the blocks in the new look take the old drawings' places
    for (name, tex) in art::blocks() {
        match t.iter_mut().find(|(n, _)| *n == name) {
            Some(slot) => slot.1 = tex,
            None => t.push((name, tex)),
        }
    }
    t
}

fn normal_of(t: &Tex) -> Vec<[f32; 4]> {
    let n = t.size as i32;
    let h = |x: i32, y: i32| t.height[(y.rem_euclid(n) * n + x.rem_euclid(n)) as usize];
    let mut out = Vec::with_capacity(t.size * t.size);
    for y in 0..n {
        for x in 0..n {
            let dx = (h(x + 1, y) - h(x - 1, y)) * t.bump;
            let dy = (h(x, y + 1) - h(x, y - 1)) * t.bump;
            let v = Vec3::new(-dx, dy, 1.0).normalize();
            out.push([(v.x * 0.5 + 0.5) * 255.0, (v.y * 0.5 + 0.5) * 255.0, (v.z * 0.5 + 0.5) * 255.0, 255.0]);
        }
    }
    out
}

/// Builds one atlas image from tiles `full` px square, with each tile's
/// mips made from that tile alone, down to 8 px (whose gutter is one texel).
fn atlas_image(tiles: &[Vec<[f32; 4]>], full: usize, srgb: bool) -> Image {
    let mips = full.trailing_zeros() - 2;
    let mut data = Vec::new();
    for level in 0..mips {
        let tile = full >> level;
        let pad = tile / GUTTER;
        let cell = tile + 2 * pad;
        let size = cell * GRID;
        let mut img = vec![0u8; size * size * 4];
        let mut small = vec![[0u8; 4]; tile * tile];
        let step = 1 << level;
        for (i, src) in tiles.iter().enumerate() {
            for y in 0..tile {
                for x in 0..tile {
                    // average the block of full-size texels, weighting colour by alpha
                    let (mut c, mut a, mut w) = ([0.0f32; 3], 0.0f32, 0.0f32);
                    for sy in 0..step {
                        for sx in 0..step {
                            let p = src[(y * step + sy) * full + x * step + sx];
                            let k = p[3] / 255.0;
                            for j in 0..3 {
                                let v = if srgb { (p[j] / 255.0).powf(2.2) } else { p[j] / 255.0 };
                                c[j] += v * k;
                            }
                            a += k;
                            w += 1.0;
                        }
                    }
                    let o = &mut small[y * tile + x];
                    for j in 0..3 {
                        let v = if a > 0.0 { c[j] / a } else { 0.0 };
                        let v = if srgb { v.powf(1.0 / 2.2) } else { v };
                        o[j] = (v * 255.0).round().clamp(0.0, 255.0) as u8;
                    }
                    o[3] = ((a / w) * 255.0).round() as u8;
                }
            }
            let (ox, oy) = ((i % GRID) * cell, (i / GRID) * cell);
            for y in 0..cell {
                for x in 0..cell {
                    let p = small[((y + tile - pad) % tile) * tile + (x + tile - pad) % tile];
                    let o = ((oy + y) * size + ox + x) * 4;
                    img[o..o + 4].copy_from_slice(&p);
                }
            }
        }
        data.extend(img);
    }
    let format = if srgb { TextureFormat::Rgba8UnormSrgb } else { TextureFormat::Rgba8Unorm };
    let size = ((full + 2 * (full / GUTTER)) * GRID) as u32;
    let mut image = Image::new_uninit(Extent3d { width: size, height: size, depth_or_array_layers: 1 }, TextureDimension::D2, format, RenderAssetUsages::RENDER_WORLD);
    image.texture_descriptor.mip_level_count = mips;
    image.data = Some(data);
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        mipmap_filter: bevy::image::ImageFilterMode::Linear,
        ..ImageSamplerDescriptor::nearest()
    });
    image
}

fn icon_image(rgba: &[[f32; 4]], full: usize) -> Image {
    let mut data = Vec::with_capacity(full * full * 4);
    for p in rgba {
        data.extend(p.iter().map(|v| v.round().clamp(0.0, 255.0) as u8));
    }
    let mut image = Image::new(
        Extent3d { width: full as u32, height: full as u32, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::nearest();
    image
}

/// One block's maps at the atlas's size.
struct Maps {
    color: Vec<[f32; 4]>,
    normal: Vec<[f32; 4]>,
    rough: Vec<[f32; 4]>,
}

/// A drawn tile's texels (`n` px square) at `full` px: repeated to scale up,
/// averaged to scale down.
fn resample(src: &[[f32; 4]], n: usize, full: usize) -> Vec<[f32; 4]> {
    if full >= n {
        let k = full / n;
        return (0..full * full).map(|i| src[(i / full / k) * n + (i % full) / k]).collect();
    }
    let k = n / full;
    (0..full * full)
        .map(|i| {
            let (x, y) = (i % full, i / full);
            let (mut c, mut a) = ([0.0f32; 4], 0.0);
            for sy in 0..k {
                for sx in 0..k {
                    let p = src[(y * k + sy) * n + x * k + sx];
                    let w = p[3] / 255.0;
                    for j in 0..3 {
                        c[j] += p[j] * w;
                    }
                    a += w;
                }
            }
            if a > 0.0 {
                for v in c.iter_mut().take(3) {
                    *v /= a;
                }
            }
            c[3] = a / (k * k) as f32 * 255.0;
            c
        })
        .collect()
}

/// The baked maps in `BAKED`, decoded: name (with its `_n`, `_r` or `_a`) to
/// texels, all `size` px square. Files of another size are left out.
pub(crate) fn baked() -> (HashMap<&'static str, Vec<[f32; 4]>>, usize) {
    use bevy::image::{CompressedImageFormats, ImageType};
    let mut out = HashMap::new();
    let mut size = 0;
    for (file, bytes) in BAKED {
        let name = file.trim_end_matches(".png");
        let Ok(img) = Image::from_buffer(bytes, ImageType::Extension("png"), CompressedImageFormats::NONE, false, ImageSampler::Default, RenderAssetUsages::MAIN_WORLD) else {
            warn!("Couldn't read the baked block texture {file}");
            continue;
        };
        // PNGs come out as 8-bit RGBA
        let (w, h) = (img.width() as usize, img.height() as usize);
        let Some(data) = img.data.as_ref().filter(|d| d.len() == w * h * 4) else {
            warn!("The baked block texture {file} isn't 8-bit colour; leaving it out");
            continue;
        };
        if size == 0 && w.is_power_of_two() && w >= TILE {
            size = w;
        }
        if w != size || h != size {
            warn!("The baked block texture {file} is {w} px, not {size}; leaving it out");
            continue;
        }
        out.insert(name, data.chunks_exact(4).map(|p| [p[0] as f32, p[1] as f32, p[2] as f32, p[3] as f32]).collect());
    }
    (out, size.max(ART))
}

pub fn build(images: &mut Assets<Image>) -> BlockAtlas {
    let drawn = draw_all();
    let (baked, full) = baked();
    // the bakes' size only when one is used; the 64 px art needs no more
    let full = if drawn.iter().any(|(name, t)| !t.glow && t.size < ART && baked.contains_key(*name)) { full } else { ART };
    let maps: Vec<Maps> = drawn
        .iter()
        // bakes take the place of the old 32 px drawings, but not of glowing
        // tiles or of the 64 px art in the style Ken picked
        .map(|(name, t)| match baked.get(*name).filter(|_| !t.glow && t.size < ART) {
            Some(color) => {
                let mut color = color.clone();
                // leaves: the see-through map sets alpha
                if let Some(a) = baked.get(format!("{name}_a").as_str()) {
                    for (c, a) in color.iter_mut().zip(a) {
                        c[3] = if a[0] > 127.0 { 255.0 } else { 0.0 };
                    }
                }
                let normal = baked.get(format!("{name}_n").as_str()).cloned().unwrap_or_else(|| vec![[128.0, 128.0, 255.0, 255.0]; full * full]);
                let rough = baked
                    .get(format!("{name}_r").as_str())
                    .map(|r| r.iter().map(|p| [0.0, p[0], 0.0, 255.0]).collect())
                    .unwrap_or_else(|| vec![[0.0, ROUGH * 255.0, 0.0, 255.0]; full * full]);
                Maps { color, normal, rough }
            }
            None => {
                let rough = match &t.rough {
                    Some(r) => resample(&r.iter().map(|r| [0.0, r * 255.0, 0.0, 255.0]).collect::<Vec<_>>(), t.size, full),
                    None => vec![[0.0, ROUGH * 255.0, 0.0, 255.0]; full * full],
                };
                Maps { color: resample(&t.rgba, t.size, full), normal: resample(&normal_of(t), t.size, full), rough }
            }
        })
        .collect();
    if full > TILE {
        info!("Block textures at {full} px ({} baked)", drawn.iter().filter(|(n, t)| !t.glow && t.size < ART && baked.contains_key(*n)).count());
    }
    // Glowing tiles go at the far end of the atlas with empty tiles beside
    // them: past the smallest mip, a far face's edge can still reach its
    // neighbour in the atlas, and a lantern's glow there shows as sparks.
    let mut slots: Vec<Option<usize>> = vec![None; GRID * GRID];
    let (mut next, mut far) = (0, GRID * GRID - 1);
    for (i, (_, t)) in drawn.iter().enumerate() {
        if t.glow {
            slots[far] = Some(i);
            far -= 2;
        } else {
            slots[next] = Some(i);
            next += 1;
        }
    }
    assert!(next + GRID <= far, "too many block textures for the atlas");
    let empty = vec![[0.0, 0.0, 0.0, 0.0]; full * full];
    let pick = |f: &dyn Fn(usize) -> Vec<[f32; 4]>| -> Vec<Vec<[f32; 4]>> { slots.iter().map(|s| s.map(f).unwrap_or_else(|| empty.clone())).collect() };
    let colors = pick(&|i| maps[i].color.clone());
    let normals = pick(&|i| maps[i].normal.clone());
    let roughs = pick(&|i| maps[i].rough.clone());
    // only the bright parts of a glowing tile glow, not its frame
    let glows = pick(&|i| {
        if drawn[i].1.glow {
            maps[i].color.iter().map(|p| if p[0] + p[1] > 300.0 { *p } else { [0.0, 0.0, 0.0, 255.0] }).collect()
        } else {
            vec![[0.0, 0.0, 0.0, 255.0]; full * full]
        }
    });
    let mut tiles = HashMap::new();
    let mut icons = HashMap::new();
    for (slot, s) in slots.iter().enumerate() {
        if let Some(i) = s {
            tiles.insert(drawn[*i].0, slot as u16);
            icons.insert(drawn[*i].0, images.add(icon_image(&maps[*i].color, full)));
        }
    }
    BlockAtlas {
        color: images.add(atlas_image(&colors, full, true)),
        normal: images.add(atlas_image(&normals, full, false)),
        rough: images.add(atlas_image(&roughs, full, false)),
        glow: images.add(atlas_image(&glows, full, true)),
        sky: images.add(sky_image()),
        tiles,
        icons,
    }
}

/// The ambient light curve as a strip of texels, blended between.
fn sky_image() -> Image {
    let n = crate::mesh::SKY_STEPS;
    let data: Vec<u8> = (0..n)
        .flat_map(|i| {
            let v = (crate::mesh::sky_ambient(i as f32 / (n - 1) as f32) * 255.0).round() as u8;
            [v, v, v, 255]
        })
        .collect();
    let mut image = Image::new(
        Extent3d { width: n as u32, height: 1, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::linear();
    image
}

/// Writes every drawn tile to `dir` as PPM pictures at its own size:
/// `<name>.ppm` (colour over grey where see-through), `<name>_n.ppm` and
/// `<name>_r.ppm`. For looking at textures while drawing them.
pub fn dump(dir: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    for (name, t) in draw_all() {
        let n = t.size;
        let write = |suffix: &str, f: &dyn Fn(usize) -> [f32; 3]| -> std::io::Result<()> {
            let mut out = format!("P6 {n} {n} 255\n").into_bytes();
            for i in 0..n * n {
                out.extend(f(i).map(|v| v.round().clamp(0.0, 255.0) as u8));
            }
            std::fs::write(dir.join(format!("{name}{suffix}.ppm")), out)
        };
        write("", &|i| {
            let p = t.rgba[i];
            let a = p[3] / 255.0;
            [p[0] * a + 128.0 * (1.0 - a), p[1] * a + 128.0 * (1.0 - a), p[2] * a + 128.0 * (1.0 - a)]
        })?;
        let normal = normal_of(&t);
        write("_n", &|i| [normal[i][0], normal[i][1], normal[i][2]])?;
        write("_r", &|i| {
            let r = t.rough.as_ref().map(|r| r[i]).unwrap_or(ROUGH) * 255.0;
            [r, r, r]
        })?;
    }
    Ok(())
}
