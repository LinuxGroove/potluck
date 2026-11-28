//! The look of the world: block materials, the sky, the sun and moon going
//! round, fog, and lantern light near the player.

use std::f32::consts::{PI, TAU};

use bevy::anti_alias::taa::TemporalAntiAliasing;
use bevy::camera::Exposure;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::light::atmosphere::ScatteringMedium;
use bevy::light::{
    light_consts::lux, Atmosphere, AtmosphereEnvironmentMapLight, CascadeShadowConfigBuilder, DirectionalLightShadowMap, FogVolume,
    ShadowFilteringMethod, SunDisk, VolumetricFog, VolumetricLight,
};
use bevy::pbr::{AtmosphereSettings, ContactShadows, DistanceFog, FogFalloff, ScreenSpaceAmbientOcclusion, ScreenSpaceAmbientOcclusionQualityLevel};
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;

pub mod clouds;
pub mod dome;
pub mod fall;
pub mod far;
pub mod timings;
pub mod motes;
pub mod murk;

use crate::mesh::BlockMaterials;
use crate::player::{MainCamera, Me};
use crate::settings::Settings;
use crate::textures::BlockAtlas;
use crate::world::Voxels;

/// Minutes for a whole day and night.
const DAY_MINUTES: f32 = 20.0;

/// The colour the sky and fog turn towards at sunset.
const SUNSET: Vec3 = Vec3::new(0.9, 0.58, 0.42);

/// How wide and how tall the haze round the camera is, in blocks.
const HAZE: Vec2 = Vec2::new(200.0, 90.0);

#[derive(Resource)]
pub struct Clock {
    /// 0 is midnight, 0.25 sunrise, 0.5 noon, 0.75 sunset.
    pub time: f32,
    pub running: bool,
}

impl Default for Clock {
    fn default() -> Self {
        Self { time: 0.34, running: true }
    }
}

impl Clock {
    /// How much daylight there is, 0 at night to 1 in the day.
    pub fn daylight(&self) -> f32 {
        let e = (TAU * (self.time - 0.25)).sin();
        crate::noise::smooth(-0.12, 0.25, e)
    }
}

/// How much the lighting asks of the graphics card. Low suits integrated
/// graphics and the Steam Deck; High and Ultra are for a desktop card.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Tier {
    /// Two shadow cascades, flat sky, bright ambient light.
    Low,
    /// The atmosphere lights the world, bloom, four cascades.
    Medium,
    /// Temporal antialiasing, ambient occlusion, contact shadows, light
    /// shafts through haze, sharper and longer shadows.
    High,
    /// High at its finest: more shadow detail and distance, finer haze.
    Ultra,
}

impl Tier {
    pub const NAMES: [&'static str; 4] = ["low", "medium", "high", "ultra"];

    pub fn of(name: &str) -> Tier {
        match name {
            "low" => Tier::Low,
            "high" => Tier::High,
            "ultra" => Tier::Ultra,
            _ => Tier::Medium,
        }
    }

    /// The setting after this one, round to the start.
    pub fn next(name: &str) -> &'static str {
        Self::NAMES[(Self::of(name) as usize + 1) % Self::NAMES.len()]
    }
}

/// Whether the world is ray traced: Ultra, on a graphics card that can,
/// in a build with the `raytracing` feature.
#[derive(Resource, Default, Clone, Copy)]
pub struct Raytraced(pub bool);

impl Raytraced {
    /// Works it out from the setting and what the graphics card can do.
    pub fn new(settings: &Settings, device: Option<&bevy::render::renderer::RenderDevice>) -> Self {
        #[cfg(feature = "raytracing")]
        if Tier::of(&settings.lighting) == Tier::Ultra {
            let need = bevy::solari::SolariPlugins::required_wgpu_features();
            let can = device.is_some_and(|d| d.features().contains(need));
            if can {
                info!("Ultra lighting ray traced");
            } else {
                info!("Ultra lighting without ray tracing: this graphics card can't ray trace");
            }
            return Raytraced(can);
        }
        let _ = (settings, device);
        Raytraced(false)
    }
}

/// Meshes ray tracing leaves out (water).
#[derive(Component)]
pub struct NotRaytraced;

/// Puts a mesh in the ray traced scene only, as a child of `e`: the tracer
/// sees it and nothing draws it.
pub fn trace_only(e: &mut EntityCommands, mesh: Handle<Mesh>, material: Handle<StandardMaterial>) {
    #[cfg(feature = "raytracing")]
    e.with_child((bevy::solari::prelude::RaytracingMesh3d(mesh), MeshMaterial3d(material), Transform::IDENTITY));
    #[cfg(not(feature = "raytracing"))]
    let _ = (e, mesh, material);
}

/// Every mesh drawn joins the ray traced scene, when there is one.
#[cfg(feature = "raytracing")]
pub fn raytrace_meshes(rt: Res<Raytraced>, added: Query<(Entity, &Mesh3d), (Added<Mesh3d>, Without<NotRaytraced>)>, mut commands: Commands) {
    if rt.0 {
        for (e, m) in &added {
            commands.entity(e).insert(bevy::solari::prelude::RaytracingMesh3d(m.0.clone()));
        }
    }
}

