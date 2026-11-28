//! Clouds: a soft layer high over the island, drifting with the wind, lit by
//! the time of day (white at noon, gold and rose at sunset, a dim blue at
//! night) and thinning out toward the horizon. One painted texture on a wide
//! disc that follows the player, so they cost one see-through draw on every
//! tier. They stay out of the ray traced scene, which would count them solid.
//!
//! As the weather (`sky::Weather`) clouds over, a lower deck comes in under
//! them: heavy billows, then a grey sheet behind them reaching out to the
//! horizon, where the fog takes it. Both are drawn only while there's any
//! cloud, and both grey with the fog and darken in a storm (`Gloom`).

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::light::NotShadowCaster;
use bevy::math::Affine2;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use super::{Clock, Gloom, NotRaytraced, SUNSET};
use crate::noise::{smooth, tnoise};
use crate::player::Me;
use crate::sky::Weather;

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

/// The deck under the clouds: the grey sheet, and the billows under it.
const SHEET_HEIGHT: f32 = 190.0;
const BILLOWS_HEIGHT: f32 = 180.0;
/// The deck's radius: far enough that its rim is near the horizon (the
/// fog has it long before).
const DECK_RADIUS: f32 = 6000.0;
/// Blocks one copy of the billows' texture covers.
const BILLOWS_TILE: f32 = 1600.0;

#[derive(Component)]
pub struct Clouds;

/// The grey sheet (false) or the billows (true) of the deck.
#[derive(Component)]
pub struct Deck(bool);

