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

use crate::mesh::BlockMaterials;
use crate::player::{MainCamera, Me};
use crate::settings::Settings;
use crate::textures::BlockAtlas;
use crate::world::Voxels;

/// Minutes for a whole day and night.
const DAY_MINUTES: f32 = 20.0;

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

/// Every mesh drawn joins the ray traced scene, when there is one.
#[cfg(feature = "raytracing")]
pub fn raytrace_meshes(rt: Res<Raytraced>, added: Query<(Entity, &Mesh3d), (Added<Mesh3d>, Without<NotRaytraced>)>, mut commands: Commands) {
    if rt.0 {
        for (e, m) in &added {
            commands.entity(e).insert(bevy::solari::prelude::RaytracingMesh3d(m.0.clone()));
        }
    }
}

#[derive(Component)]
pub struct Sun;

/// The haze light shafts show in, kept round the camera.
#[derive(Component)]
pub struct Haze;

#[derive(Component)]
pub struct LanternLight;

pub fn make_materials(atlas: &BlockAtlas, mats: &mut Assets<StandardMaterial>) -> BlockMaterials {
    let solid = StandardMaterial {
        base_color_texture: Some(atlas.color.clone()),
        normal_map_texture: Some(atlas.normal.clone()),
        metallic_roughness_texture: Some(atlas.rough.clone()),
        emissive_texture: Some(atlas.glow.clone()),
        emissive: LinearRgba::rgb(12.0, 9.0, 5.0) * 600.0,
        // the texture says how rough each block is
        perceptual_roughness: 1.0,
        metallic: 0.0,
        reflectance: 0.3,
        ..default()
    };
    let cutout = StandardMaterial { alpha_mode: AlphaMode::Mask(0.5), double_sided: true, cull_mode: None, ..solid.clone() };
    let water = StandardMaterial {
        base_color: Color::srgba(1.0, 1.0, 1.0, 0.82),
        base_color_texture: Some(atlas.color.clone()),
        alpha_mode: AlphaMode::Blend,
        perceptual_roughness: 0.08,
        reflectance: 0.5,
        double_sided: true,
        cull_mode: None,
        ..default()
    };
    BlockMaterials { layers: [mats.add(solid), mats.add(cutout), mats.add(water)] }
}

pub fn spawn_sky(commands: &mut Commands, settings: &Settings, media: &mut Assets<ScatteringMedium>, rt: Raytraced) {
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
        Exposure { ev100: 13.0 },
        Tonemapping::AgX,
        DistanceFog {
            color: Color::srgba(0.62, 0.74, 0.88, 1.0),
            directional_light_color: Color::srgba(1.0, 0.93, 0.8, 0.4),
            directional_light_exponent: 24.0,
            falloff: FogFalloff::Linear { start: 140.0, end: 330.0 },
        },
    )).id();
    if medium {
        commands.entity(cam).insert((AtmosphereSettings::default(), AtmosphereEnvironmentMapLight::default(), Bloom::NATURAL));
        commands.spawn(Atmosphere::earth(media.add(ScatteringMedium::earth(256, 256))));
    }
    if high {
        // TAA in place of MSAA: SSAO and contact shadows need it, and it
        // smooths the shadow filter and the haze's ray marching too
        commands.entity(cam).insert((
            Msaa::Off,
            TemporalAntiAliasing::default(),
            ShadowFilteringMethod::Temporal,
            ScreenSpaceAmbientOcclusion {
                quality_level: if ultra { ScreenSpaceAmbientOcclusionQualityLevel::Ultra } else { ScreenSpaceAmbientOcclusionQualityLevel::High },
                ..default()
            },
            ContactShadows::default(),
            VolumetricFog { ambient_color: Color::srgb(0.62, 0.74, 0.88), ambient_intensity: 0.0, jitter: 0.6, step_count: if ultra { 128 } else { 64 } },
        ));
        commands.spawn((
            FogVolume { fog_color: Color::srgb(0.9, 0.93, 1.0), density_factor: 0.01, absorption: 0.25, scattering: 0.35, ..default() },
            Transform::from_scale(Vec3::new(HAZE.x, HAZE.y, HAZE.x)),
            Haze,
        ));
        commands.insert_resource(DirectionalLightShadowMap { size: if ultra { 8192 } else { 4096 } });
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
        DirectionalLight { shadow_maps_enabled: !rt.0, contact_shadows_enabled: high, illuminance: lux::RAW_SUNLIGHT, ..default() },
        cascades,
        SunDisk::EARTH,
        Transform::default(),
        Sun,
    ));
    if high {
        sun.insert(VolumetricLight);
    }
    commands.insert_resource(ClearColor(Color::srgb(0.5, 0.68, 0.9)));
    commands.insert_resource(GlobalAmbientLight { color: Color::srgb(0.75, 0.82, 1.0), brightness: if medium { 1200.0 } else { 6000.0 }, ..default() });
}