/// Your own body leaves the ray traced scene while it's hidden (in first
/// person), since the tracer doesn't look at what's hidden: it would stand
/// round the eye in the dark, putting what's in hand in its shade and
/// keeping the torch in hand's light in.
#[cfg(feature = "raytracing")]
pub fn untrace_hidden(
    rt: Res<Raytraced>,
    me: Query<(Entity, &Visibility), With<crate::net::MyView>>,
    kids: Query<&Children>,
    meshes: Query<(&Mesh3d, Has<bevy::solari::prelude::RaytracingMesh3d>), Without<NotRaytraced>>,
    mut commands: Commands,
) {
    use bevy::solari::prelude::RaytracingMesh3d;
    if !rt.0 {
        return;
    }
    for (e, vis) in &me {
        let hidden = *vis == Visibility::Hidden;
        for c in kids.iter_descendants(e) {
            match meshes.get(c) {
                Ok((_, true)) if hidden => {
                    commands.entity(c).remove::<RaytracingMesh3d>();
                }
                Ok((m, false)) if !hidden => {
                    commands.entity(c).insert(RaytracingMesh3d(m.0.clone()));
                }
                _ => {}
            }
        }
    }
}

#[derive(Component)]
pub struct Sun;

/// The haze light shafts show in, kept round the camera.
#[derive(Component)]
pub struct Haze;

/// The water round the eye when it's under the surface: a ball the fog
/// colours whole, so the sky and the far land don't show through the
/// murk (`sky` shows it, and closes the fog in).
#[derive(Component)]
pub struct UnderSea;

/// How far you see under water: the fog is whole by here, and the ball
/// round the eye a little further out.
const UNDER_SEA: f32 = 26.0;

/// A real light for a lantern, candle or brazier near the player: its
/// steady brightness, how much its flame flickers, and a number that keeps
/// neighbouring flames out of step.
#[derive(Component, Default)]
pub struct LanternLight {
    pub base: f32,
    pub flicker: f32,
    pub phase: f32,
}

/// Water: the block material, except that it keeps its own bumpy normals
/// where the camera has a normal prepass (High, for SSAO). Bevy otherwise
/// lights see-through things with the prepass's normals, which are those of
/// the sea floor under them, and the near sea went flat and pale.
pub type WaterMaterial = bevy::pbr::ExtendedMaterial<StandardMaterial, OwnNormals>;

#[derive(Asset, bevy::render::render_resource::AsBindGroup, Reflect, Debug, Clone, Default)]
pub struct OwnNormals {}

impl bevy::pbr::MaterialExtension for OwnNormals {
    fn specialize(
        _pipeline: &bevy::pbr::MaterialExtensionPipeline,
        descriptor: &mut bevy::render::render_resource::RenderPipelineDescriptor,
        _layout: &bevy::mesh::MeshVertexBufferLayoutRef,
        _key: bevy::pbr::MaterialExtensionKey<Self>,
    ) -> Result<(), bevy::render::render_resource::SpecializedMeshPipelineError> {
        if let Some(f) = descriptor.fragment.as_mut() {
            f.shader_defs.retain(|d| *d != bevy::shader::ShaderDefVal::from("LOAD_PREPASS_NORMALS"));
        }
        Ok(())
    }
}

pub fn make_materials(atlas: &BlockAtlas, images: &mut Assets<Image>, mats: &mut Assets<StandardMaterial>, waters: &mut Assets<WaterMaterial>, rt: Raytraced, tier: Tier) -> BlockMaterials {
    let solid = StandardMaterial {
        base_color_texture: Some(atlas.color.clone()),
        normal_map_texture: Some(atlas.normal.clone()),
        metallic_roughness_texture: Some(atlas.rough.clone()),
        // the texture says how rough each block is
        perceptual_roughness: 1.0,
        metallic: 0.0,
        reflectance: 0.3,
        // rooms under a roof see less of the sky (`mesh::Sky`); ray tracing
        // finds that out for itself
        occlusion_texture: if rt.0 { None } else { Some(atlas.sky.clone()) },
        occlusion_channel: bevy::mesh::UvChannel::Uv1,
        ..default()
    };
    // only glowing blocks get a glowing material (see `mesh::ChunkMesh`).
    // Drawn normally, glow skips the eye's exposure; ray traced, it's real
    // light that goes through it like everything else, and it's all the
    // light lanterns give there, so it's brighter. Low has no bloom to spread
    // a bright flame's colour round it, so there a flame stays dim enough to
    // keep its colour rather than burning out to white
    let glow = StandardMaterial {
        emissive_texture: Some(atlas.glow.clone()),
        emissive: LinearRgba::rgb(12.0, 9.0, 5.0) * if rt.0 { 2400.0 } else if tier == Tier::Low { 0.22 } else { 600.0 },
        // a flame's box shows only the flame
        alpha_mode: AlphaMode::Mask(0.5),
        ..solid.clone()
    };
    // ores and glowcaps that glow a little (`mesh::FAINT`), in their own
    // colours, bright enough to find in the dark but lighting nothing.
    // Drawn normally that's what the screen shows whatever the eye's
    // exposure; ray traced, the eye's exposure takes it down by day
    let faint = StandardMaterial {
        emissive_texture: Some(atlas.glow.clone()),
        emissive: LinearRgba::rgb(1.0, 1.0, 1.0) * if rt.0 { 800.0 } else { 0.8 },
        alpha_mode: AlphaMode::Mask(0.5),
        ..solid.clone()
    };
    let cutout = StandardMaterial { alpha_mode: AlphaMode::Mask(0.5), double_sided: true, cull_mode: None, ..solid.clone() };
    // water reflects about 2% straight on (reflectance 0.35); its colour and
    // how see-through it is come from its depth (`mesh::water_tint`), and the
    // water tile's bumps ripple the reflections
    let water = StandardMaterial {
        base_color: Color::WHITE,
        normal_map_texture: Some(atlas.normal.clone()),
        alpha_mode: AlphaMode::Blend,
        perceptual_roughness: 0.07,
        reflectance: 0.35,
        double_sided: true,
        cull_mode: None,
        ..default()
    };
    // under water, the corners of drawn blocks darken with depth (`mesh::UNDERWATER`);
    // ray traced meshes carry no colours, so there the material takes the
    // colour of a block or two of water instead of showing bare sand
    let under = if rt.0 { StandardMaterial { base_color: LinearRgba::rgb(0.4, 0.76, 0.8).into(), ..solid.clone() } } else { solid.clone() };
    let own = waters.add(WaterMaterial { base: water.clone(), extension: OwnNormals {} });
    let canopy = mats.add(StandardMaterial { base_color: Color::srgb(0.22, 0.4, 0.15), perceptual_roughness: 0.9, ..default() });
    // a flame's stand-in for the tracer gives off a lantern's light at night
    // (`lanterns`), spread over the box, in the lamp's colour; a brighter
    // lamp's box is as much bigger (`mesh::stand_in`). Nothing sees the box
    // itself but the tracer, so it reflects nothing
    let lamps = mats.add(StandardMaterial {
        base_color: Color::BLACK,
        emissive_texture: Some(images.add(lamp_colours())),
        emissive: LinearRgba::WHITE * (LAMP_LUMENS / (PI * 6.0 * STAND_IN * STAND_IN)),
        perceptual_roughness: 1.0,
        ..default()
    });
    BlockMaterials { layers: [mats.add(solid), mats.add(cutout), mats.add(water), mats.add(glow), mats.add(under), mats.add(faint)], canopy, water: own, lamps }
}

