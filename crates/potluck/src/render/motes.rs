//! Small things in the air near the player: embers rising from braziers,
//! fireflies drifting over the grass at night, and dust hanging in rooms and
//! under trees by day. A few dozen tiny specks, moved on the CPU each frame;
//! embers and fireflies glow (bloom gives them their halo), while dust is
//! lit like anything else, so it only shows where a shaft of sun catches it.
//! They blink and fade slowly, never flash.

use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use super::{Clock, NotRaytraced};
use crate::blocks::{BRAZIER, GRASS};
use crate::noise::hash2;
use crate::player::Me;
use crate::world::{Voxels, SEA};

/// The most embers and fireflies about at once.
const EMBERS: usize = 90;
const FIREFLIES: usize = 28;
const DUST: usize = 110;
/// How far from the player braziers send up embers and fireflies stray.
const EMBER_REACH: f32 = 28.0;
const FIREFLY_REACH: f32 = 20.0;
const DUST_REACH: f32 = 4.5;

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Ember,
    Firefly,
    Dust,
}

#[derive(Component)]
pub struct Mote {
    kind: Kind,
    vel: Vec3,
    age: f32,
    life: f32,
    size: f32,
    seed: f32,
}

#[derive(Resource)]
pub struct MoteLooks {
    mesh: Handle<Mesh>,
    ember: Handle<StandardMaterial>,
    firefly: Handle<StandardMaterial>,
    dust: Handle<StandardMaterial>,
}

pub fn setup(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut mats: ResMut<Assets<StandardMaterial>>, settings: Res<crate::settings::Settings>) {
    let glow = |c: LinearRgba| StandardMaterial { base_color: Color::BLACK, emissive: c, perceptual_roughness: 1.0, ..default() };
    // Low has no bloom, so there they're dim enough to keep their colour
    // rather than burning out to white specks (as flames are, `render::make_materials`)
    let low = crate::render::Tier::of(&settings.lighting) == crate::render::Tier::Low;
    commands.insert_resource(MoteLooks {
        mesh: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        ember: mats.add(glow(LinearRgba::rgb(14.0, 5.0, 1.2) * if low { 0.25 } else { 600.0 })),
        firefly: mats.add(glow(LinearRgba::rgb(5.0, 12.0, 2.0) * if low { 0.25 } else { 260.0 })),
        dust: mats.add(StandardMaterial { base_color: Color::srgb(1.0, 0.94, 0.82), perceptual_roughness: 1.0, reflectance: 0.0, ..default() }),
    });
}

/// A small random number generator for the motes, so they need no crate.
#[derive(Default)]
pub struct Dice(u32);

impl Dice {
    fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.0 >> 8) as f32 / (1u32 << 24) as f32
    }

    fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.next()
    }
}

