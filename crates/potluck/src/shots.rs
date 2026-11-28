//! Things shot and thrown (`world.shoot`, world 0.19): arrows, stones and
//! spears in flight. The host flies each one (`Scripts::tick`) and decides
//! what it hits: a creature hears `hit`, and the creation that shot it hears
//! where it landed. Every game is told when one sets off and when it's gone
//! (`Msg::Shot`, `Msg::ShotGone`), and flies its own copy to draw it.

use bevy::prelude::*;

use crate::creatures::{Body, Herd};
use crate::world::Voxels;

/// What a shot can look like, by number.
pub const LOOKS: [&str; 4] = ["arrow", "stone", "spear", "ember"];

/// How fast shots fall at `drop` 1, in blocks a second a second.
pub const GRAVITY: f32 = 16.0;

/// The longest a shot flies, in seconds.
pub const LIFE: f32 = 6.0;

/// The most a creation has in the air at once.
pub const MAX_FLYING: usize = 64;

/// How far a shot moves before it looks for what it hit, in blocks.
const STEP: f32 = 0.25;

/// A shot in the air.
#[derive(Clone, Debug, PartialEq)]
pub struct Flying {
    pub id: u32,
    pub look: u8,
    pub pos: Vec3,
    pub vel: Vec3,
    /// How strongly it falls: 0 flies straight, 1 falls like a stone.
    pub drop: f32,
    pub age: f32,
    /// Stuck in a block (drawn there until it's gone).
    pub stuck: bool,
}

impl Flying {
    /// Moves it on `dt` seconds.
    pub fn fly(&mut self, dt: f32) {
        self.age += dt;
        if self.stuck {
            return;
        }
        self.vel.y -= GRAVITY * self.drop * dt;
        self.pos += self.vel * dt;
    }
}

/// What a shot came down on.
#[derive(Clone, Debug, PartialEq)]
pub enum Landed {
    /// A creature, by id, and where on it.
    Creature(u64, Vec3),
    /// A player, by id, and where on them (only a creature's shots).
    Player(u32, Vec3),
    /// A block, and where it went in.
    Block(IVec3, Vec3),
    /// Nothing: it flew too long, or out of the world.
    Nothing(Vec3),
}

/// Moves a shot on `dt` seconds, a little at a time, and says what it hit
/// if it hit something: the first creature or solid block in its way.
/// `skip` is a creature it can't hit (the one its shooter rides, or the one
/// that threw it), and `players` the players it can (a creature's shots).
pub fn advance(s: &mut Flying, voxels: &Voxels, herd: &Herd, skip: Option<u64>, players: &[crate::script::PlayerInfo], dt: f32) -> Option<Landed> {
    let from = s.pos;
    s.fly(dt);
    let d = s.pos - from;
    let len = d.length();
    // only creatures it could reach this step
    let near: Vec<&Body> = herd.bodies.values().filter(|b| Some(b.id) != skip && b.pos.distance(from) <= len + 4.0).collect();
    let n = ((len / STEP).ceil() as usize).clamp(1, 256);
    for i in 1..=n {
        let p = from + d * (i as f32 / n as f32);
        if let Some(b) = near.iter().find(|b| hits(b, p)) {
            s.pos = p;
            return Some(Landed::Creature(b.id, p));
        }
        if let Some(q) = players.iter().find(|q| in_player(q.pos, p)) {
            s.pos = p;
            return Some(Landed::Player(q.id, p));
        }
        let at = p.floor().as_ivec3();
        if crate::blocks::is_solid(voxels.at(at)) {
            s.pos = p;
            return Some(Landed::Block(at, p));
        }
    }
    if s.age > LIFE || s.pos.y < -140.0 {
        return Some(Landed::Nothing(s.pos));
    }
    None
}

/// Whether a point is inside a player standing at `feet` (their box, a
/// little wider: 0.8 across and 1.9 high).
fn in_player(feet: Vec3, p: Vec3) -> bool {
    (p.x - feet.x).abs() <= 0.4 && (p.z - feet.z).abs() <= 0.4 && p.y >= feet.y - 0.05 && p.y <= feet.y + 1.9
}

/// Whether a point is inside a creature: the box round its parts, turned
/// the way it faces.
fn hits(b: &Body, p: Vec3) -> bool {
    let (lo, hi) = b.bounds();
    let local = Quat::from_rotation_y(-b.yaw) * (p - b.pos);
    let pad = Vec3::splat(0.1);
    local.cmpge(lo - pad).all() && local.cmple(hi + pad).all()
}

/// The shots every game draws: its own copies, flown each frame.
#[derive(Resource, Default)]
pub struct Shots {
    pub list: Vec<Flying>,
}

