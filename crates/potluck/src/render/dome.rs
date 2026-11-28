//! Low's sky. Low draws no atmosphere, so its sky is a dome of colours round
//! the camera worked out from the sun: deep blue overhead, the fog's colour
//! at the horizon (where the far hills fade into it), and gold and rose
//! toward a low sun. One opaque draw of a few thousand vertices, recoloured
//! only as the sun moves.

use bevy::asset::RenderAssetUsages;
use bevy::camera::Exposure;
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::pbr::DistanceFog;
use bevy::prelude::*;

use super::{Clock, Gloom, NotRaytraced, Sun};
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
    /// And the sun's glow.
    glow: Glow,
    /// And the weather's.
    gloom: Gloom,
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
    commands.insert_resource(DomeLook { mesh, at: -1.0, glow: Glow::default(), gloom: Gloom::default() });
}

/// The sky's colour, in sRGB like the fog's, looking along `d` (unit length)
/// with the sun toward `sun`, at daylight `day` with the sun `low` (1 at
/// sunset) as `sky` works them out. The glow round the sun comes on top
/// (`Glow`).
pub fn colour(d: Vec3, sun: Vec3, day: f32, low: f32) -> Vec3 {
    colour_in(d, sun, day, low, &Gloom::default())
}

/// The same under the weather: greying over as it clouds up, to the fog's
/// grey at the horizon and a brighter grey overhead, with no glow round a
/// sun that's behind the cloud.
pub(crate) fn colour_in(d: Vec3, sun: Vec3, day: f32, low: f32, gloom: &Gloom) -> Vec3 {
    let horizon = gloom.fog_colour(day, low);
    let clear = Vec3::new(0.015, 0.025, 0.07).lerp(Vec3::new(0.36, 0.56, 0.96), day);
    let zenith = clear.lerp((gloom.grey(day, low) * 1.15).min(Vec3::ONE), gloom.cover);
    let c = horizon.lerp(zenith, smooth(0.0, 0.5, d.y).powf(0.65));
    // toward a low sun the sky over the horizon burns gold, with rose above
    // and round it. Below the horizon it stays the fog's colour, as the far
    // sea is: the sea stops where it meets the dome, a little below the
    // horizon, and any gold there drew a hard line along it
    let set = low * day * (1.0 - gloom.cover);
    let facing = Vec2::new(d.x, d.z).normalize_or_zero().dot(Vec2::new(sun.x, sun.z).normalize_or_zero()).max(0.0);
    let band = smooth(0.0, 0.05, d.y) * (1.0 - smooth(0.05, 0.5, d.y));
    c.lerp(Vec3::new(0.86, 0.5, 0.56), set * 0.5 * band * facing.powf(1.5)).lerp(Vec3::new(1.0, 0.6, 0.26), set * 0.85 * band * facing.powf(5.0))
}

/// The glow the fog takes on toward the sun (`DistanceFog`'s
/// `directional_light_color`), worked out as Bevy's fog shader does it, so
/// the sky round the sun shines just as the far hills and sea under it do.
#[derive(Clone, Copy, PartialEq, Default, Debug)]
pub struct Glow {
    /// Toward the light (the sun, or the moon at night).
    pub to: Vec3,
    /// Linear colour at its brightest, exposure and all.
    pub colour: Vec3,
    pub exponent: f32,
}

impl Glow {
    pub fn of(light: &DirectionalLight, to: Vec3, exposure: f32, fog: &DistanceFog) -> Self {
        let dl = fog.directional_light_color.to_linear();
        let colour = light.color.to_linear().to_vec3() * light.illuminance * exposure * dl.to_vec3() * dl.alpha;
        Glow { to, colour, exponent: fog.directional_light_exponent }
    }

    /// The glow added looking along `d`, linear.
    pub fn at(&self, d: Vec3) -> Vec3 {
        self.colour * d.dot(self.to).max(0.0).powf(self.exponent)
    }

    /// Whether it has moved or changed enough to colour the sky again.
    fn differs(&self, o: &Glow) -> bool {
        self.to.distance(o.to) > 0.002 || (self.colour - o.colour).abs().max_element() > 0.03 * self.colour.max_element().max(0.05)
    }
}

/// Keeps the dome round the camera and coloured for the time of day.
pub fn follow(
    clock: Res<Clock>,
    look: Option<ResMut<DomeLook>>,
    mut meshes: ResMut<Assets<Mesh>>,
    cam: Query<(&GlobalTransform, &Exposure, &DistanceFog), With<MainCamera>>,
    sun: Query<(&GlobalTransform, &DirectionalLight), With<Sun>>,
    mut dome: Query<&mut Transform, With<Dome>>,
    weather: Res<crate::sky::Weather>,
    settings: Res<crate::settings::Settings>,
) {
    let Some(mut look) = look else { return };
    let (Ok((cam, exposure, fog)), Ok(mut t)) = (cam.single(), dome.single_mut()) else { return };
    t.translation = cam.translation();
    let glow = sun.single().map(|(st, l)| Glow::of(l, st.back().into(), exposure.exposure(), fog)).unwrap_or_default();
    let gloom = Gloom::of(&weather, settings.reduce_flashing);
    let changed = [gloom.cover - look.gloom.cover, gloom.fog - look.gloom.fog, gloom.storm - look.gloom.storm, gloom.glow - look.gloom.glow].iter().any(|d| d.abs() > 0.004);
    if (clock.time - look.at).abs() <= 0.001 && !glow.differs(&look.glow) && !changed {
        return;
    }
    look.at = clock.time;
    look.glow = glow;
    look.gloom = gloom;
    let (sun, day, low) = super::sun_now(&clock);
    let Some(mut mesh) = meshes.get_mut(&look.mesh) else { return };
    // (never `attribute`, which panics on a mesh whose data has gone to the GPU)
    let pos: Vec<[f32; 3]> = mesh.try_attribute_option(Mesh::ATTRIBUTE_POSITION).ok().flatten().and_then(|p| p.as_float3()).map(|p| p.to_vec()).unwrap_or_default();
    if let Ok(Some(VertexAttributeValues::Float32x4(col))) = mesh.try_attribute_mut_option(Mesh::ATTRIBUTE_COLOR) {
        for (c, p) in col.iter_mut().zip(&pos) {
            let d = Vec3::from(*p) / RADIUS;
            let s = colour_in(d, sun, day, low, &gloom);
            let lin = Color::srgb(s.x, s.y, s.z).to_linear().to_vec3() + glow.at(d);
            *c = [lin.x, lin.y, lin.z, 1.0];
        }
    }
}
