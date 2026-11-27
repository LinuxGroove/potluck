//! Clouds: a soft layer high over the island, drifting with the wind, lit by
//! the time of day (white at noon, gold and rose at sunset, a dim blue at
//! night) and thinning out toward the horizon. One painted texture on a wide
//! disc that follows the player, so they cost one see-through draw on every
//! tier. They stay out of the ray traced scene, which would count them solid.

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::light::NotShadowCaster;
use bevy::math::Affine2;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use super::{Clock, NotRaytraced, SUNSET};
use crate::noise::{smooth, tnoise};
use crate::player::Me;

/// How high the clouds float (the world is 128 tall).
pub const HEIGHT: f32 = 205.0;
/// The disc's radius: inside the camera's reach (900) from the ground.
const RADIUS: f32 = 840.0;
/// Blocks one copy of the texture covers.
const TILE: f32 = 1000.0;
/// The wind, in blocks a second.
const WIND: Vec2 = Vec2::new(2.2, 0.9);
/// The texture's size.
const SIZE: u32 = 512;

#[derive(Component)]
pub struct Clouds;

#[derive(Resource)]
pub struct CloudLook {
    mat: Handle<StandardMaterial>,
    mesh: Handle<Mesh>,
    /// The time of day the glow toward the sun was last worked out for.
    glow_at: f32,
}

/// How cloudy a point of the texture is, 0 to 1; `u` and `v` run 0 to 1 and
/// the pattern tiles.
pub fn cover(u: f32, v: f32) -> f32 {
    let (x, y) = (u * 32.0, v * 32.0);
    // a slow warp rounds the big masses off into billows (it tiles too, so
    // the pattern still does); the small billows and wisps go on unwarped,
    // or they'd streak
    let (wx, wy) = (x + (tnoise(x, y, 5, 81) - 0.5) * 3.0, y + (tnoise(x, y, 5, 82) - 0.5) * 3.0);
    let n = 0.45 * tnoise(wx, wy, 4, 71) + 0.25 * tnoise(wx, wy, 8, 72) + 0.15 * tnoise(x, y, 16, 73) + 0.1 * tnoise(x, y, 32, 74) + 0.05 * tnoise(x, y, 64, 75);
    smooth(0.48, 0.66, n)
}

fn texture() -> Image {
    let mut data = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for j in 0..SIZE {
        for i in 0..SIZE {
            let (u, v) = (i as f32 / SIZE as f32, j as f32 / SIZE as f32);
            let c = cover(u, v);
            // thick middles are grey underneath, edges bright
            let shade = 1.0 - 0.36 * smooth(0.4, 1.0, c);
            let g = (shade * 255.0) as u8;
            data.extend_from_slice(&[g, g, (g as f32 * 1.02).min(255.0) as u8, (c.powf(0.8) * 235.0) as u8]);
        }
    }
    let mut image = Image::new(
        Extent3d { width: SIZE, height: SIZE, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        ..ImageSamplerDescriptor::linear()
    });
    image
}

/// The disc: rings out from the middle, see-through toward the rim, with
/// texture coordinates in tiles so the texture stays put in the world.
fn disc() -> Mesh {
    let (rings, segs) = (14u32, 48u32);
    let mut pos = vec![[0.0, 0.0, 0.0]];
    let mut uv = vec![[0.0, 0.0]];
    let mut col = vec![[1.0, 1.0, 1.0, 1.0]];
    for r in 1..=rings {
        let t = r as f32 / rings as f32;
        let radius = RADIUS * t * t.sqrt();
        // full overhead, gone at the rim, so the edge never shows
        let a = 1.0 - smooth(0.35, 1.0, radius / RADIUS);
        for s in 0..segs {
            let ang = s as f32 / segs as f32 * std::f32::consts::TAU;
            let (x, z) = (radius * ang.cos(), radius * ang.sin());
            pos.push([x, 0.0, z]);
            uv.push([x / TILE, z / TILE]);
            col.push([1.0, 1.0, 1.0, a]);
        }
    }
    let mut idx = Vec::new();
    for s in 0..segs {
        idx.extend_from_slice(&[0, 1 + (s + 1) % segs, 1 + s]);
    }
    for r in 0..rings - 1 {
        let (a, b) = (1 + r * segs, 1 + (r + 1) * segs);
        for s in 0..segs {
            let s1 = (s + 1) % segs;
            idx.extend_from_slice(&[a + s, a + s1, b + s, a + s1, b + s1, b + s]);
        }
    }
    let n = pos.len();
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, -1.0, 0.0]; n])
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, col)
        .with_inserted_indices(Indices::U32(idx))
}