impl Shots {
    /// One that set off.
    pub fn add(&mut self, s: Flying) {
        if self.list.len() < 512 {
            self.list.retain(|f| f.id != s.id);
            self.list.push(s);
        }
    }

    /// One that's gone.
    pub fn gone(&mut self, id: u32) {
        self.list.retain(|f| f.id != id);
    }
}

#[derive(Component)]
pub struct ShotView(u32);

/// Each look's boxes: (size, where along it, colour). They point down -z,
/// the way they fly.
fn parts(look: u8) -> &'static [([f32; 3], [f32; 3], [f32; 3])] {
    const WOOD: [f32; 3] = [0.55, 0.38, 0.2];
    const FLINT: [f32; 3] = [0.28, 0.28, 0.3];
    const FLETCH: [f32; 3] = [0.92, 0.88, 0.78];
    const STONE: [f32; 3] = [0.5, 0.5, 0.5];
    const IRON: [f32; 3] = [0.74, 0.76, 0.8];
    const EMBER: [f32; 3] = [1.0, 0.55, 0.12];
    const FLAME: [f32; 3] = [1.0, 0.32, 0.06];
    match look {
        0 => &[
            ([0.035, 0.035, 0.75], [0.0, 0.0, 0.0], WOOD),
            ([0.07, 0.07, 0.12], [0.0, 0.0, -0.42], FLINT),
            ([0.12, 0.01, 0.16], [0.0, 0.0, 0.3], FLETCH),
            ([0.01, 0.12, 0.16], [0.0, 0.0, 0.3], FLETCH),
        ],
        2 => &[([0.05, 0.05, 1.5], [0.0, 0.0, 0.0], WOOD), ([0.1, 0.025, 0.24], [0.0, 0.0, -0.86], IRON)],
        3 => &[([0.2, 0.2, 0.2], [0.0, 0.0, 0.0], EMBER), ([0.12, 0.12, 0.3], [0.0, 0.0, 0.2], FLAME)],
        _ => &[([0.16, 0.14, 0.16], [0.0, 0.0, 0.0], STONE)],
    }
}

/// Draws every shot in the air, flying each on from where it was last told
/// it was. One that hits a block here stays stuck in it until it's gone.
pub fn draw(
    mut commands: Commands,
    time: Res<Time>,
    voxels: Res<Voxels>,
    mut shots: ResMut<Shots>,
    mut views: Query<(Entity, &ShotView, &mut Transform)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut kit: Local<Vec<Vec<(Handle<Mesh>, Handle<StandardMaterial>, Vec3)>>>,
) {
    let dt = time.delta_secs().min(0.1);
    if kit.is_empty() {
        for look in 0..LOOKS.len() as u8 {
            let list = parts(look)
                .iter()
                .map(|(size, at, c)| {
                    let mesh = Mesh::from(Cuboid::new(size[0], size[1], size[2])).with_generated_tangents().expect("a box has UVs and normals");
                    // an ember glows
                    let glow = if look == 3 { LinearRgba::rgb(c[0] * 6.0, c[1] * 4.0, c[2] * 2.0) } else { LinearRgba::BLACK };
                    let mat = StandardMaterial { base_color: Color::srgb(c[0], c[1], c[2]), emissive: glow, perceptual_roughness: 0.8, ..default() };
                    (meshes.add(mesh), mats.add(mat), Vec3::from_array(*at))
                })
                .collect();
            kit.push(list);
        }
    }
    for s in shots.list.iter_mut() {
        if !s.stuck {
            s.fly(dt);
            if crate::blocks::is_solid(voxels.at(s.pos.floor().as_ivec3())) {
                s.stuck = true;
            }
        }
    }
    // the host says when each is gone; this is in case it never does
    shots.list.retain(|s| s.age < LIFE + 2.0);
    let mut seen = std::collections::HashSet::new();
    for (e, v, mut t) in &mut views {
        let Some(s) = shots.list.iter().find(|s| s.id == v.0) else {
            commands.entity(e).despawn();
            continue;
        };
        seen.insert(s.id);
        *t = placed(s);
    }
    for s in &shots.list {
        if seen.contains(&s.id) {
            continue;
        }
        let Some(list) = kit.get(s.look as usize) else { continue };
        commands.spawn((ShotView(s.id), placed(s), Visibility::default())).with_children(|c| {
            for (mesh, mat, at) in list {
                c.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(mat.clone()), Transform::from_translation(*at)));
            }
        });
    }
}

/// Where a shot is drawn, pointing the way it flies.
fn placed(s: &Flying) -> Transform {
    let t = Transform::from_translation(s.pos);
    if s.vel.length_squared() > 1e-4 {
        let up = if s.vel.normalize().y.abs() > 0.99 { Vec3::X } else { Vec3::Y };
        t.looking_to(s.vel, up)
    } else {
        t
    }
}
