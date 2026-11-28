//! Rain and snow falling round the player, as the weather says
//! (`sky::Weather`): thousands of streaks or flakes in one draw each, moved
//! on the graphics card (`fall.wgsl`), so all that changes each frame is a
//! few numbers. They slant with the wind and stop at roofs, trees and water
//! (`Roofs`, a map of the blocks round the player), and stay out of the
//! shadows and the ray traced scene. Nothing moves the camera.

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, MeshVertexBufferLayoutRef, PrimitiveTopology};
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, Extent3d, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError, TextureDimension, TextureFormat};
use bevy::shader::ShaderRef;

use super::{Clock, NotRaytraced, Tier};
use crate::player::{MainCamera, Me};
use crate::sky::Weather;

const SHADER: Handle<Shader> = bevy::asset::uuid_handle!("a4c1e7d2-5b38-4f96-8e0a-71d3c9b25f46");

/// The roof map's size, in blocks each way, round the player.
pub const ROOFS: i32 = 64;
/// How big the box drops wrap round in is, in blocks.
const RAIN_BOX: f32 = 26.0;
const SNOW_BOX: f32 = 22.0;
/// The most drops and flakes there are (Low has half).
const DROPS: usize = 5000;
const FLAKES: usize = 6000;

/// What the shader's told each frame (see `fall.wgsl`).
#[derive(Clone, Copy, Default, ShaderType, Debug)]
pub struct FallUniform {
    pub view_proj: Mat4,
    pub eye: Vec4,
    pub wind: Vec4,
    pub look: Vec4,
    pub shape: Vec4,
    pub roofs: Vec4,
}

#[derive(Asset, AsBindGroup, TypePath, Clone, Debug)]
pub struct FallMaterial {
    #[uniform(0)]
    pub fall: FallUniform,
    #[texture(1, sample_type = "float", filterable = false)]
    pub roofs: Handle<Image>,
}

