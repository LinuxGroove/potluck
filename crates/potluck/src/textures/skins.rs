//! Creatures' skins: small tiling textures painted from code, in grey so a
//! part's own colour tints them. Which one a part wears comes from its name
//! (`Skin::of`): fur for most, fluff for fleece and moss, plates for shells,
//! a smooth sheen for jelly, fine streaks for horns and beaks, a weave for
//! hats and belts, bare skin for faces and hands, and glossy eyes. Each has a height (for the normal map)
//! and a roughness, like the blocks' art.

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use crate::noise::hash2;

/// Texels across a skin's tile.
pub const SIZE: usize = 64;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Skin {
    Fur,
    Fluff,
    Shell,
    Smooth,
    Horn,
    Cloth,
    Bare,
    Eye,
}

impl Skin {
    pub const ALL: [Skin; 8] = [Skin::Fur, Skin::Fluff, Skin::Shell, Skin::Smooth, Skin::Horn, Skin::Cloth, Skin::Bare, Skin::Eye];

    /// The skin a part wears, from its name.
    pub fn of(name: &str) -> Skin {
        let has = |words: &[&str]| words.iter().any(|w| name.contains(w));
        if has(&["eye", "pupil"]) {
            Skin::Eye
        } else if has(&["face", "hand", "bare"]) {
            Skin::Bare
        } else if has(&["shell", "plate", "scale"]) {
            Skin::Shell
        } else if has(&["fleece", "wool", "moss", "tuft", "beard", "mane", "fluff"]) {
            Skin::Fluff
        } else if has(&["jelly", "bulb", "glow", "antenna", "slime", "bubble", "light"]) {
            Skin::Smooth
        } else if has(&["horn", "claw", "bill", "beak", "nose", "snout", "tusk", "hoof", "tooth", "teeth"]) {
            Skin::Horn
        } else if has(&["hat", "belt", "boot", "saddle", "coat", "cloth", "scarf", "wheel", "strap", "bag"]) {
            Skin::Cloth
        } else {
            Skin::Fur
        }
    }

    /// How many blocks one tile covers on a part.
    pub fn tile(self) -> f32 {
        match self {
            Skin::Fur | Skin::Horn | Skin::Cloth | Skin::Bare => 0.5,
            Skin::Fluff => 0.6,
            Skin::Shell => 0.8,
            Skin::Smooth | Skin::Eye => 1.0,
        }
    }

    /// Glossy or matt, where the texture doesn't say.
    pub fn roughness(self) -> f32 {
        match self {
            Skin::Eye => 0.08,
            Skin::Smooth => 0.3,
            Skin::Bare => 0.62,
            _ => 1.0,
        }
    }

    /// Whether parts wear the soft shading that darkens toward their
    /// underside; eyes and glowing jelly stay even.
    pub fn shaded(self) -> bool {
        !matches!(self, Skin::Eye | Skin::Smooth)
    }
}

/// Tiling value noise with its own number of cells across and down, so it
/// can stretch into strands.
fn vn(x: f32, y: f32, cx: i32, cy: i32, s: i32) -> f32 {
    let fx = x / SIZE as f32 * cx as f32;
    let fy = y / SIZE as f32 * cy as f32;
    let (xi, yi) = (fx.floor(), fy.floor());
    let f = |t: f32| t * t * (3.0 - 2.0 * t);
    let (xf, yf) = (f(fx - xi), f(fy - yi));
    let (xi, yi) = (xi as i32, yi as i32);
    let g = |i: i32, j: i32| hash2(i.rem_euclid(cx), j.rem_euclid(cy), s);
    let (a, b, c, d) = (g(xi, yi), g(xi + 1, yi), g(xi, yi + 1), g(xi + 1, yi + 1));
    a + (b - a) * xf + (c - a) * yf + (a - b - c + d) * xf * yf
}

/// The nearest and second nearest of `n` x `n` jittered points, tiling.
fn cells(x: f32, y: f32, n: i32, s: i32) -> (f32, f32, i32) {
    let fx = x / SIZE as f32 * n as f32;
    let fy = y / SIZE as f32 * n as f32;
    let (xi, yi) = (fx.floor() as i32, fy.floor() as i32);
    let (mut d1, mut d2, mut id) = (9.0f32, 9.0f32, 0);
    for j in -1..=1 {
        for i in -1..=1 {
            let (cx, cy) = (xi + i, yi + j);
            let (wx, wy) = (cx.rem_euclid(n), cy.rem_euclid(n));
            let px = cx as f32 + 0.15 + 0.7 * hash2(wx, wy, s);
            let py = cy as f32 + 0.15 + 0.7 * hash2(wx, wy, s + 1);
            let d = ((fx - px).powi(2) + (fy - py).powi(2)).sqrt();
            if d < d1 {
                d2 = d1;
                d1 = d;
                id = wy * n + wx;
            } else if d < d2 {
                d2 = d;
            }
        }
    }
    (d1, d2, id)
}

