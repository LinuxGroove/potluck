//! The look of the world: block materials, the sky, the sun and moon going
//! round, fog, and lantern light near the player.

use std::f32::consts::{PI, TAU};

use bevy::camera::Exposure;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::light::atmosphere::ScatteringMedium;
use bevy::light::{light_consts::lux, Atmosphere, AtmosphereEnvironmentMapLight, CascadeShadowConfigBuilder, SunDisk};
use bevy::pbr::{AtmosphereSettings, DistanceFog, FogFalloff};
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;

use crate::mesh::BlockMaterials;
use crate::player::{MainCamera, Me};
use crate::settings::Settings;
use crate::textures::BlockAtlas;
use crate::world::Voxels;

/// Minutes for a whole day and night.
const DAY_MINUTES: f32 = 20.0;

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

#[derive(Component)]
pub struct Sun;

#[derive(Component)]
pub struct LanternLight;

pub fn make_materials(atlas: &BlockAtlas, mats: &mut Assets<StandardMaterial>) -> BlockMaterials {
    let solid = StandardMaterial {
        base_color_texture: Some(atlas.color.clone()),
        normal_map_texture: Some(atlas.normal.clone()),
        emissive_texture: Some(atlas.glow.clone()),
        emissive: LinearRgba::rgb(12.0, 9.0, 5.0) * 600.0,
        perceptual_roughness: 0.92,
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

pub fn spawn_sky(commands: &mut Commands, settings: &Settings, media: &mut Assets<ScatteringMedium>) {
    let medium = settings.lighting != "low";
    let mut cam = commands.spawn((
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
    ));
    if medium {
        cam.insert((AtmosphereSettings::default(), AtmosphereEnvironmentMapLight::default(), Bloom::NATURAL));
        commands.spawn(Atmosphere::earth(media.add(ScatteringMedium::earth(256, 256))));
    }
    let cascades = CascadeShadowConfigBuilder {
        num_cascades: if medium { 4 } else { 2 },
        first_cascade_far_bound: 12.0,
        maximum_distance: if medium { 160.0 } else { 80.0 },
        ..default()
    }
    .build();
    commands.spawn((
        DirectionalLight { shadow_maps_enabled: true, illuminance: lux::RAW_SUNLIGHT, ..default() },
        cascades,
        SunDisk::EARTH,
        Transform::default(),
        Sun,
    ));
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
    let medium = settings.lighting != "low";
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