pub fn spawn(commands: &mut Commands, meshes: &mut Assets<Mesh>, mats: &mut Assets<StandardMaterial>, images: &mut Assets<Image>) {
    if std::env::var("POTLUCK_OFF").unwrap_or_default().split(',').any(|o| o.trim() == "clouds") {
        return;
    }
    let mat = mats.add(StandardMaterial {
        base_color_texture: Some(images.add(texture())),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        fog_enabled: false,
        double_sided: true,
        cull_mode: None,
        ..default()
    });
    let mesh = meshes.add(disc());
    commands.spawn((
        Mesh3d(mesh.clone()),
        MeshMaterial3d(mat.clone()),
        Transform::from_xyz(0.0, HEIGHT, 0.0),
        Clouds,
        NotShadowCaster,
        NotRaytraced,
    ));
    commands.insert_resource(CloudLook { mat, mesh, glow_at: -1.0 });
}

/// The colour of the clouds for the time of day.
pub fn tint(day: f32, low: f32) -> Vec3 {
    // unlit, so brighter than white to stand out bright against the sky
    let noon = Vec3::splat(2.6);
    let night = Vec3::new(0.05, 0.065, 0.11);
    // gold underneath, rose at the edges, as the sun goes down
    let glow = SUNSET.lerp(Vec3::new(1.0, 0.55, 0.5), 0.35) * 2.0;
    night.lerp(noon, day).lerp(glow, 0.75 * low * day)
}

/// How much brighter a cloud at `at` (from the middle of the disc) shines
/// when the sun is low in the direction `sun` (flat, unit length): the ones
/// out toward the sun light up, the ones overhead don't.
pub fn glow(at: Vec2, sun: Vec2, low: f32) -> f32 {
    let r = at.length() / RADIUS;
    if r < 1e-3 {
        return 1.0;
    }
    let facing = (at.normalize().dot(sun)).max(0.0);
    1.0 + 1.6 * low * facing * facing * r.min(1.0).sqrt()
}

/// Keeps the clouds over the player and drifting.
pub fn drift(
    time: Res<Time>,
    clock: Res<Clock>,
    me: Res<Me>,
    look: Option<ResMut<CloudLook>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut disc: Query<&mut Transform, With<Clouds>>,
) {
    let Some(mut look) = look else { return };
    let Ok(mut t) = disc.single_mut() else { return };
    t.translation = Vec3::new(me.pos.x, HEIGHT, me.pos.z);
    let a = std::f32::consts::TAU * (clock.time - 0.25);
    let up = a.sin();
    let low = (1.0 - smooth(0.02, 0.4, up)) * clock.daylight();
    // the glow toward a low sun, worked out again as the sun moves
    if (clock.time - look.glow_at).abs() > 0.002 {
        look.glow_at = clock.time;
        let sun = Vec2::new(a.cos(), -0.35).normalize();
        if let Some(mut mesh) = meshes.get_mut(&look.mesh) {
            let pos: Vec<[f32; 3]> = mesh.attribute(Mesh::ATTRIBUTE_POSITION).and_then(|p| p.as_float3()).map(|p| p.to_vec()).unwrap_or_default();
            if let Some(bevy::mesh::VertexAttributeValues::Float32x4(col)) = mesh.attribute_mut(Mesh::ATTRIBUTE_COLOR) {
                for (c, p) in col.iter_mut().zip(&pos) {
                    let g = glow(Vec2::new(p[0], p[2]), sun, low);
                    // warmer as well as brighter
                    *c = [g, 1.0 + (g - 1.0) * 0.75, 1.0 + (g - 1.0) * 0.55, c[3]];
                }
            }
        }
    }
    let Some(mut m) = mats.get_mut(&look.mat) else { return };
    let off = (Vec2::new(me.pos.x, me.pos.z) - WIND * time.elapsed_secs()) / TILE;
    m.uv_transform = Affine2::from_translation(off);
    let low = 1.0 - smooth(0.02, 0.4, up);
    let c = tint(clock.daylight(), low);
    m.base_color = Color::linear_rgba(c.x, c.y, c.z, 0.55 + 0.4 * clock.daylight());
}