/// How wide a lantern's stand-in for the ray tracer is (`mesh::stand_in`):
/// wide enough that its light is spread over a box that can't be missed,
/// which keeps the tracer's speckle down, and still inside its block.
pub const STAND_IN: f32 = 0.4;

/// How much light a lantern gives at night, or deep underground.
const LAMP_LUMENS: f32 = 450_000.0;

/// Each block's lamp colour (`lamp`), by id along a row: what the flames'
/// stand-ins glow in.
fn lamp_colours() -> Image {
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    let mut data = Vec::with_capacity(256 * 4);
    for id in 0..=255u8 {
        let c = lamp(id).2.to_srgba().to_u8_array();
        data.extend_from_slice(&c);
    }
    let mut image = Image::new(Extent3d { width: 256, height: 1, depth_or_array_layers: 1 }, TextureDimension::D2, data, TextureFormat::Rgba8UnormSrgb, bevy::asset::RenderAssetUsages::RENDER_WORLD);
    image.sampler = bevy::image::ImageSampler::nearest();
    image
}

/// Where the tools (`--shot`, `--demo`) draw: an image of exactly the size asked
/// for, whatever size the screen lets the window be, shown scaled in the window.
#[derive(Resource, Clone)]
pub struct Offscreen {
    pub image: Handle<Image>,
    pub size: UVec2,
}

impl Offscreen {
    pub fn new(images: &mut Assets<Image>, size: UVec2) -> Self {
        use bevy::render::render_resource::{TextureFormat, TextureUsages};
        let mut image = Image::new_target_texture(size.x, size.y, TextureFormat::Rgba8UnormSrgb, None);
        // screenshots copy out of it
        image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
        image.texture_descriptor.label = Some("offscreen shot");
        Self { image: images.add(image), size }
    }

    /// The window shows the image, scaled to fit.
    fn show(&self, commands: &mut Commands) {
        use bevy::camera::ScalingMode;
        commands.spawn((
            Camera2d,
            Projection::Orthographic(OrthographicProjection {
                scaling_mode: ScalingMode::AutoMin { min_width: self.size.x as f32, min_height: self.size.y as f32 },
                ..OrthographicProjection::default_2d()
            }),
        ));
        commands.spawn(Sprite::from_image(self.image.clone()));
    }
}

/// The world behind a menu, softened and a little darker (`ui::draw::blurs`),
/// eased in over a moment. Low skips it: the menu's shade is enough there.
#[derive(Component, bevy::render::extract_component::ExtractComponent, Clone, Copy, bevy::render::render_resource::ShaderType, Default)]
#[extract_app(bevy::render::RenderApp)]
pub struct MenuBlur {
    pub amount: f32,
    _a: f32,
    _b: f32,
    _c: f32,
}

const MENU_BLUR: Handle<Shader> = bevy::asset::uuid_handle!("6f1d3c2a-8e47-4b9a-a5d2-3c7e91b04f68");

impl bevy::core_pipeline::fullscreen_material::FullscreenMaterial for MenuBlur {
    fn fragment_shader() -> bevy::shader::ShaderRef {
        MENU_BLUR.into()
    }
}

/// The menu blur's shader and pass, for the game (not the server).
pub fn add_menu_blur(app: &mut App) {
    bevy::asset::load_internal_asset!(app, MENU_BLUR, "render/menu_blur.wgsl", Shader::from_wgsl);
    app.add_plugins(bevy::core_pipeline::fullscreen_material::FullscreenMaterialPlugin::<MenuBlur>::default());
    app.add_systems(Update, menu_blur);
}

pub fn menu_blur(mut commands: Commands, focus: Res<crate::ui::Focus>, info: Res<crate::ui::WorldInfo>, settings: Res<Settings>, time: Res<Time>, mut cam: Query<(Entity, Option<&mut MenuBlur>), With<MainCamera>>, mut amount: Local<f32>) {
    let want = info.in_world && crate::ui::draw::blurs(&focus.screen) && Tier::of(&settings.lighting) != Tier::Low;
    let goal = if want { 1.0 } else { 0.0 };
    *amount += (goal - *amount).clamp(-1.0, 1.0) * (time.delta_secs() * 7.0).min(1.0);
    if (goal - *amount).abs() < 0.01 {
        *amount = goal;
    }
    let Ok((e, blur)) = cam.single_mut() else { return };
    match blur {
        Some(mut b) if *amount > 0.0 => {
            if b.amount != *amount {
                b.amount = *amount;
            }
        }
        Some(_) => {
            commands.entity(e).remove::<MenuBlur>();
        }
        None if *amount > 0.0 => {
            commands.entity(e).insert(MenuBlur { amount: *amount, ..default() });
        }
        None => {}
    }
}