impl Material for FallMaterial {
    fn vertex_shader() -> ShaderRef {
        SHADER.into()
    }

    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    fn specialize(_: &MaterialPipeline, descriptor: &mut RenderPipelineDescriptor, _: &MeshVertexBufferLayoutRef, _: MaterialPipelineKey<Self>) -> Result<(), SpecializedMeshPipelineError> {
        // they turn to face the eye, whichever way round that leaves them
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

/// Rain (or snow) falling round the player.
#[derive(Component)]
pub struct Falling {
    pub snow: bool,
    mat: Handle<FallMaterial>,
}

/// Where rain and snow stop round the player (`Voxels::roof`), one texel a
/// block, made again as they move and now and then for what's built.
#[derive(Resource)]
pub struct Roofs {
    image: Handle<Image>,
    /// Its north-west corner, once it has been made.
    corner: Option<IVec2>,
    since: f32,
}

/// Its shader, and the drawing, for the game (not the server).
pub fn add(app: &mut App) {
    bevy::asset::load_internal_asset!(app, SHADER, "fall.wgsl", Shader::from_wgsl);
    app.add_plugins(MaterialPlugin::<FallMaterial>::default()).add_systems(Update, (roofs, fall).chain().after(crate::Sim));
}

/// `n` quads, each at a place in the box (0 to 1 each way), with its share
/// of the most that falls and a phase for drifting.
pub fn quads(n: usize, seed: u32) -> Mesh {
    let mut state = seed;
    let mut next = || {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (state >> 8) as f32 / (1u32 << 24) as f32
    };
    let (mut pos, mut uv, mut uv_b, mut idx) = (Vec::with_capacity(n * 4), Vec::with_capacity(n * 4), Vec::with_capacity(n * 4), Vec::with_capacity(n * 6));
    for i in 0..n as u32 {
        let at = [next(), next(), next()];
        // evenly spread shares, so a little rain is a little of the drops
        let share = (i as f32 + 0.5) / n as f32;
        let phase = next();
        for c in [[-0.5, 0.0], [0.5, 0.0], [0.5, 1.0], [-0.5, 1.0]] {
            pos.push(at);
            uv.push(c);
            uv_b.push([share, phase]);
        }
        let k = i * 4;
        idx.extend_from_slice(&[k, k + 1, k + 2, k, k + 2, k + 3]);
    }
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_1, uv_b)
        .with_inserted_indices(Indices::U32(idx))
}

fn roof_image() -> Image {
    let n = ROOFS as u32;
    let mut image = Image::new_fill(Extent3d { width: n, height: n, depth_or_array_layers: 1 }, TextureDimension::D2, &(-1.0e6f32).to_le_bytes(), TextureFormat::R32Float, RenderAssetUsages::default());
    image.texture_descriptor.label = Some("roofs");
    image
}

pub fn setup(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut mats: ResMut<Assets<FallMaterial>>, mut images: ResMut<Assets<Image>>, settings: Res<crate::settings::Settings>) {
    let image = images.add(roof_image());
    let low = Tier::of(&settings.lighting) == Tier::Low;
    for (snow, n, seed) in [(false, DROPS, 17), (true, FLAKES, 29)] {
        let n = if low { n / 2 } else { n };
        let mat = mats.add(FallMaterial { fall: FallUniform::default(), roofs: image.clone() });
        commands.spawn((
            Mesh3d(meshes.add(quads(n, seed))),
            MeshMaterial3d(mat.clone()),
            Transform::IDENTITY,
            Visibility::Hidden,
            Falling { snow, mat },
            NoFrustumCulling,
            NotShadowCaster,
            NotRaytraced,
        ));
    }
    commands.insert_resource(Roofs { image, corner: None, since: 0.0 });
}

/// Makes the roof map again when the player has moved a few blocks from
/// its middle, or every couple of seconds while anything falls.
pub fn roofs(time: Res<Time>, me: Res<Me>, weather: Res<Weather>, voxels: Res<crate::world::Voxels>, roofs: Option<ResMut<Roofs>>, mut images: ResMut<Assets<Image>>) {
    let Some(mut roofs) = roofs else { return };
    if weather.rain.max(weather.snow) <= 0.0 {
        return;
    }
    roofs.since += time.delta_secs();
    let corner = me.pos.xz().floor().as_ivec2() - IVec2::splat(ROOFS / 2);
    if roofs.corner.is_some_and(|c| (corner - c).abs().max_element() < 8) && roofs.since < 2.0 {
        return;
    }
    roofs.corner = Some(corner);
    roofs.since = 0.0;
    let Some(mut image) = images.get_mut(&roofs.image) else { return };
    let Some(data) = image.data.as_mut() else { return };
    for z in 0..ROOFS {
        for x in 0..ROOFS {
            let y = voxels.roof(corner.x + x, corner.y + z) as f32;
            let i = ((z * ROOFS + x) * 4) as usize;
            data[i..i + 4].copy_from_slice(&y.to_le_bytes());
        }
    }
}

/// Shows rain and snow as the weather says, and tells the shader where the
/// eye is, the wind and how much falls.
#[allow(clippy::too_many_arguments)]
pub fn fall(
    time: Res<Time>,
    clock: Res<Clock>,
    weather: Res<Weather>,
    roofs: Option<Res<Roofs>>,
    info: Res<crate::ui::WorldInfo>,
    cam: Query<(&GlobalTransform, &Projection), With<MainCamera>>,
    mut falling: Query<(&Falling, &mut Visibility, &mut Transform)>,
    mut mats: ResMut<Assets<FallMaterial>>,
    settings: Res<crate::settings::Settings>,
) {
    let Ok((gt, proj)) = cam.single() else { return };
    let Some(roofs) = roofs else { return };
    let view_proj = proj.get_clip_from_view() * gt.affine().inverse();
    let eye = gt.translation();
    // dim at night, greyer in a storm, and lit up by lightning
    let day = clock.daylight();
    let light = (0.06 + 0.7 * day) * (1.0 - 0.35 * weather.storm) + 0.8 * weather.glow(settings.reduce_flashing);
    let t = (time.elapsed_secs_f64() % 3600.0) as f32;
    for (f, mut vis, mut at) in &mut falling {
        // where it's drawn is the shader's; this only sorts it with the
        // other see-through things, nearest of all, so the clouds never
        // cover it
        at.translation = eye;
        let amount = if f.snow { weather.snow } else { weather.rain };
        let show = amount > 0.005 && info.in_world;
        vis.set_if_neq(if show { Visibility::Inherited } else { Visibility::Hidden });
        if !show {
            continue;
        }
        let Some(mut m) = mats.get_mut(&f.mat) else { continue };
        let (size, speed, length, width, look) = if f.snow {
            (SNOW_BOX, 1.5, 0.07, 0.07, Vec4::new(light, light, light * 1.04, 0.85))
        } else {
            (RAIN_BOX, 15.0, 0.85, 0.026, Vec4::new(0.9 * light, 0.97 * light, 1.08 * light, 0.5))
        };
        // flakes ride the wind more than drops do
        let wind = weather.wind * if f.snow { 0.6 } else { 0.35 };
        m.fall = FallUniform {
            view_proj,
            eye: eye.extend(t),
            wind: Vec4::new(wind.x, wind.y, speed, amount.clamp(0.0, 1.0)),
            look,
            shape: Vec4::new(size, length, width, if f.snow { 1.0 } else { 0.0 }),
            // until the map's made, it covers nothing
            roofs: roofs.corner.map_or(Vec4::ZERO, |c| Vec4::new(c.x as f32, c.y as f32, ROOFS as f32, 0.0)),
        };
    }
}