/// One texel: brightness (0 to 1, tinted by the part), height and roughness.
fn texel(skin: Skin, x: f32, y: f32) -> (f32, f32, f32) {
    match skin {
        Skin::Fur => {
            // short strands down the part, in soft patches
            let strand = vn(x, y, 32, 5, 11);
            let fine = vn(x, y, 64, 9, 12);
            let patch = vn(x, y, 4, 4, 13);
            let h = 0.6 * strand + 0.4 * fine;
            (0.56 + 0.32 * h + 0.12 * patch, h, 0.85 + 0.1 * (1.0 - h))
        }
        Skin::Fluff => {
            // round curls with deep shade between them
            let (d1, _, id) = cells(x, y, 6, 21);
            let curl = (1.0 - d1 * 1.35).clamp(0.0, 1.0);
            let ring = 0.5 + 0.5 * (d1 * 18.0 + hash2(id, 0, 22) * 6.0).sin();
            let h = curl * (0.85 + 0.15 * ring);
            (0.52 + 0.48 * h.powf(0.7), h, 0.95)
        }
        Skin::Shell => {
            // plates with dark seams and growth rings
            let (d1, d2, id) = cells(x, y, 3, 31);
            let seam = ((d2 - d1) * 3.0).clamp(0.0, 1.0);
            let rings = 0.5 + 0.5 * (d1 * 26.0).sin();
            let shade = 0.85 + 0.15 * hash2(id, 1, 32);
            let h = seam.powf(0.5) * (0.9 + 0.1 * rings);
            ((0.45 + 0.55 * seam.powf(0.35)) * shade * (0.92 + 0.08 * rings), h, 0.42 + 0.2 * (1.0 - seam))
        }
        Skin::Smooth => {
            // a soft sheen with a few faint spots
            let spots = vn(x, y, 5, 5, 41);
            let h = 0.5 + 0.1 * vn(x, y, 3, 3, 42);
            (0.9 + 0.1 * (1.0 - (spots - 0.7).max(0.0) * 3.0), h, 0.3)
        }
        Skin::Horn => {
            // fine streaks across
            let streak = vn(x, y, 3, 40, 51);
            let h = 0.4 + 0.6 * streak;
            (0.78 + 0.22 * streak, h, 0.45 + 0.15 * (1.0 - streak))
        }
        Skin::Cloth => {
            // a plain weave
            let (u, v) = (x * std::f32::consts::TAU / 8.0, y * std::f32::consts::TAU / 8.0);
            let over = if ((x / 4.0).floor() as i32 + (y / 4.0).floor() as i32) % 2 == 0 { u.sin().abs() } else { v.sin().abs() };
            let fuzz = vn(x, y, 32, 32, 61);
            let h = 0.7 * over + 0.3 * fuzz;
            (0.8 + 0.2 * h, h, 0.92)
        }
        Skin::Bare => {
            // even skin, faintly mottled, with a fine grain
            let mottle = vn(x, y, 4, 4, 71);
            let grain = vn(x, y, 32, 32, 72);
            let h = 0.5 + 0.12 * grain;
            (0.9 + 0.06 * mottle + 0.04 * grain, h, 0.55 + 0.12 * mottle)
        }
        Skin::Eye => (1.0, 0.5, 0.08),
    }
}

/// A skin's three textures: colour (grey, sRGB), normals and roughness.
pub struct SkinImages {
    pub color: Image,
    pub normal: Image,
    pub rough: Image,
}

fn image(data: Vec<u8>, format: TextureFormat) -> Image {
    let mut img = Image::new(
        Extent3d { width: SIZE as u32, height: SIZE as u32, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        format,
        RenderAssetUsages::RENDER_WORLD,
    );
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        ..ImageSamplerDescriptor::nearest()
    });
    img
}

/// Paints a skin.
pub fn paint(skin: Skin) -> SkinImages {
    let n = SIZE;
    let px: Vec<(f32, f32, f32)> = (0..n * n).map(|i| texel(skin, (i % n) as f32 + 0.5, (i / n) as f32 + 0.5)).collect();
    let h = |x: i32, y: i32| px[(y.rem_euclid(n as i32) as usize) * n + x.rem_euclid(n as i32) as usize].1;
    let bump = match skin {
        Skin::Fluff | Skin::Shell => 2.2,
        Skin::Fur | Skin::Cloth => 1.4,
        _ => 0.8,
    };
    let mut color = Vec::with_capacity(n * n * 4);
    let mut normal = Vec::with_capacity(n * n * 4);
    let mut rough = Vec::with_capacity(n * n * 4);
    for (i, &(c, _, r)) in px.iter().enumerate() {
        let (x, y) = ((i % n) as i32, (i / n) as i32);
        let g = (c.clamp(0.0, 1.0) * 255.0) as u8;
        color.extend_from_slice(&[g, g, g, 255]);
        let (dx, dy) = ((h(x + 1, y) - h(x - 1, y)) * bump, (h(x, y + 1) - h(x, y - 1)) * bump);
        let v = Vec3::new(-dx, dy, 1.0).normalize();
        normal.extend_from_slice(&[((v.x * 0.5 + 0.5) * 255.0) as u8, ((v.y * 0.5 + 0.5) * 255.0) as u8, ((v.z * 0.5 + 0.5) * 255.0) as u8, 255]);
        rough.extend_from_slice(&[0, (r.clamp(0.0, 1.0) * 255.0) as u8, 0, 255]);
    }
    SkinImages {
        color: image(color, TextureFormat::Rgba8UnormSrgb),
        normal: image(normal, TextureFormat::Rgba8Unorm),
        rough: image(rough, TextureFormat::Rgba8Unorm),
    }
}