/// A screenshot of what the game draws: the offscreen image when there is one.
pub fn screenshot(off: Option<&Offscreen>) -> bevy::render::view::screenshot::Screenshot {
    use bevy::render::view::screenshot::Screenshot;
    match off {
        Some(o) => Screenshot::image(o.image.clone()),
        None => Screenshot::primary_window(),
    }
}

#[allow(clippy::too_many_arguments)]
pub fn spawn_sky(commands: &mut Commands, settings: &Settings, media: &mut Assets<ScatteringMedium>, meshes: &mut Assets<Mesh>, mats: &mut Assets<StandardMaterial>, rt: Raytraced, off: Option<&Offscreen>) {
    let tier = Tier::of(&settings.lighting);
    let medium = tier >= Tier::Medium;
    // ray tracing does the shadows, corners and bounced light itself
    let high = tier >= Tier::High && !rt.0;
    let ultra = tier == Tier::Ultra;
    let cam = commands.spawn((
        Camera3d::default(),
        MainCamera,
        Projection::Perspective(PerspectiveProjection { fov: settings.fov.clamp(70.0, 110.0).to_radians() * 0.75, near: 0.05, far: 900.0, ..default() }),
        Transform::from_xyz(0.0, 60.0, 0.0),
        Exposure { ev100: 13.7 },
        // set through the day (`sky`)
        bevy::render::view::ColorGrading::default(),
        // keeps colours rich where AgX washed the island out to pastels
        Tonemapping::TonyMcMapface,
        DistanceFog {
            color: Color::srgba(0.62, 0.74, 0.88, 1.0),
            directional_light_color: Color::srgba(1.0, 0.93, 0.8, 0.4),
            directional_light_exponent: 24.0,
            // with the sky's own haze on Medium and up, fog only softens the far hills
            falloff: if medium { FogFalloff::Linear { start: 160.0, end: 520.0 } } else { FogFalloff::Linear { start: 140.0, end: 330.0 } },
        },
    )).id();
    let murk = mats.add(StandardMaterial { base_color: Color::BLACK, unlit: true, cull_mode: None, ..default() });
    let ball = commands
        .spawn((
            Mesh3d(meshes.add(Sphere::new(UNDER_SEA + 4.0).mesh().ico(2).unwrap())),
            MeshMaterial3d(murk),
            Transform::IDENTITY,
            Visibility::Hidden,
            bevy::light::NotShadowCaster,
            bevy::light::NotShadowReceiver,
            bevy::camera::visibility::NoFrustumCulling,
            NotRaytraced,
            UnderSea,
        ))
        .id();
    commands.entity(cam).add_child(ball);
    if let Some(off) = off {
        commands.entity(cam).insert((bevy::camera::RenderTarget::Image(off.image.clone().into()), IsDefaultUiCamera));
        off.show(commands);
    }
    if medium {
        commands.entity(cam).insert((AtmosphereSettings::default(), AtmosphereEnvironmentMapLight::default(), Bloom::NATURAL));
        // past the island is sea, not the beige ground under Earth's sky
        let mut air = Atmosphere::earth(media.add(ScatteringMedium::earth(256, 256)));
        air.ground_albedo = Vec3::new(0.02, 0.07, 0.11);
        commands.spawn(air);
    }
    // for measuring what High costs: POTLUCK_HIGH_OFF=ssao,contact,haze,taa,shadowmap
    let off = std::env::var("POTLUCK_HIGH_OFF").unwrap_or_default();
    let on = |what: &str| !off.split(',').any(|o| o.trim() == what);
    if high {
        // TAA in place of MSAA: SSAO and contact shadows need it, and it
        // smooths the shadow filter and the haze's ray marching too
        if on("taa") {
            commands.entity(cam).insert((Msaa::Off, TemporalAntiAliasing::default(), ShadowFilteringMethod::Temporal));
        } else {
            // SSAO won't run with MSAA
            commands.entity(cam).insert(Msaa::Off);
        }
        if on("ssao") {
            commands.entity(cam).insert(ScreenSpaceAmbientOcclusion {
                quality_level: if ultra { ScreenSpaceAmbientOcclusionQualityLevel::Ultra } else { ScreenSpaceAmbientOcclusionQualityLevel::High },
                ..default()
            });
        }
        // contact shadows cost High half a millisecond for nothing anyone
        // could see on the p16g3, so only Ultra without ray tracing keeps them
        if ultra && on("contact") {
            commands.entity(cam).insert(ContactShadows::default());
        }
        if on("haze") {
            // TAA blends the jittered steps, so 8 look as good as 16 for half
            // the cost (POTLUCK_HAZE_STEPS=n tries others, for measuring)
            let steps = std::env::var("POTLUCK_HAZE_STEPS").ok().and_then(|n| n.parse().ok()).unwrap_or(if ultra { 16 } else { 8 });
            commands.entity(cam).insert(VolumetricFog { ambient_color: Color::srgb(0.62, 0.74, 0.88), ambient_intensity: 0.0, jitter: 0.6, step_count: steps });
            commands.spawn((
                FogVolume { fog_color: Color::srgb(0.9, 0.93, 1.0), density_factor: 0.01, absorption: 0.25, scattering: 0.35, ..default() },
                Transform::from_scale(Vec3::new(HAZE.x, HAZE.y, HAZE.x)),
                Haze,
            ));
        }
        // ground cover casts shadows only near the player (`mesh::Detail`), and
        // the first cascade covers 10 blocks, so 2048 keeps shadows crisp
        // (sizes must be powers of two: Bevy rounds anything else up)
        commands.insert_resource(DirectionalLightShadowMap { size: if !on("shadowmap") { 1024 } else if ultra { 4096 } else { 2048 } });
    }
    if tier == Tier::Low {
        // a forest's shadows are most of Low's frame on integrated graphics,
        // and over Low's 80 blocks of shadow 1024 still shows each leaf block
        commands.insert_resource(DirectionalLightShadowMap { size: 1024 });
    }
    let cascades = CascadeShadowConfigBuilder {
        num_cascades: if medium { 4 } else { 2 },
        first_cascade_far_bound: if high { 10.0 } else { 12.0 },
        maximum_distance: match tier {
            Tier::Low => 80.0,
            Tier::Medium => 160.0,
            Tier::High => 220.0,
            Tier::Ultra => 320.0,
        },
        ..default()
    }
    .build();
    #[cfg(feature = "raytracing")]
    if rt.0 {
        use bevy::render::render_resource::TextureUsages;
        commands.entity(cam).insert((
            Msaa::Off,
            bevy::camera::CameraMainTextureUsages::default().with(TextureUsages::STORAGE_BINDING),
            bevy::solari::prelude::SolariLighting::default(),
            TemporalAntiAliasing::default(),
        ));
    }
    let mut sun = commands.spawn((
        DirectionalLight { shadow_maps_enabled: !rt.0, contact_shadows_enabled: high && ultra && on("contact"), illuminance: lux::RAW_SUNLIGHT, ..default() },
        cascades,
        SunDisk::EARTH,
        Transform::default(),
        Sun,
        // hidden chunks that still shade what's seen (`mesh::cull`)
        bevy::camera::visibility::RenderLayers::from_layers(&[0, crate::mesh::cull::SHADOW_ONLY]),
    ));
    if high && on("haze") {
        sun.insert(VolumetricLight);
    }
    // no point light casts a shadow (lanterns are many and near), but Bevy
    // keeps a cube of shadow map for them anyway: 24 MB at its 1024
    if std::env::var("POTLUCK_OFF").unwrap_or_default().split(',').all(|o| o.trim() != "cube") {
        commands.insert_resource(bevy::light::PointLightShadowMap { size: 16 });
    }
    commands.insert_resource(ClearColor(Color::srgb(0.5, 0.68, 0.9)));
    commands.insert_resource(GlobalAmbientLight { color: Color::srgb(0.75, 0.82, 1.0), brightness: if medium { 1200.0 } else { 6000.0 }, ..default() });
}

