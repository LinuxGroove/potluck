//! The base game's block textures, drawn from code at 32 px: albedo, a normal
//! map from each texture's height, and glow. They go into one atlas with its
//! own mip levels, made tile by tile so neighbours never bleed together, each
//! tile in a gutter of itself wrapped round (see `GUTTER`).
//!
//! Blocks baked by `tools/blocks.py` (in `assets/blocks`, built in by
//! build.rs) take the place of the drawn ones, with their own normal,
//! roughness and see-through maps; drawn tiles are scaled up to match.

use std::collections::HashMap;

use bevy::{
    asset::RenderAssetUsages,
    image::{ImageSampler, ImageSamplerDescriptor},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};

use crate::noise::{seed_of, tfbm, tnoise, Rng};

/// How big the drawn textures are.
pub const TILE: usize = 32;
pub const GRID: usize = 8; // 8 x 8 tiles
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

/// One drawn texture: colours with alpha, and a height for the normal map.
struct Tex {
    rgba: Vec<[f32; 4]>,
    height: Vec<f32>,
    bump: f32,
    glow: bool,
}

#[derive(Resource, Clone)]
pub struct BlockAtlas {
    pub color: Handle<Image>,
    pub normal: Handle<Image>,
    /// Roughness in green, as `StandardMaterial` reads it.
    pub rough: Handle<Image>,
    pub glow: Handle<Image>,
    tiles: HashMap<&'static str, u16>,
    /// Each tile's colour at full size, for item icons in the UI.
    pub icons: HashMap<&'static str, Handle<Image>>,
}

impl BlockAtlas {
    pub fn tile(&self, name: &str) -> u16 {
        *self.tiles.get(name).unwrap_or(&0)
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
    Tex { rgba, height, bump, glow: false }
}

/// Like [`make`], with the height taken from the colour's brightness.
fn make_lum(name: &str, bump: f32, mut f: impl FnMut(f32, f32, &mut Rng) -> [f32; 3]) -> Tex {
    make(name, bump, |x, y, r| {
        let c = f(x, y, r);
        px(c, lum(c))
    })
}

const GRASS: &[u32] = &[0x2a5a24, 0x376f2a, 0x4a8731, 0x5f9c39, 0x78b146];
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
    t
}

fn normal_of(t: &Tex) -> Vec<[f32; 4]> {
    let n = TILE as i32;
    let h = |x: i32, y: i32| t.height[(y.rem_euclid(n) * n + x.rem_euclid(n)) as usize];
    let mut out = Vec::with_capacity(TILE * TILE);
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

/// A drawn tile's texels, scaled up to `full` px by repeating them.
fn upscale(src: &[[f32; 4]], full: usize) -> Vec<[f32; 4]> {
    let k = full / TILE;
    (0..full * full).map(|i| src[(i / full / k) * TILE + (i % full) / k]).collect()
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
    (out, size.max(TILE))
}

pub fn build(images: &mut Assets<Image>) -> BlockAtlas {
    let drawn = draw_all();
    let (baked, full) = baked();
    let maps: Vec<Maps> = drawn
        .iter()
        .map(|(name, t)| match baked.get(*name).filter(|_| !t.glow) {
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
            None => Maps { color: upscale(&t.rgba, full), normal: upscale(&normal_of(t), full), rough: vec![[0.0, ROUGH * 255.0, 0.0, 255.0]; full * full] },
        })
        .collect();
    if full > TILE {
        info!("Block textures at {full} px ({} baked)", drawn.iter().filter(|(n, t)| !t.glow && baked.contains_key(*n)).count());
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
        tiles,
        icons,
    }
}