#[derive(Resource)]
pub struct CloudLook {
    mat: Handle<StandardMaterial>,
    mesh: Handle<Mesh>,
    /// The deck's sheet and billows.
    sheet: Handle<StandardMaterial>,
    billows: Handle<StandardMaterial>,
    /// How far the high clouds and the billows have drifted.
    drift: (Vec2, Vec2),
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

/// How thick the heavy billows of a cloudy day are at a point, 0 to 1:
/// bigger masses than `cover`, mostly joined up, with gaps between.
pub fn heavy(u: f32, v: f32) -> f32 {
    let (x, y) = (u * 32.0, v * 32.0);
    let (wx, wy) = (x + (tnoise(x, y, 4, 91) - 0.5) * 4.0, y + (tnoise(x, y, 4, 92) - 0.5) * 4.0);
    let n = 0.5 * tnoise(wx, wy, 4, 93) + 0.28 * tnoise(wx, wy, 8, 94) + 0.14 * tnoise(x, y, 16, 95) + 0.08 * tnoise(x, y, 32, 96);
    smooth(0.36, 0.6, n)
}

fn texture() -> Image {
    painted(|u, v| {
        let c = cover(u, v);
        // thick middles are grey underneath, edges bright
        (1.0 - 0.36 * smooth(0.4, 1.0, c), c.powf(0.8) * 0.92)
    })
}

/// The billows' texture: darker where they're thickest, as rain clouds are
/// underneath.
fn heavy_texture() -> Image {
    painted(|u, v| {
        let c = heavy(u, v);
        (1.0 - 0.42 * smooth(0.3, 1.0, c), c.powf(0.7))
    })
}

/// A tiling cloud texture from (shade, cover) at each point.
fn painted(at: impl Fn(f32, f32) -> (f32, f32)) -> Image {
    let mut data = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for j in 0..SIZE {
        for i in 0..SIZE {
            let (shade, a) = at(i as f32 / SIZE as f32, j as f32 / SIZE as f32);
            let g = (shade * 255.0) as u8;
            data.extend_from_slice(&[g, g, (g as f32 * 1.02).min(255.0) as u8, (a.clamp(0.0, 1.0) * 255.0) as u8]);
        }
    }
    let mut image = Image::new(
        Extent3d { width: SIZE, height: SIZE, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_descriptor.label = Some("cloud");
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        ..ImageSamplerDescriptor::linear()
    });
    image
}

/// The disc: rings out from the middle, see-through toward the rim, with
/// texture coordinates in tiles so the texture stays put in the world.
pub(crate) fn disc() -> Mesh {
    disc_of(RADIUS, 14, TILE, 0.35)
}

/// A disc `radius` across in `rings`, its texture `tile` blocks a copy,
/// fading out from `fade` of the way to its rim.
fn disc_of(outer: f32, rings: u32, tile: f32, fade: f32) -> Mesh {
    let segs = 48u32;
    let mut pos = vec![[0.0, 0.0, 0.0]];
    let mut uv = vec![[0.0, 0.0]];
    let mut col = vec![[1.0, 1.0, 1.0, 1.0]];
    for r in 1..=rings {
        let t = r as f32 / rings as f32;
        let radius = outer * t * t.sqrt();
        // full overhead, gone at the rim, so the edge never shows
        let a = 1.0 - smooth(fade, 1.0, radius / outer);
        for s in 0..segs {
            let ang = s as f32 / segs as f32 * std::f32::consts::TAU;
            let (x, z) = (radius * ang.cos(), radius * ang.sin());
            pos.push([x, 0.0, z]);
            uv.push([x / tile, z / tile]);
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
    // kept in the main world too: `drift` recolours it as the sun moves
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
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
    // the deck: fogged like the land, so out toward the horizon it turns
    // into the fog the far hills fade into
    let deck = |texture: Option<Handle<Image>>| StandardMaterial {
        base_color_texture: texture,
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        double_sided: true,
        cull_mode: None,
        ..default()
    };
    let sheet = mats.add(deck(None));
    let billows = mats.add(deck(Some(images.add(heavy_texture()))));
    let deck_mesh = meshes.add(disc_of(DECK_RADIUS, 24, BILLOWS_TILE, 0.55));
    for (top, m, h) in [(false, &sheet, SHEET_HEIGHT), (true, &billows, BILLOWS_HEIGHT)] {
        commands.spawn((
            Mesh3d(deck_mesh.clone()),
            MeshMaterial3d(m.clone()),
            Transform::from_xyz(0.0, h, 0.0),
            Visibility::Hidden,
            Deck(top),
            bevy::camera::visibility::NoFrustumCulling,
            NotShadowCaster,
            NotRaytraced,
        ));
    }
    commands.insert_resource(CloudLook { mat, mesh, sheet, billows, drift: (Vec2::ZERO, Vec2::ZERO), glow_at: -1.0 });
}

/// How much of the deck shows under this weather: the grey sheet, then the
/// billows. The billows come first, with gaps of sky between them; the
/// sheet fills those in as it clouds right over, or as fog comes down.
pub(crate) fn deck(gloom: &Gloom) -> (f32, f32) {
    let sheet = smooth(0.45, 0.95, gloom.cover);
    let billows = smooth(0.1, 0.6, gloom.cover) * (1.0 - 0.6 * smooth(0.2, 0.8, gloom.fog));
    (sheet, billows)
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
    mut disc: Query<&mut Transform, (With<Clouds>, Without<Deck>)>,
    mut decks: Query<(&Deck, &mut Transform, &mut Visibility), Without<Clouds>>,
    weather: Res<Weather>,
    settings: Res<crate::settings::Settings>,
) {
    let Some(mut look) = look else { return };
    let Ok(mut t) = disc.single_mut() else { return };
    t.translation = Vec3::new(me.pos.x, HEIGHT, me.pos.z);
    let gloom = Gloom::of(&weather, settings.reduce_flashing);
    let (sheet, billows) = deck(&gloom);
    let shown = [sheet, billows];
    for (d, mut t, mut v) in &mut decks {
        t.translation = Vec3::new(me.pos.x, if d.0 { BILLOWS_HEIGHT } else { SHEET_HEIGHT }, me.pos.z);
        v.set_if_neq(if shown[d.0 as usize] > 0.003 { Visibility::Inherited } else { Visibility::Hidden });
    }
    let a = std::f32::consts::TAU * (clock.time - 0.25);
    let up = a.sin();
    let low = (1.0 - smooth(0.02, 0.4, up)) * clock.daylight();
    // the glow toward a low sun, worked out again as the sun moves
    if (clock.time - look.glow_at).abs() > 0.002 {
        look.glow_at = clock.time;
        let sun = Vec2::new(a.cos(), -0.35).normalize();
        if let Some(mut mesh) = meshes.get_mut(&look.mesh) {
            // (never `attribute`, which panics on a mesh whose data has gone to the GPU)
            let pos: Vec<[f32; 3]> = mesh.try_attribute_option(Mesh::ATTRIBUTE_POSITION).ok().flatten().and_then(|p| p.as_float3()).map(|p| p.to_vec()).unwrap_or_default();
            if let Ok(Some(bevy::mesh::VertexAttributeValues::Float32x4(col))) = mesh.try_attribute_mut_option(Mesh::ATTRIBUTE_COLOR) {
                for (c, p) in col.iter_mut().zip(&pos) {
                    let g = glow(Vec2::new(p[0], p[2]), sun, low);
                    // warmer as well as brighter
                    *c = [g, 1.0 + (g - 1.0) * 0.75, 1.0 + (g - 1.0) * 0.55, c[3]];
                }
            }
        }
    }
    let low = 1.0 - smooth(0.02, 0.4, up);
    let day = clock.daylight();
    // the weather's wind blows the clouds along faster, with the high
    // ones going their own way a little
    let wind = WIND + weather.wind * 1.5;
    let drift = look_drift(&mut look, time.delta_secs(), wind);
    if let Some(mut m) = mats.get_mut(&look.mat) {
        let off = (Vec2::new(me.pos.x, me.pos.z) - drift.0) / TILE;
        m.uv_transform = Affine2::from_translation(off);
        let grey = linear(gloom.grey(day, low)) * 2.0;
        let c = tint(day, low).lerp(grey, 0.6 * gloom.cover);
        // hidden by the deck as it clouds over, and lost in the fog
        let a = (0.55 + 0.4 * day) * (1.0 - smooth(0.6, 0.95, gloom.cover)) * (1.0 - smooth(0.1, 0.6, gloom.fog));
        m.base_color = Color::linear_rgba(c.x, c.y, c.z, a);
    }
    // overhead the deck is brighter than its grey out toward the horizon,
    // as an overcast sky is
    let grey = linear(gloom.grey(day, low));
    if let Some(mut m) = mats.get_mut(&look.sheet) {
        let c = grey * 1.5;
        m.base_color = Color::linear_rgba(c.x, c.y, c.z, sheet);
    }
    if let Some(mut m) = mats.get_mut(&look.billows) {
        let off = (Vec2::new(me.pos.x, me.pos.z) - drift.1) / BILLOWS_TILE;
        m.uv_transform = Affine2::from_translation(off);
        let c = grey * 1.45;
        m.base_color = Color::linear_rgba(c.x, c.y, c.z, billows);
    }
}

/// How far the high clouds and the billows have drifted, blocks along x and
/// z: added up a frame at a time, so a change of wind never jumps them.
fn look_drift(look: &mut CloudLook, dt: f32, wind: Vec2) -> (Vec2, Vec2) {
    look.drift.0 += wind * dt;
    look.drift.1 += (wind * 1.4 + Vec2::new(0.4, -0.3)) * dt;
    // kept small, a whole number of tiles at a time, so they stay exact
    for (d, tile) in [(&mut look.drift.0, TILE), (&mut look.drift.1, BILLOWS_TILE)] {
        *d = (*d / tile).fract() * tile;
    }
    look.drift
}

fn linear(s: Vec3) -> Vec3 {
    Color::srgb(s.x, s.y, s.z).to_linear().to_vec3()
}