/// The fog's colour (sRGB), which the far hills fade into, at daylight `day`
/// with the sun `low` (1 at sunset); Low's sky meets it at the horizon.
pub(crate) fn fog_colour(day: f32, low: f32) -> Vec3 {
    let day_fog = Vec3::new(0.62, 0.74, 0.88);
    let night_fog = Vec3::new(0.03, 0.045, 0.09);
    night_fog.lerp(day_fog, day).lerp(SUNSET, 0.45 * low * day)
}

/// What the weather (`sky::Weather`) does to the light: how much of the sky
/// is grey cloud, how much of the sun gets through it, how thick the fog
/// is, and lightning's glow. The sky, the clouds, Low's dome, the fog and
/// the falling rain all read the same numbers, so they agree.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Gloom {
    /// How much of the sky is grey, 0 (clear) to 1 (overcast).
    pub cover: f32,
    /// The share of the sun's light that gets through, 1 in a clear sky.
    pub sun: f32,
    /// How thick the fog is, 0 to 1.
    pub fog: f32,
    /// How dark a storm's cloud is, 0 to 1.
    pub storm: f32,
    /// Lightning's soft glow now, 0 to 0.5 (none with Reduce flashing on).
    pub glow: f32,
}

impl Default for Gloom {
    /// A clear sky.
    fn default() -> Self {
        Gloom { cover: 0.0, sun: 1.0, fog: 0.0, storm: 0.0, glow: 0.0 }
    }
}

impl Gloom {
    pub fn of(w: &crate::sky::Weather, reduce_flashing: bool) -> Gloom {
        use crate::noise::smooth;
        let fog = w.fog.clamp(0.0, 1.0);
        Gloom {
            cover: smooth(0.2, 0.9, w.cloud).max(smooth(0.05, 0.6, fog)),
            sun: (1.0 - 0.82 * smooth(0.2, 1.0, w.cloud)) * (1.0 - 0.6 * fog),
            fog,
            storm: w.storm.clamp(0.0, 1.0),
            glow: w.glow(reduce_flashing),
        }
    }

    /// The cloud's grey (sRGB): pale by day, slate in a storm, nearly black
    /// at night, warmed a little by a low sun; and lit up by lightning.
    pub fn grey(&self, day: f32, low: f32) -> Vec3 {
        let pale = Vec3::new(0.64, 0.67, 0.71).lerp(Vec3::new(0.3, 0.32, 0.36), self.storm);
        let night = Vec3::new(0.022, 0.027, 0.04);
        night.lerp(pale, day).lerp(SUNSET * 0.7, 0.25 * low * day) + Vec3::new(0.5, 0.52, 0.6) * self.glow
    }

    /// The fog's colour under this weather (sRGB): the clear day's, going
    /// grey as the cloud comes over.
    pub fn fog_colour(&self, day: f32, low: f32) -> Vec3 {
        fog_colour(day, low).lerp(self.grey(day, low), self.cover)
    }

    /// Where the fog starts and where it's whole: as far as the clear day's
    /// on Medium and up (or Low's), closing in as the fog thickens.
    pub fn reach(&self, medium: bool) -> (f32, f32) {
        let (start, clear) = if medium { (160.0, 520.0) } else { (140.0, 330.0) };
        let end = clear + (48.0 - clear) * self.fog.powf(0.6);
        (start * (end / clear) * (1.0 - self.fog.sqrt()), end)
    }