/// Spawns, moves and retires the embers and fireflies.
#[allow(clippy::too_many_arguments)]
pub fn motes(
    mut commands: Commands,
    looks: Option<Res<MoteLooks>>,
    time: Res<Time>,
    voxels: Res<Voxels>,
    me: Res<Me>,
    clock: Res<Clock>,
    mut motes: Query<(Entity, &mut Transform, &mut Mote)>,
    mut dice: Local<Dice>,
    mut owed: Local<f32>,
) {
    let Some(looks) = looks else { return };
    let dt = time.delta_secs().min(0.1);
    let t = time.elapsed_secs();
    let night = 1.0 - clock.daylight();
    let (mut embers, mut fireflies, mut dust) = (0, 0, 0);
    for (e, mut tf, mut m) in &mut motes {
        m.age += dt;
        let far = match m.kind {
            Kind::Ember => false,
            Kind::Firefly => tf.translation.distance(me.pos) > FIREFLY_REACH + 4.0,
            Kind::Dust => tf.translation.distance(me.pos) > DUST_REACH + 3.0,
        };
        if m.age > m.life || far {
            commands.entity(e).despawn();
            continue;
        }
        let k = m.age / m.life;
        match m.kind {
            Kind::Ember => {
                embers += 1;
                // rising, slowing, drifting with the breeze and curling a little
                let curl = Vec3::new((t * 3.1 + m.seed).sin(), 0.0, (t * 2.7 + m.seed * 1.3).cos()) * 0.35;
                m.vel.y = (m.vel.y - 0.25 * dt).max(0.25);
                tf.translation += (m.vel + curl + Vec3::new(0.18, 0.0, 0.08)) * dt;
                tf.scale = Vec3::splat(m.size * (1.0 - k * k));
            }
            Kind::Firefly => {
                fireflies += 1;
                // a slow wander, turning gently
                let a = t * 0.4 + m.seed * 6.0 + 2.0 * (t * 0.23 + m.seed).sin();
                let want = Vec3::new(a.cos(), 0.3 * (t * 0.7 + m.seed * 3.0).sin(), a.sin()) * 0.45;
                m.vel = m.vel.lerp(want, (dt * 0.8).min(1.0));
                tf.translation += m.vel * dt;
                // a soft glow that swells and dies away every few seconds
                let pulse = (0.5 + 0.5 * (t * 1.3 + m.seed * 9.0).sin()).powi(3);
                let fade = (m.age / 2.0).min(1.0) * ((m.life - m.age) / 2.0).min(1.0) * ((night - 0.6) / 0.25).clamp(0.0, 1.0);
                tf.scale = Vec3::splat(m.size * pulse * fade);
            }
            Kind::Dust => {
                dust += 1;
                // hanging in the air, turning over slowly
                let sway = Vec3::new((t * 0.31 + m.seed).sin(), 0.6 * (t * 0.23 + m.seed * 2.0).sin(), (t * 0.27 + m.seed * 1.7).cos()) * 0.04;
                tf.translation += (m.vel + sway) * dt;
                let fade = (m.age / 3.0).min(1.0) * ((m.life - m.age) / 3.0).min(1.0);
                tf.scale = Vec3::splat(m.size * fade);
            }
        }
    }
    let spawn = |commands: &mut Commands, kind: Kind, at: Vec3, vel: Vec3, life: f32, size: f32, seed: f32| {
        commands.spawn((
            Mesh3d(looks.mesh.clone()),
            MeshMaterial3d(match kind {
                Kind::Ember => looks.ember.clone(),
                Kind::Firefly => looks.firefly.clone(),
                Kind::Dust => looks.dust.clone(),
            }),
            Transform::from_translation(at).with_scale(Vec3::ZERO),
            Mote { kind, vel, age: 0.0, life, size, seed },
            NotShadowCaster,
            NotRaytraced,
        ));
    };
    // embers: a few a second from each brazier near the player
    *owed += dt * 5.0;
    let n = owed.floor() as usize;
    *owed -= n as f32;
    if n > 0 {
        let braziers: Vec<IVec3> = voxels
            .lanterns
            .iter()
            .copied()
            .filter(|p| voxels.at(*p) == BRAZIER && p.as_vec3().distance(me.pos) < EMBER_REACH)
            .take(10)
            .collect();
        for p in &braziers {
            for _ in 0..n {
                if embers >= EMBERS {
                    break;
                }
                let at = p.as_vec3() + Vec3::new(dice.range(0.3, 0.7), 0.95, dice.range(0.3, 0.7));
                let vel = Vec3::new(dice.range(-0.15, 0.15), dice.range(0.7, 1.4), dice.range(-0.15, 0.15));
                spawn(&mut commands, Kind::Ember, at, vel, dice.range(1.2, 2.6), dice.range(0.025, 0.045), dice.range(0.0, 50.0));
                embers += 1;
            }
        }
    }
    // fireflies: over the grass round the player, once it's dark
    if night > 0.6 && fireflies < FIREFLIES {
        let a = dice.range(0.0, std::f32::consts::TAU);
        let r = dice.range(3.0, FIREFLY_REACH);
        let (x, z) = ((me.pos.x + a.cos() * r).floor() as i32, (me.pos.z + a.sin() * r).floor() as i32);
        let top = voxels.top(x, z);
        if top > SEA && voxels.get(x, top, z) == GRASS {
            let seed = hash2(x, z, 913) * 50.0 + dice.next();
            let at = Vec3::new(x as f32 + dice.next(), top as f32 + 1.0 + dice.range(0.4, 2.2), z as f32 + dice.next());
            spawn(&mut commands, Kind::Firefly, at, Vec3::ZERO, dice.range(18.0, 40.0), dice.range(0.04, 0.06), seed);
        }
    }
    // dust: by day, in the air round a player under a roof or the trees
    let eye = me.eye();
    let covered = voxels.top(eye.x.floor() as i32, eye.z.floor() as i32) as f32 > eye.y + 1.0;
    if covered && night < 0.5 && dust < DUST {
        for _ in 0..2 {
            let at = eye + Vec3::new(dice.range(-1.0, 1.0), dice.range(-0.8, 0.9), dice.range(-1.0, 1.0)) * DUST_REACH;
            if voxels.at(at.floor().as_ivec3()) == crate::blocks::AIR {
                let vel = Vec3::new(dice.range(-0.03, 0.03), dice.range(-0.02, 0.01), dice.range(-0.03, 0.03));
                spawn(&mut commands, Kind::Dust, at, vel, dice.range(8.0, 16.0), dice.range(0.03, 0.05), dice.range(0.0, 50.0));
            }
        }
    }
}
