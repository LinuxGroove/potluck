//! Low's sky. Low draws no atmosphere, so its sky is a dome of colours round
//! the camera worked out from the sun: deep blue overhead, the fog's colour
//! at the horizon (where the far hills fade into it), and gold and rose
//! toward a low sun. One opaque draw of a few thousand vertices, recoloured
//! only as the sun moves.

use bevy::asset::RenderAssetUsages;
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::prelude::*;

use super::{Clock, NotRaytraced};
use crate::noise::smooth;
use crate::player::MainCamera;

/// Inside the camera's reach (900); the clouds have faded out before it.
const RADIUS: f32 = 870.0;
/// Rings from straight down to straight up, closest round the horizon.
const RINGS: u32 = 32;
const SEGS: u32 = 64;

#[derive(Component)]
pub struct Dome;

#[derive(Resource)]
pub struct DomeLook {
    mesh: Handle<Mesh>,
    /// The time of day the colours were last worked out for.
    at: f32,
}

/// How high above the horizon ring `r` of `RINGS` is, -1 (straight down) to
/// 1 (straight up), as a sine: the rings bunch up round the horizon, where
/// the colours change fastest.
fn ring_height(r: u32) -> f32 {
    let t = r as f32 / RINGS as f32 * 2.0 - 1.0;
    t * t.abs()
}

pub(crate) fn mesh() -> Mesh {
    let mut pos = Vec::new();
    for r in 0..=RINGS {
        let y = ring_height(r);
        let flat = (1.0 - y * y).max(0.0).sqrt();
        for s in 0..=SEGS {
            let a = s as f32 / SEGS as f32 * std::f32::consts::TAU;
            pos.push([RADIUS * flat * a.cos(), RADIUS * y, RADIUS * flat * a.sin()]);
        }
    }
    let row = SEGS + 1;
    let mut idx = Vec::new();
    for r in 0..RINGS {
        for s in 0..SEGS {
            let (a, b) = (r * row + s, (r + 1) * row + s);
            idx.extend_from_slice(&[a, b, a + 1, a + 1, b, b + 1]);
        }
    }
    let n = pos.len();
    let normals: Vec<[f32; 3]> = pos.iter().map(|p| (-Vec3::from(*p) / RADIUS).into()).collect();
    // kept in the main world too: `follow` recolours it as the sun moves
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, vec![[1.0, 1.0, 1.0, 1.0]; n])
        .with_inserted_indices(Indices::U32(idx))
}

pub fn spawn(commands: &mut Commands, meshes: &mut Assets<Mesh>, mats: &mut Assets<StandardMaterial>) {
    let mat = mats.add(StandardMaterial { unlit: true, fog_enabled: false, double_sided: true, cull_mode: None, ..default() });
    let mesh = meshes.add(mesh());
    commands.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(mat), Transform::default(), Dome, NotShadowCaster, NotRaytraced));
    commands.insert_resource(DomeLook { mesh, at: -1.0 });
}

/// The sky's colour, in sRGB like the fog's, looking along `d` (unit length)
/// with the sun toward `sun`, at daylight `day` with the sun `low` (1 at
/// sunset) as `sky` works them out.
pub fn colour(d: Vec3, sun: Vec3, day: f32, low: f32) -> Vec3 {
    let horizon = super::fog_colour(day, low);
    let zenith = Vec3::new(0.015, 0.025, 0.07).lerp(Vec3::new(0.22, 0.43, 0.88), day);
    let mut c = horizon.lerp(zenith, smooth(0.0, 0.5, d.y).powf(0.65));
    // toward a low sun the horizon burns gold, with rose above and round it
    let set = low * day;
    let facing = Vec2::new(d.x, d.z).normalize_or_zero().dot(Vec2::new(sun.x, sun.z).normalize_or_zero()).max(0.0);
    let band = smooth(-0.08, 0.0, d.y) * (1.0 - smooth(0.0, 0.5, d.y));
    c = c.lerp(Vec3::new(0.86, 0.5, 0.56), set * 0.5 * band * facing.powf(1.5));
    c = c.lerp(Vec3::new(1.0, 0.6, 0.26), set * 0.85 * band * facing.powf(5.0));
    // and the air round the sun shines, as the fog does toward it
    let halo = d.dot(sun).max(0.0).powf(16.0) * smooth(-0.1, 0.05, sun.y);
    c + Vec3::new(1.0, 0.82, 0.55) * halo * (0.25 + 0.35 * set)
}

/// Keeps the dome round the camera and coloured for the time of day.
pub fn follow(
    clock: Res<Clock>,
    look: Option<ResMut<DomeLook>>,
    mut meshes: ResMut<Assets<Mesh>>,
    cam: Query<&GlobalTransform, With<MainCamera>>,
    mut dome: Query<&mut Transform, With<Dome>>,
) {
    let Some(mut look) = look else { return };
    let (Ok(cam), Ok(mut t)) = (cam.single(), dome.single_mut()) else { return };
    t.translation = cam.translation();
    if (clock.time - look.at).abs() <= 0.001 {
        return;
    }
    look.at = clock.time;
    let (sun, day, low) = super::sun_now(&clock);
    let Some(mut mesh) = meshes.get_mut(&look.mesh) else { return };
    // (never `attribute`, which panics on a mesh whose data has gone to the GPU)
    let pos: Vec<[f32; 3]> = mesh.try_attribute_option(Mesh::ATTRIBUTE_POSITION).ok().flatten().and_then(|p| p.as_float3()).map(|p| p.to_vec()).unwrap_or_default();
    if let Ok(Some(VertexAttributeValues::Float32x4(col))) = mesh.try_attribute_mut_option(Mesh::ATTRIBUTE_COLOR) {
        for (c, p) in col.iter_mut().zip(&pos) {
            let s = colour(Vec3::from(*p) / RADIUS, sun, day, low);
            *c = Color::srgb(s.x, s.y, s.z).to_linear().to_f32_array();
        }
    }
}