    /// How many stops the eye opens by day as the cloud takes the sun away
    /// (less than it lost: an overcast day is duller than a sunny one).
    pub fn opens(&self) -> f32 {
        (-self.sun.max(0.05).log2() * 0.55).min(1.6)
    }
}

/// The colour of the water round the eye (sRGB) at daylight `day`, `deep`
/// blocks under the surface: green-blue near the top by day, bluer and
/// darker going down, and nearly black at night.
pub(crate) fn under_water_colour(day: f32, deep: f32) -> Vec3 {
    let top = Vec3::new(0.12, 0.43, 0.44);
    let down = Vec3::new(0.02, 0.1, 0.16);
    top.lerp(down, 1.0 - (-deep / 14.0).exp()) * (0.06 + 0.94 * day)
}

/// Toward the sun (unit length; below the horizon at night), the daylight,
/// and how low the sun is (1 at sunset), as `sky` works them out.
pub(crate) fn sun_now(clock: &Clock) -> (Vec3, f32, f32) {
    let a = TAU * (clock.time - 0.25);
    let up = a.sin();
    (Vec3::new(a.cos(), up, -0.35).normalize(), clock.daylight(), 1.0 - crate::noise::smooth(0.02, 0.4, up))
}

/// How far into the night it is for the eye, 0 by day and 1 at night.
fn night_eye(day: f32) -> f32 {
    (1.0 - day).powf(0.7)
}

/// What the eye has got used to: how much of the sky it sees, and how far
/// underground it is (0 in the open, 1 deep in a cave where no sky
/// reaches), each followed slowly as eyes adjust. Deep underground it's as
/// dark by day as by night: the eye opens up as it does at night, the
/// sky's fill and reflections stay outside, and flames burn as bright as
/// they do at night.
#[derive(Resource)]
pub struct Eye {
    since: f32,
    open: f32,
    pub below: f32,
}

impl Default for Eye {
    fn default() -> Self {
        Self { since: 1.0, open: 1.0, below: 0.0 }
    }
}

/// The atmosphere lays the haze of sunlit air over everything a little way
/// off (its aerial view). In a cave, with the eye opened wide, that haze
/// showed as a blue veil by day; so the deeper the player is, the further
/// the haze is spread, until none of it falls this side of the rock.
pub fn cave_air(eye: Res<Eye>, mut air: Query<&mut AtmosphereSettings, With<MainCamera>>) {
    let far = AtmosphereSettings::default().aerial_view_lut_max_distance * 1000f32.powf(eye.below);
    for mut s in &mut air {
        if (s.aerial_view_lut_max_distance / far - 1.0).abs() > 0.01 {
            s.aerial_view_lut_max_distance = far;
        }
    }
}

/// How far underground a point is for the eye: 0 within a few blocks of
/// the ground over it (rooms, overhangs, the shade of trees), 1 well down.
pub fn underground(voxels: &Voxels, p: Vec3) -> f32 {
    let (x, z) = (p.x.floor() as i32, p.z.floor() as i32);
    let top = voxels.top(x, z);
    // the highest rock over it, not a tree's trunk
    let roof = (p.y.floor() as i32 + 1..=top).rev().find(|&y| {
        let b = voxels.get(x, y, z);
        crate::blocks::is_opaque(b) && b != crate::blocks::LOG
    });
    roof.map_or(0.0, |r| crate::noise::smooth(5.0, 15.0, r as f32 - p.y))
}