pub fn sky(
    time: Res<Time>,
    mut clock: ResMut<Clock>,
    settings: Res<Settings>,
    mut sun: Query<(&mut Transform, &mut DirectionalLight), With<Sun>>,
    mut cam: Query<(&mut Exposure, &mut DistanceFog, &mut Projection), With<MainCamera>>,
    mut clear: ResMut<ClearColor>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut haze: Query<(&mut Transform, &mut FogVolume), (With<Haze>, Without<Sun>, Without<MainCamera>)>,
    me: Res<Me>,
) {
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
    if let Ok((mut t, mut l)) = sun.single_mut() {
        *t = Transform::from_translation(Vec3::ZERO).looking_to(-dir, Vec3::Y);
        l.illuminance = if up > -0.05 { lux::RAW_SUNLIGHT * strength.max(0.002) } else { 900.0 };
        l.color = Color::srgb(1.0, 0.86 + 0.14 * day, 0.7 + 0.3 * day);
    }
    let medium = Tier::of(&settings.lighting) >= Tier::Medium;
    if let Ok((mut t, mut v)) = haze.single_mut() {
        // the haze sits on the ground round the player, thickest at dawn and dusk
        t.translation = Vec3::new(me.pos.x, (me.pos.y - 20.0).max(0.0) + HAZE.y * 0.5, me.pos.z);
        let low_sun = 1.0 - (up.abs() * 2.5).min(1.0);
        v.density_factor = 0.006 + 0.012 * low_sun;
    }
    if let Ok((mut ex, mut fog, mut proj)) = cam.single_mut() {
        // eyes adjust to the dark, gently
        ex.ev100 = 13.0 - (1.0 - day) * 5.2;
        let day_fog = Vec3::new(0.62, 0.74, 0.88);
        let night_fog = Vec3::new(0.03, 0.045, 0.09);
        let f = night_fog.lerp(day_fog, day);
        fog.color = Color::srgba(f.x, f.y, f.z, 1.0);
        if let Projection::Perspective(p) = &mut *proj {
            p.fov = settings.fov.clamp(70.0, 110.0).to_radians() * 0.75;
        }
    }
    let s = Vec3::new(0.02, 0.03, 0.07).lerp(Vec3::new(0.5, 0.68, 0.9), day);
    clear.0 = Color::srgb(s.x, s.y, s.z);
    ambient.brightness = if medium { 250.0 + 1100.0 * day } else { 900.0 + 5200.0 * day };
    let _ = PI;
}

/// Real lights for the lanterns nearest the player; the rest just glow.
pub fn lanterns(
    mut commands: Commands,
    voxels: Res<Voxels>,
    me: Res<Me>,
    clock: Res<Clock>,
    mut lights: Query<(Entity, &mut Transform, &mut PointLight), With<LanternLight>>,
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
    near.truncate(20);
    let night = 1.0 - clock.daylight();
    let lumens = 40_000.0 + 260_000.0 * night;
    let mut have: Vec<_> = lights.iter_mut().collect();
    for (i, p) in near.iter().enumerate() {
        let at = p.as_vec3() + Vec3::splat(0.5);
        if let Some((_, t, l)) = have.get_mut(i) {
            t.translation = at;
            l.intensity = lumens;
        } else {
            commands.spawn((
                PointLight { intensity: lumens, range: 14.0, color: Color::srgb(1.0, 0.78, 0.48), shadow_maps_enabled: false, ..default() },
                Transform::from_translation(at),
                LanternLight,
            ));
        }
    }
    for (e, _, _) in have.into_iter().skip(near.len()) {
        commands.entity(e).despawn();
    }
}