pub fn sky(
    time: Res<Time>,
    mut clock: ResMut<Clock>,
    settings: Res<Settings>,
    mut commands: Commands,
    mut sun: Query<(Entity, &mut Transform, &mut DirectionalLight, Has<VolumetricLight>), With<Sun>>,
    mut cam: Query<(&mut Exposure, &mut DistanceFog, &mut Projection, &mut bevy::render::view::ColorGrading, Option<&mut bevy::light::EnvironmentMapLight>), With<MainCamera>>,
    mut clear: ResMut<ClearColor>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut haze: Query<(&mut Transform, &mut FogVolume), (With<Haze>, Without<Sun>, Without<MainCamera>)>,
    me: Res<Me>,
    rt: Res<Raytraced>,
    voxels: Res<crate::world::Voxels>,
    mut under: Query<(&mut Visibility, &MeshMaterial3d<StandardMaterial>), With<UnderSea>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut eye: ResMut<Eye>,
    weather: Res<crate::sky::Weather>,
) {
    let gloom = Gloom::of(&weather, settings.reduce_flashing);
    // how open the sky over the player is, and how far underground they
    // are, looked at a few times a second and followed slowly, as eyes adjust
    eye.since += time.delta_secs();
    if eye.since > 0.25 {
        let now = crate::mesh::sky_seen(&voxels, me.eye());
        let below = underground(&voxels, me.eye()) * (1.0 - now).clamp(0.0, 1.0);
        let k = (eye.since / 1.5).min(1.0);
        eye.open += (now - eye.open) * k;
        eye.below += (below - eye.below) * k;
        eye.since = 0.0;
    }
    let shade = 1.0 - eye.open.clamp(0.0, 1.0);
    let below = eye.below.clamp(0.0, 1.0);
    if clock.running {
        clock.time = (clock.time + time.delta_secs() / (DAY_MINUTES * 60.0)) % 1.0;
    }
    let a = TAU * (clock.time - 0.25);
    let day = clock.daylight();
    // the sun crosses from east to south to west; at night the moon takes over
    let up = a.sin();
    let (dir, strength) = if up > -0.05 {
        (Vec3::new(a.cos(), up.max(0.02), -0.35).normalize(), day)
    } else {
        (Vec3::new(-a.cos(), -up, 0.35).normalize(), 0.0)
    };
    let medium = Tier::of(&settings.lighting) >= Tier::Medium;
    // how near the horizon the sun is, 1 at sunset
    let low = 1.0 - crate::noise::smooth(0.02, 0.4, up);
    // the haze only shows in sunlight, so it thins away as the moon takes
    // over and isn't drawn at all in the dead of night (on High that's a
    // sixth of the frame)
    // and none under grey cloud, where no shafts come through
    let haze_lit = crate::noise::smooth(-0.3, -0.12, up) * (1.0 - crate::noise::smooth(0.3, 0.85, gloom.cover));
    if let Ok((e, mut t, mut l, shafts)) = sun.single_mut() {
        if !haze.is_empty() && shafts != (haze_lit > 0.0) {
            if shafts {
                commands.entity(e).remove::<VolumetricLight>();
            } else {
                commands.entity(e).insert(VolumetricLight);
            }
        }
        *t = Transform::from_translation(Vec3::ZERO).looking_to(-dir, Vec3::Y);
        if up > -0.05 {
            // the sun fades out just before the moon takes over, so neither jumps
            l.illuminance = lux::RAW_SUNLIGHT * strength.max(0.002) * crate::noise::smooth(-0.05, 0.06, up);
            l.color = Color::srgb(1.0, 0.86 + 0.14 * day, 0.7 + 0.3 * day);
            if rt.0 || !medium {
                // the atmosphere reddens and dims a low sun for the other tiers;
                // ray tracing and Low's flat sky need telling
                l.color = Color::srgb(1.0, 0.86 + 0.14 * day - 0.36 * low, 0.7 + 0.3 * day - 0.52 * low);
            }
            if !medium {
                l.illuminance *= 1.0 - 0.75 * low;
            }
        } else {
            // moonlight: cool, and dim enough to look like night
            l.illuminance = 400.0 * (1.0 - crate::noise::smooth(-0.2, -0.05, up));
            l.color = Color::srgb(0.66, 0.76, 1.0);
        }
        // cloud takes the sun (and the moon) away, and its shadows with it
        l.illuminance *= gloom.sun;
    }
    if let Ok((mut t, mut v)) = haze.single_mut() {
        // the haze sits on the ground round the player, thickest at dawn and dusk
        t.translation = Vec3::new(me.pos.x, (me.pos.y - 20.0).max(0.0) + HAZE.y * 0.5, me.pos.z);
        let low_sun = 1.0 - (up.abs() * 2.5).min(1.0);
        v.density_factor = if me.under_water { 0.0 } else { (0.0025 + 0.0055 * low_sun) * haze_lit };
    }
    if let Ok((mut ex, mut fog, mut proj, mut grade, env)) = cam.single_mut() {
        // the day's grade: golden and a little richer as the sun goes down,
        // cool and quieter at night, plain at noon
        let golden = low * day;
        let night = 1.0 - day;
        grade.global.temperature = 0.035 * golden - 0.03 * night;
        grade.global.tint = 0.015 * golden;
        grade.global.post_saturation = 1.0 + 0.12 * golden - 0.18 * night;
        grade.shadows.lift = -0.008 * golden;
        grade.highlights.gain = 1.0 + 0.04 * golden;
        // an overcast day is a little greyer and cooler
        grade.global.post_saturation *= 1.0 - 0.22 * gloom.cover;
        grade.global.temperature -= 0.025 * gloom.cover * day;
        // eyes adjust to the dark, gently; ray traced shade has no made-up
        // fill from everywhere, so there the eye opens up a little more
        let night_open = |n: f32| n * 4.4 + if rt.0 { 0.4 + 1.0 * n } else { 0.0 };
        // under trees or a roof by day the eye opens up: a little where the
        // sky's fill still reaches in, more where ray tracing finds the
        // shade truly dark; deep underground, as far as it does at night
        let open = night_open(night_eye(day)) + day * shade.powf(1.3) * if rt.0 { 3.5 } else { 1.0 } + day * gloom.opens();
        ex.ev100 = 13.7 - open.max(night_open(below));
        // the sky's reflections and fill don't reach down into caves either
        // (the sky's share each face sees only dims some of it): deep
        // underground there's a little of the night sky's, never the day's,
        // so a cave is as dark at noon as at midnight
        if let Some(mut env) = env {
            env.intensity = 1.0 - below * (1.0 - 0.03 * (1.0 - day));
        }
        // the fog greys under cloud and closes in as it thickens; past the
        // clear day's haze it hides the land whole, so the far hills go
        let f = gloom.fog_colour(day, low);
        let whole = crate::noise::smooth(0.0, 0.5, gloom.fog).max(0.5 * gloom.cover);
        fog.color = Color::srgba(f.x, f.y, f.z, if medium { 0.65 + 0.35 * whole } else { 1.0 });
        fog.directional_light_color = Color::srgba(1.0, 0.93, 0.8, 0.4 * gloom.sun);
        fog.directional_light_exponent = 24.0;
        let (start, end) = gloom.reach(medium);
        fog.falloff = FogFalloff::Linear { start, end };
        if me.under_water {
            // under water the murk closes in, darker the deeper you go and
            // brighter toward the sun; the grade turns cool and quiet. It
            // only changes as the eye goes under, never sways
            let deep = (crate::player::surface(&voxels, me.eye()) - me.eye().y).max(0.0);
            let w = under_water_colour(day, deep);
            fog.color = Color::srgb(w.x, w.y, w.z);
            fog.directional_light_color = Color::srgba(0.45, 0.75, 0.72, 0.5 * day * (-deep / 16.0).exp());
            fog.directional_light_exponent = 6.0;
            fog.falloff = FogFalloff::Linear { start: 1.0, end: UNDER_SEA };
            grade.global.temperature -= 0.1;
            grade.global.post_saturation *= 0.85;
        }
        if let Projection::Perspective(p) = &mut *proj {
            p.fov = settings.fov.clamp(70.0, 110.0).to_radians() * 0.75;
        }
    }
    for (mut v, m) in &mut under {
        v.set_if_neq(if me.under_water { Visibility::Inherited } else { Visibility::Hidden });
        // the fog colours it whole; and where nothing's fogged (ray traced),
        // it's the fog's colour itself
        if me.under_water {
            let c = cam.single().map(|(_, f, _, _, _)| f.color).unwrap_or(Color::BLACK);
            if let Some(mut mat) = mats.get_mut(&m.0) {
                mat.base_color = c.with_alpha(1.0);
            }
        }
    }
    // Low's sky, under its dome (`dome`)
    let s = Vec3::new(0.02, 0.03, 0.07).lerp(Vec3::new(0.5, 0.68, 0.9), day).lerp(SUNSET, 0.4 * low * day).lerp(gloom.grey(day, low), gloom.cover);
    clear.0 = Color::srgb(s.x, s.y, s.z);
    // deep underground the sky's fill is the night's, day or night. Under
    // cloud the sky's light is grey and comes from everywhere, so more of
    // it is fill; lightning lights everything for a moment
    let grey_fill = gloom.cover * (1.0 - gloom.sun);
    ambient.brightness = if medium { 40.0 + 1310.0 * day * (1.0 - below) * (1.0 + 1.2 * grey_fill) } else { 160.0 + 5940.0 * day * (1.0 - below) * (1.0 + 0.9 * grey_fill) };
    ambient.brightness += gloom.glow * (1.0 - below) * if medium { 2600.0 } else { 9000.0 };
    let _ = PI;
}

/// How bright a glowing block's light is (a lantern's is 1), how high in the
/// block it sits, its colour and how much its flame flickers (open flames
/// more than one behind glass).
pub fn lamp(id: crate::blocks::BlockId) -> (f32, f32, Color, f32) {
    use crate::blocks::*;
    match id {
        CANDLE => (0.32, 0.6, Color::srgb(1.0, 0.74, 0.42), 0.16),
        BRAZIER => (1.6, 0.9, Color::srgb(1.0, 0.6, 0.28), 0.22),
        // a small open flame: a wall beside it would burn out to white at
        // a lantern's brightness
        TORCH => (0.6, 0.62, Color::srgb(1.0, 0.72, 0.4), 0.12),
        _ => (1.0, 0.5, Color::srgb(1.0, 0.78, 0.48), 0.07),
    }
}

/// Flames flicker: each lamp's light wavers round its steady brightness,
/// out of step with its neighbours. Slow and soft, never a strobe.
pub fn flicker(time: Res<Time>, mut lights: Query<(&LanternLight, &mut PointLight)>) {
    let t = time.elapsed_secs();
    for (l, mut p) in &mut lights {
        let a = t * 7.3 + l.phase;
        let b = t * 13.1 + l.phase * 1.7;
        let c = t * 2.1 + l.phase * 0.6;
        let wave = 0.5 * a.sin() + 0.3 * b.sin() + 0.2 * c.sin();
        p.intensity = l.base * (1.0 + l.flicker * wave);
    }
}

/// How bright a lantern's flame is now. Flames show brighter in the dark
/// (the eye's open wider); deep underground it's as dark by day as by night.
pub fn flame_lumens(clock: &Clock, eye: &Eye) -> f32 {
    let night = (1.0 - clock.daylight()).max(eye.below);
    40_000.0 + (LAMP_LUMENS - 40_000.0) * night
}

/// Real lights for the lanterns, candles and braziers nearest the player;
/// the rest just glow.
pub fn lanterns(
    mut commands: Commands,
    settings: Res<Settings>,
    voxels: Res<Voxels>,
    me: Res<Me>,
    clock: Res<Clock>,
    eye: Res<Eye>,
    mut lights: Query<(Entity, &mut Transform, &mut PointLight, &mut LanternLight), Without<crate::held::HandLight>>,
    mut every: Local<f32>,
    time: Res<Time>,
) {
    *every -= time.delta_secs();
    if *every > 0.0 {
        return;
    }
    *every = 0.3;
    let mut near: Vec<IVec3> = voxels.lanterns.iter().copied().filter(|p| p.as_vec3().distance(me.pos) < 48.0).collect();
    near.sort_by(|a, b| a.as_vec3().distance(me.pos).total_cmp(&b.as_vec3().distance(me.pos)));
    // on Low each light costs integrated graphics dearly, and the furthest
    // still glow without lighting the ground round them
    near.truncate(if Tier::of(&settings.lighting) == Tier::Low { 8 } else { 20 });
    let lumens = flame_lumens(&clock, &eye);
    let mut have: Vec<_> = lights.iter_mut().collect();
    for (i, p) in near.iter().enumerate() {
        let (k, high, color, flicker) = lamp(voxels.at(*p));
        let at = p.as_vec3() + Vec3::new(0.5, high, 0.5);
        let phase = crate::noise::hash2(p.x * 7 + p.y, p.z * 13 - p.y, 401) * 100.0;
        let me = LanternLight { base: lumens * k, flicker, phase };
        if let Some((_, t, l, lamp)) = have.get_mut(i) {
            t.translation = at;
            l.color = color;
            l.range = 14.0 * k.sqrt().max(0.5);
            **lamp = me;
        } else {
            commands.spawn((
                PointLight { intensity: lumens * k, range: 14.0 * k.sqrt().max(0.5), color, shadow_maps_enabled: false, ..default() },
                Transform::from_translation(at),
                me,
            ));
        }
    }
    for (e, _, _, _) in have.into_iter().skip(near.len()) {
        commands.entity(e).despawn();
    }
}
