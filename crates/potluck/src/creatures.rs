//! Creatures: bodies made of boxes that creations define, walking, hopping
//! and being ridden. The host moves them; everyone draws them.

use std::collections::{BTreeMap, HashMap, VecDeque};

use bevy::prelude::*;

use crate::noise::Rng;
use crate::player::{move_body, Me};
use crate::script::KindDef;
use crate::textures::skins::{paint, Skin};
use crate::world::Voxels;

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Part {
    pub name: String,
    pub size: Vec3,
    pub at: Vec3,
    pub color: [f32; 3],
    pub leg: bool,
    pub head: bool,
    pub glow: bool,
    /// Its picture from a Blockbench model (world 0.11), drawn in place of
    /// its colour and skin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paint: Option<Box<Painted>>,
}

/// Where a part's picture is: a texture of one of its creation's Blockbench
/// models, and the rectangle of it on each face (0 to 1 across the texture;
/// none where the model leaves a face bare). Every game has the creation's
/// files, so only this goes between games, never the picture.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Painted {
    pub file: String,
    pub texture: u32,
    /// East, west, south, north, up, down: (u0, v0, u1, v1), u0 on the
    /// left and v0 at the top as the face is seen from outside.
    pub faces: [Option<[f32; 4]>; 6],
    /// What its colour was changed by, channel by channel (1 as the model
    /// has it), so a part given another colour keeps its picture's detail.
    pub tint: [f32; 3],
    /// Changes with the model's file, so a model saved again is drawn again.
    pub stamp: u32,
}

/// The faces in `Painted::faces`' order, as Blockbench names them.
pub const FACES: [&str; 6] = ["east", "west", "south", "north", "up", "down"];

#[derive(Clone, Debug)]
pub struct Body {
    pub id: u64,
    pub creation: String,
    pub kind: String,
    /// The kind it came as: `count` keeps these up, whatever they've
    /// become since, so a sheared sheep doesn't bring another.
    pub born: String,
    pub pos: Vec3,
    pub vel: Vec3,
    pub yaw: f32,
    /// Half width, height, half depth.
    pub half: Vec3,
    pub goal: Option<Vec3>,
    pub wander: bool,
    pub wander_in: f32,
    pub home: Vec3,
    pub speed: f32,
    pub rider: Option<u32>,
    pub rideable: bool,
    pub seat: f32,
    /// Floats with no gravity, rising or sinking at `climb` (world 0.9).
    pub flies: bool,
    /// How fast a flier wants to rise (or sink, below 0), in blocks a second.
    pub climb: f32,
    pub says: Option<(String, f32)>,
    pub on_ground: bool,
    pub face: Option<f32>,
    pub parts: Vec<Part>,
    /// Bumped when the look changes, so views rebuild.
    pub rev: u32,
    pub phase: f32,
    /// Points it goes through in turn (`creature:follow`, world 0.23), with
    /// no gravity, as a cart on rails.
    pub path: VecDeque<Vec3>,
    /// The speed it was told to go along its path, and the speed it's
    /// going, which eases toward it.
    pub path_speed: f32,
    pub along: f32,
    /// Whether a rider steers it (a kind's `steer`, world 0.23).
    pub steer: bool,
}

impl Body {
    pub fn restyle(&mut self, k: &KindDef) {
        if self.parts != k.parts {
            self.parts = k.parts.clone();
            self.rev += 1;
        }
        self.half = k.half;
        self.speed = k.speed;
        self.rideable = k.rideable;
        self.seat = k.seat;
        self.flies = k.flies;
        self.steer = k.steer;
        if !k.home_here {
            self.home = k.home;
        }
        if !k.rideable {
            self.rider = None;
        }
    }

    pub fn forward(&self) -> Vec3 {
        Vec3::new(-self.yaw.sin(), 0.0, -self.yaw.cos())
    }

    /// Where a rider's feet go.
    pub fn saddle(&self) -> Vec3 {
        self.pos + Vec3::Y * (self.seat - 0.55) + self.forward() * 0.1
    }

    /// The box round all of it, in its own space (from its feet, facing -z):
    /// its parts and its collision box, which a long body's head and tail
    /// reach past.
    pub fn bounds(&self) -> (Vec3, Vec3) {
        let mut lo = Vec3::new(-self.half.x, 0.0, -self.half.z);
        let mut hi = Vec3::new(self.half.x, self.half.y, self.half.z);
        for p in &self.parts {
            lo = lo.min(p.at - p.size / 2.0);
            hi = hi.max(p.at + p.size / 2.0);
        }
        (lo, hi)
    }

    /// How far from its middle it reaches along the ground (to its nose, say).
    pub fn reach(&self) -> f32 {
        let (lo, hi) = self.bounds();
        lo.abs().max(hi.abs()).xz().max_element()
    }

    /// Where a ray from `from` along `dir` first meets this body, if it
    /// does: anywhere on it, turned the way it faces.
    pub fn ray(&self, from: Vec3, dir: Vec3) -> Option<f32> {
        let (lo, hi) = self.bounds();
        let back = Quat::from_rotation_y(-self.yaw);
        let from = back * (from - self.pos);
        let dir = back * dir;
        let inv = dir.recip();
        let t1 = (lo - from) * inv;
        let t2 = (hi - from) * inv;
        let tmin = t1.min(t2).max_element();
        let tmax = t1.max(t2).min_element();
        if tmax >= tmin.max(0.0) { Some(tmin.max(0.0)) } else { None }
    }
}

#[derive(Resource, Default)]
pub struct Herd {
    pub bodies: BTreeMap<u64, Body>,
    pub next: u64,
    /// What each rider's controls say: (move, yaw).
    pub riders: HashMap<u32, (Vec2, f32)>,
}

impl Herd {
    pub fn spawn(&mut self, creation: &str, k: &KindDef, at: Vec3) -> u64 {
        self.next += 1;
        let id = self.next;
        let mut b = Body {
            id,
            creation: creation.to_string(),
            kind: k.name.clone(),
            born: k.name.clone(),
            pos: at,
            vel: Vec3::ZERO,
            yaw: (id as f32 * 2.1) % 6.28,
            half: k.half,
            goal: None,
            wander: true,
            wander_in: 0.0,
            home: k.home,
            speed: k.speed,
            rider: None,
            rideable: k.rideable,
            seat: k.seat,
            flies: k.flies,
            climb: 0.0,
            says: None,
            on_ground: false,
            face: None,
            parts: vec![],
            rev: 0,
            phase: 0.0,
            path: VecDeque::new(),
            path_speed: 0.0,
            along: 0.0,
            steer: true,
        };
        b.restyle(k);
        if k.home_here {
            b.home = at;
        }
        self.bodies.insert(id, b);
        id
    }

    /// The nearest body a ray hits within reach.
    pub fn pick(&self, from: Vec3, dir: Vec3, reach: f32) -> Option<(u64, f32)> {
        self.bodies
            .values()
            .filter_map(|b| b.ray(from, dir).map(|t| (b.id, t)))
            .filter(|(_, t)| *t <= reach)
            .min_by(|a, b| a.1.total_cmp(&b.1))
    }
}

fn turn_toward(from: f32, to: f32, max: f32) -> f32 {
    let mut d = (to - from) % std::f32::consts::TAU;
    if d > std::f32::consts::PI {
        d -= std::f32::consts::TAU;
    }
    if d < -std::f32::consts::PI {
        d += std::f32::consts::TAU;
    }
    from + d.clamp(-max, max)
}

/// How quickly a flier's speed through the air follows what it wants (per second).
const FLIER_GRIP: f32 = 0.7;
/// How quickly a flier's climb follows what it was told (per second).
const FLIER_EASE: f32 = 0.8;
/// How far above the world's top a flier may go.
pub const FLY_ABOVE: f32 = 24.0;
/// The fastest a flier climbs or sinks (`creature:climb`).
pub const MAX_CLIMB: f32 = 4.0;
/// The most points a creature's path holds (`creature:follow`, world 0.23).
pub const MAX_PATH: usize = 128;
/// The fastest a creature goes along a path, in blocks a second.
pub const MAX_PATH_SPEED: f32 = 12.0;
/// How quickly it gathers or loses speed along one, in blocks a second each
/// second: gently, for whoever rides it.
pub const PATH_EASE: f32 = 2.5;
/// How quickly it slows to stop at the end of one: a little gentler, so it
/// can keep to it exactly.
const PATH_STOP: f32 = 2.0;

/// How far a body has still to go along its path.
pub fn path_left(b: &Body) -> f32 {
    let mut at = b.pos;
    let mut left = 0.0;
    for p in &b.path {
        left += at.distance(*p);
        at = *p;
    }
    left
}

/// Moves a body along its path (world 0.23): at the speed it was told,
/// gathering and losing speed gently and slowing to stop at the end, with
/// no gravity and nothing in its way, as a cart on rails. It rests on the
/// path's last point until it's given more, or let go (`follow({})`).
fn follow(b: &mut Body, dt: f32) {
    let from = b.pos;
    let ease = PATH_EASE * dt;
    b.along = (b.along + (b.path_speed - b.along).clamp(-ease, ease)).max(0.0);
    // never faster than it can stop in before the end
    b.along = b.along.min((2.0 * PATH_STOP * path_left(b)).sqrt());
    let mut go = b.along * dt;
    while go > 0.0 {
        let Some(&next) = b.path.front() else { break };
        let d = next.distance(b.pos);
        if d <= go {
            b.pos = next;
            go -= d;
            if b.path.len() == 1 {
                b.along = 0.0;
                break;
            }
            b.path.pop_front();
        } else {
            b.pos += (next - b.pos) / d * go;
            go = 0.0;
        }
    }
    b.vel = (b.pos - from) / dt.max(1e-4);
    b.on_ground = true;
    let flat = Vec2::new(b.vel.x, b.vel.z);
    if flat.length() > 0.3 {
        b.yaw = turn_toward(b.yaw, (-flat.x).atan2(-flat.y), dt * 12.0);
    }
}

/// How high a walking creature steps up onto what's in its way.
pub const STEP_UP: f32 = 1.0;

/// Where a body walking by `d` from `from` ends up if it steps up onto what
/// stopped it: lifted a block, moved on and settled onto the top. None when
/// there's no room above, it's taller than a block, or it's a fence, which
/// keeps creatures in as it would anywhere.
pub fn step_up(voxels: &Voxels, from: Vec3, d: Vec3, half: Vec3) -> Option<Vec3> {
    let mut p = from;
    if move_body(voxels, &mut p, Vec3::Y * STEP_UP, half.x, half.y).y {
        return None;
    }
    move_body(voxels, &mut p, Vec3::new(d.x, 0.0, d.z), half.x, half.y);
    let gone = Vec2::new(p.x - from.x, p.z - from.z).length();
    move_body(voxels, &mut p, Vec3::Y * -STEP_UP, half.x, half.y);
    if p.y < from.y + 0.5 || gone < 1e-4 {
        return None;
    }
    // what it would stand on
    let (lo, hi) = ((p - half).floor().as_ivec3(), (p + half - Vec3::splat(1e-4)).floor().as_ivec3());
    let y = p.y.round() as i32 - 1;
    for z in lo.z..=hi.z {
        for x in lo.x..=hi.x {
            if voxels.get(x, y, z) == crate::blocks::FENCE {
                return None;
            }
        }
    }
    Some(p)
}

/// Moves every creature one frame (on the host, or alone).
pub fn step(herd: &mut Herd, voxels: &Voxels, dt: f32, rng: &mut Rng) {
    let riders = herd.riders.clone();
    for b in herd.bodies.values_mut() {
        // where the land isn't made (nobody's near), it waits for it
        if !voxels.loaded(b.pos.x.floor() as i32, b.pos.z.floor() as i32) {
            continue;
        }
        // on a path: along it, and nothing else moves it
        if !b.path.is_empty() {
            follow(b, dt);
            b.phase += Vec2::new(b.vel.x, b.vel.z).length() * dt * 3.5;
            if let Some((_, t)) = &mut b.says {
                *t -= dt;
                if *t <= 0.0 {
                    b.says = None;
                }
            }
            continue;
        }
        let mut want = Vec3::ZERO;
        if let Some(r) = b.rider.filter(|_| b.steer) {
            // a flier is steered in the air; on the ground it stays put
            if let Some((mv, yaw)) = riders.get(&r).filter(|_| !(b.flies && b.on_ground)) {
                let fwd = Vec3::new(-yaw.sin(), 0.0, -yaw.cos());
                let right = Vec3::new(-fwd.z, 0.0, fwd.x);
                want = (fwd * mv.y + right * mv.x) * b.speed.max(1.0) * 1.6;
            }
        } else {
            if b.wander && b.goal.is_none() {
                b.wander_in -= dt;
                if b.wander_in <= 0.0 {
                    b.wander_in = 3.0 + rng.next() * 5.0;
                    if rng.next() < 0.7 {
                        let a = rng.next() * std::f32::consts::TAU;
                        let r = 2.0 + rng.next() * 5.0;
                        let mut g = b.pos + Vec3::new(a.cos() * r, 0.0, a.sin() * r);
                        // stay within reach of home
                        let off = g - b.home;
                        if Vec2::new(off.x, off.z).length() > 14.0 {
                            g = b.home + (b.pos - b.home) * 0.5;
                        }
                        b.goal = Some(g);
                    }
                }
            }
            if let Some(g) = b.goal {
                let d = Vec3::new(g.x - b.pos.x, 0.0, g.z - b.pos.z);
                if d.length() > 0.7 {
                    let s = if b.wander { b.speed * 0.55 } else { b.speed };
                    want = d.normalize() * s;
                } else {
                    b.goal = None;
                }
            }
        }
        // a flier in the air drifts: it gathers speed and slows down gently
        let grip = if b.on_ground { 8.0 } else if b.flies { FLIER_GRIP } else { 2.0 };
        b.vel.x += (want.x - b.vel.x) * (dt * grip).min(1.0);
        b.vel.z += (want.z - b.vel.z) * (dt * grip).min(1.0);
        let feet = b.pos.floor().as_ivec3();
        let wet = voxels.at(feet + IVec3::Y * (b.half.y * 0.5) as i32) == crate::blocks::WATER || voxels.at(feet) == crate::blocks::WATER;
        if b.flies {
            // no gravity: it eases toward the climb it was told, so a rider's
            // view rises and settles smoothly, never with a jolt
            // and slows as it nears the top of the sky, so it stops there gently too
            let to = b.climb.min((crate::world::WY as f32 + FLY_ABOVE - b.pos.y) * 0.5);
            b.vel.y += (to - b.vel.y) * (dt * FLIER_EASE).min(1.0);
            if wet {
                b.vel.y = b.vel.y.max(0.0); // it floats
            }
        } else if wet {
            b.vel.y = (b.vel.y + 14.0 * dt).min(2.0);
        } else {
            b.vel.y = (b.vel.y - 30.0 * dt).max(-50.0);
        }
        let mut pos = b.pos;
        let mut hit = move_body(voxels, &mut pos, b.vel * dt, b.half.x, b.half.y);
        let mut landed = hit.y && b.vel.y < 0.0;
        if (hit.x || hit.z) && (landed || b.on_ground || wet) && want.length() > 0.1 && !b.flies {
            // walking into something one block high, it steps up onto it:
            // never a hop, so nothing bounces against a bank or a wall
            if let Some(up) = step_up(voxels, b.pos, b.vel * dt, b.half) {
                pos = up;
                hit = BVec3::new(false, true, false);
                landed = true;
            } else if b.wander {
                b.goal = None; // somewhere else, then
            }
        }
        if hit.y {
            b.vel.y = 0.0;
        }
        if hit.x { b.vel.x = 0.0 }
        if hit.z { b.vel.z = 0.0 }
        b.on_ground = landed;
        b.pos = pos;
        let flat = Vec2::new(b.vel.x, b.vel.z);
        if let Some(f) = b.face {
            b.yaw = turn_toward(b.yaw, f, dt * 4.0);
            if (b.yaw - f).abs() < 0.05 || flat.length() > 0.5 {
                b.face = None;
            }
        } else if flat.length() > 0.3 {
            b.yaw = turn_toward(b.yaw, (-flat.x).atan2(-flat.y), dt * 5.0);
        }
        b.phase += flat.length() * dt * 3.5;
        if let Some((_, t)) = &mut b.says {
            *t -= dt;
            if *t <= 0.0 {
                b.says = None;
            }
        }
        if b.pos.y < crate::world::MIN_Y as f32 - 10.0 {
            b.pos = b.home + Vec3::Y * 3.0;
            b.vel = Vec3::ZERO;
        }
    }
}

// ---------- drawing ----------

#[derive(Component)]
pub struct BodyView {
    pub id: u64,
    rev: u32,
}

#[derive(Component)]
pub struct PartView {
    index: usize,
}

/// The looks parts share: a material for each colour, glow and skin, the
/// skins' textures, and a mesh for each size and skin.
#[derive(Resource, Default)]
pub struct PartMaterials {
    mats: HashMap<([u8; 3], bool, Skin), Handle<StandardMaterial>>,
    skins: HashMap<Skin, [Handle<Image>; 3]>,
    meshes: HashMap<([u16; 3], Skin), Handle<Mesh>>,
    /// The catchlight dark eyes wear: a unit box and its glossy white.
    glint: Option<(Handle<Mesh>, Handle<StandardMaterial>)>,
    /// Models' pictures, by (creation, file, texture, stamp); none when the
    /// game hasn't the file or can't read it.
    pictures: HashMap<(String, String, u32, u32), Option<Handle<Image>>>,
    painted_mats: HashMap<(AssetId<Image>, [u16; 3], bool), Handle<StandardMaterial>>,
    painted_meshes: HashMap<([u16; 3], [[u16; 4]; 6]), Handle<Mesh>>,
}

/// A part's box, `size` across, showing each face's rectangle of a model's
/// picture (as `Painted::faces` has them), with no face where the model
/// leaves one bare. Drawn normally, the sides are a little darker than the
/// top, as Blockbench shows them.
pub fn painted_mesh(size: Vec3, faces: &[Option<[f32; 4]>; 6], colours: bool) -> Mesh {
    use bevy::asset::RenderAssetUsages;
    use bevy::mesh::{Indices, PrimitiveTopology};
    let h = size / 2.0;
    let sides = [
        (Vec3::X, Vec3::NEG_Z, Vec3::Y),
        (Vec3::NEG_X, Vec3::Z, Vec3::Y),
        (Vec3::Z, Vec3::X, Vec3::Y),
        (Vec3::NEG_Z, Vec3::NEG_X, Vec3::Y),
        (Vec3::Y, Vec3::X, Vec3::NEG_Z),
        (Vec3::NEG_Y, Vec3::X, Vec3::Z),
    ];
    let (mut pos, mut norm, mut uv, mut col, mut idx) = (vec![], vec![], vec![], vec![], vec![]);
    for ((n, r, up), rect) in sides.into_iter().zip(faces) {
        let Some([u0, v0, u1, v1]) = *rect else { continue };
        let (hn, hr, hu) = ((n * h).length(), (r * h).length(), (up * h).length());
        let base = pos.len() as u32;
        let k = if n.y > 0.5 { 1.0 } else if n.y < -0.5 { 0.8 } else { 0.92 };
        for (a, b) in [(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            pos.push((n * hn + r * hr * a + up * hu * b).to_array());
            norm.push(n.to_array());
            uv.push([u0 + (u1 - u0) * (a + 1.0) / 2.0, v0 + (v1 - v0) * (1.0 - b) / 2.0]);
            col.push([k, k, k, 1.0]);
        }
        idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, norm)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
        .with_inserted_indices(Indices::U32(idx));
    if colours {
        m.insert_attribute(Mesh::ATTRIBUTE_COLOR, col);
    }
    m
}

/// A part's box, `size` across, with its skin tiling at the same scale on
/// every face (strands running down the sides) and, drawn normally, shading
/// that darkens toward its underside like a painted model. Ray traced
/// meshes carry no colours, so there the light does that alone.
pub fn part_mesh(size: Vec3, skin: Skin, colours: bool) -> Mesh {
    use bevy::asset::RenderAssetUsages;
    use bevy::mesh::{Indices, PrimitiveTopology};
    let h = size / 2.0;
    let tile = skin.tile();
    // each face: its normal, the way across it and the way up it
    let faces = [
        (Vec3::X, Vec3::NEG_Z, Vec3::Y),
        (Vec3::NEG_X, Vec3::Z, Vec3::Y),
        (Vec3::Z, Vec3::X, Vec3::Y),
        (Vec3::NEG_Z, Vec3::NEG_X, Vec3::Y),
        (Vec3::Y, Vec3::X, Vec3::NEG_Z),
        (Vec3::NEG_Y, Vec3::X, Vec3::Z),
    ];
    let (mut pos, mut norm, mut uv, mut col, mut idx) = (vec![], vec![], vec![], vec![], vec![]);
    for (n, r, up) in faces {
        let (hn, hr, hu) = ((n * h).length(), (r * h).length(), (up * h).length());
        let base = pos.len() as u32;
        for (a, b) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            let p = n * hn + r * hr * a + up * hu * b;
            pos.push(p.to_array());
            norm.push(n.to_array());
            uv.push([(a + 1.0) * hr / tile, (1.0 - b) * hu / tile]);
            // lighter on top, darker underneath, and the sides a little
            // darker than the top, so a body keeps its shape in the shade
            // and far off
            let face = if n.y > 0.5 { 1.0 } else if n.y < -0.5 { 0.8 } else { 0.92 };
            let k = if skin.shaded() { face * (0.66 + 0.34 * ((p.y + h.y) / size.y.max(1e-3)).clamp(0.0, 1.0)) } else { 1.0 };
            col.push([k, k, k, 1.0]);
        }
        idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, norm)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
        .with_inserted_indices(Indices::U32(idx));
    if colours {
        m.insert_attribute(Mesh::ATTRIBUTE_COLOR, col);
    }
    m.with_generated_tangents().expect("a box has UVs and normals")
}

pub fn draw(
    mut commands: Commands,
    herd: Res<Herd>,
    mut views: Query<(Entity, &mut BodyView, &mut Transform, Option<&Children>)>,
    mut parts: Query<(&PartView, &mut Transform), Without<BodyView>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut cache: ResMut<PartMaterials>,
    rt: Option<Res<crate::render::Raytraced>>,
    lib: Res<crate::creations::Library>,
    time: Res<Time>,
) {
    let dt = time.delta_secs();
    let mut looks = Looks { meshes: &mut meshes, mats: &mut mats, images: &mut images, cache: &mut cache, rt: rt.is_some_and(|r| r.0), lib: Some(&lib) };
    let mut seen = std::collections::HashSet::new();
    for (e, mut v, mut t, children) in &mut views {
        let Some(b) = herd.bodies.get(&v.id) else {
            commands.entity(e).despawn();
            continue;
        };
        seen.insert(b.id);
        t.translation = shown_at(t.translation, b.pos, dt);
        t.rotation = Quat::from_rotation_y(b.yaw);
        if v.rev != b.rev {
            v.rev = b.rev;
            if let Some(ch) = children {
                for c in ch.iter() {
                    commands.entity(c).despawn();
                }
            }
            spawn_parts(&mut commands, e, b, &mut looks);
            continue;
        }
        if let Some(ch) = children {
            for c in ch.iter() {
                if let Ok((pv, mut pt)) = parts.get_mut(c) {
                    let Some(p) = b.parts.get(pv.index) else { continue };
                    pose(p, pv.index, b.phase, &mut pt);
                }
            }
        }
    }
    for b in herd.bodies.values() {
        if !seen.contains(&b.id) {
            let e = commands.spawn((BodyView { id: b.id, rev: b.rev }, Transform::from_translation(b.pos), Visibility::default())).id();
            spawn_parts(&mut commands, e, b, &mut looks);
        }
    }
}

/// How fast a step up is shown climbing, in blocks a second.
const CLIMB_SHOWN: f32 = 6.0;

/// Where a body is drawn this frame, following where it is: a step up a
/// block shows as a quick climb rather than a pop.
pub fn shown_at(was: Vec3, now: Vec3, dt: f32) -> Vec3 {
    let mut at = was.lerp(now, 0.6);
    let rise = now.y - was.y;
    if rise > 0.3 && rise < 2.0 {
        at.y = was.y + (CLIMB_SHOWN * dt).min(rise);
    }
    at
}

fn pose(p: &Part, i: usize, phase: f32, t: &mut Transform) {
    if p.leg {
        let swing = (phase + if i % 2 == 0 { 0.0 } else { std::f32::consts::PI }).sin() * 0.6;
        let rot = Quat::from_rotation_x(swing);
        let pivot = p.at + Vec3::Y * p.size.y / 2.0;
        t.translation = pivot + rot * Vec3::new(0.0, -p.size.y / 2.0, 0.0);
        t.rotation = rot;
    } else if p.head {
        t.translation = p.at + Vec3::Y * (phase * 2.0).sin() * 0.03;
    }
}

/// What drawing parts needs at hand.
struct Looks<'a> {
    meshes: &'a mut Assets<Mesh>,
    mats: &'a mut Assets<StandardMaterial>,
    images: &'a mut Assets<Image>,
    cache: &'a mut PartMaterials,
    rt: bool,
    /// The creations' files, for models' pictures (bodies only).
    lib: Option<&'a crate::creations::Library>,
}

impl Looks<'_> {
    fn material(&mut self, p: &Part, skin: Skin) -> Handle<StandardMaterial> {
        let key = (p.color.map(|v| (v * 255.0) as u8), p.glow, skin);
        if let Some(m) = self.cache.mats.get(&key) {
            return m.clone();
        }
        let images = &mut *self.images;
        let [color, normal, rough] = self
            .cache
            .skins
            .entry(skin)
            .or_insert_with(|| {
                let s = paint(skin);
                [images.add(s.color), images.add(s.normal), images.add(s.rough)]
            })
            .clone();
        let col = Color::srgb(p.color[0], p.color[1], p.color[2]);
        let textured = skin != Skin::Eye;
        let m = self.mats.add(StandardMaterial {
            base_color: col,
            base_color_texture: textured.then_some(color),
            normal_map_texture: textured.then_some(normal),
            metallic_roughness_texture: textured.then_some(rough),
            perceptual_roughness: skin.roughness(),
            reflectance: if skin == Skin::Eye { 0.6 } else { 0.4 },
            emissive: if p.glow { col.to_linear() * 4.0 } else { LinearRgba::BLACK },
            ..default()
        });
        self.cache.mats.insert(key, m.clone());
        m
    }

    /// A small glossy highlight for a dark eye, high on its front.
    fn glint(&mut self, p: &Part) -> Option<(Handle<Mesh>, Handle<StandardMaterial>, Transform)> {
        let dark = p.color[0] * 0.3 + p.color[1] * 0.59 + p.color[2] * 0.11 < 0.35;
        if Skin::of(&p.name) != Skin::Eye || !dark {
            return None;
        }
        let rt = self.rt;
        let (mesh, mat) = self
            .cache
            .glint
            .get_or_insert_with(|| {
                let mesh = self.meshes.add(part_mesh(Vec3::ONE, Skin::Eye, !rt));
                let mat = self.mats.add(StandardMaterial { base_color: Color::srgb(0.95, 0.95, 0.95), perceptual_roughness: 0.15, ..default() });
                (mesh, mat)
            })
            .clone();
        let s = p.size.x.min(p.size.y) * 0.34;
        let at = Vec3::new(p.size.x * 0.2, p.size.y * 0.2, -p.size.z / 2.0 - 0.003);
        Some((mesh, mat, Transform::from_translation(at).with_scale(Vec3::new(s, s, 0.004))))
    }

    /// A part's picture from its creation's model, as a mesh and material,
    /// when this game has the model and can read it.
    fn painted(&mut self, creation: &str, p: &Part) -> Option<(Handle<Mesh>, Handle<StandardMaterial>)> {
        let pic = p.paint.as_ref()?;
        let lib = self.lib?;
        let images = &mut *self.images;
        let image = self
            .cache
            .pictures
            .entry((creation.to_string(), pic.file.clone(), pic.texture, pic.stamp))
            .or_insert_with(|| {
                let text = lib.list.get(creation)?.files.get(&pic.file)?;
                let rgba = crate::model::image(text, pic.texture)?;
                let mut img = Image::new(
                    bevy::render::render_resource::Extent3d { width: rgba.width(), height: rgba.height(), depth_or_array_layers: 1 },
                    bevy::render::render_resource::TextureDimension::D2,
                    rgba.into_raw(),
                    bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
                    bevy::asset::RenderAssetUsages::RENDER_WORLD,
                );
                img.sampler = bevy::image::ImageSampler::nearest();
                Some(images.add(img))
            })
            .clone()?;
        let tint = pic.tint.map(|t| (t * 1000.0).round() as u16);
        let mats = &mut *self.mats;
        let mat = self
            .cache
            .painted_mats
            .entry((image.id(), tint, p.glow))
            .or_insert_with(|| {
                let col = Color::srgb(pic.tint[0], pic.tint[1], pic.tint[2]);
                mats.add(StandardMaterial {
                    base_color: col,
                    base_color_texture: Some(image.clone()),
                    emissive: if p.glow { col.to_linear() * 2.0 } else { LinearRgba::BLACK },
                    emissive_texture: p.glow.then(|| image.clone()),
                    alpha_mode: AlphaMode::Mask(0.5),
                    perceptual_roughness: 0.85,
                    reflectance: 0.3,
                    ..default()
                })
            })
            .clone();
        let key = (p.size.to_array().map(|v| (v * 256.0).round().clamp(1.0, 65535.0) as u16), pic.faces.map(|f| f.map_or([u16::MAX; 4], |r| r.map(|x| (x * 4096.0).round() as u16))));
        let rt = self.rt;
        let meshes = &mut *self.meshes;
        let mesh = self.cache.painted_meshes.entry(key).or_insert_with(|| meshes.add(painted_mesh(p.size, &pic.faces, !rt))).clone();
        Some((mesh, mat))
    }

    fn mesh(&mut self, p: &Part, skin: Skin) -> Handle<Mesh> {
        let key = (p.size.to_array().map(|v| (v * 256.0).round().clamp(1.0, 65535.0) as u16), skin);
        let rt = self.rt;
        let meshes = &mut *self.meshes;
        self.cache.meshes.entry(key).or_insert_with(|| meshes.add(part_mesh(p.size, skin, !rt))).clone()
    }
}

fn spawn_parts(commands: &mut Commands, e: Entity, b: &Body, looks: &mut Looks) {
    let parts: Vec<_> = b
        .parts
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let mut t = Transform::from_translation(p.at);
            pose(p, i, b.phase, &mut t);
            if let Some((mesh, mat)) = looks.painted(&b.creation, p) {
                return (mesh, mat, t, None);
            }
            let skin = Skin::of(&p.name);
            (looks.mesh(p, skin), looks.material(p, skin), t, looks.glint(p))
        })
        .collect();
    commands.entity(e).with_children(|c| {
        for (i, (mesh, mat, t, glint)) in parts.into_iter().enumerate() {
            let mut part = c.spawn((Mesh3d(mesh), MeshMaterial3d(mat), t, PartView { index: i }));
            if let Some((mesh, mat, t)) = glint {
                part.with_child((Mesh3d(mesh), MeshMaterial3d(mat), t));
            }
        }
    });
}

/// A player's body, for other players and for you seen from behind: `net`
/// places and turns it, and `people` dresses it in painted parts (shirt,
/// trousers, a belt, a face with eyes, hair) and swings its arms and legs
/// as it moves. The shirt's hue picks the skin and hair too.
#[derive(Component)]
pub struct Person {
    parts: Vec<Part>,
    /// Each swinging part's pivot (a hip or a shoulder) and where in the
    /// stride it starts.
    limbs: Vec<Option<(Vec3, f32)>>,
    dressed: bool,
    last: Option<Vec3>,
    phase: f32,
    stride: f32,
}

impl Person {
    pub fn new(hue: f32) -> Self {
        let parts = person_parts(hue);
        // hands hang from the shoulders; each arm swings with the other leg
        let limbs = parts
            .iter()
            .map(|q| {
                q.leg.then(|| {
                    let arm = q.at.y > 0.9 || q.name.contains("hand");
                    let top = if arm { 1.47 } else { q.at.y + q.size.y / 2.0 };
                    let left = q.at.x < 0.0;
                    (Vec3::new(q.at.x, top, q.at.z), if left != arm { 0.0 } else { std::f32::consts::PI })
                })
            })
            .collect();
        Self { parts, limbs, dressed: false, last: None, phase: 0.0, stride: 0.0 }
    }
}

/// A person's parts, feet at the origin, facing -z; legs, arms and hands
/// are marked `leg`, to swing.
pub fn person_parts(hue: f32) -> Vec<Part> {
    let pick = |list: &[[f32; 3]], salt: i32| list[(crate::noise::hash2(hue as i32, salt, 77) * list.len() as f32) as usize % list.len()];
    let skin = pick(&[[0.96, 0.8, 0.69], [0.87, 0.68, 0.53], [0.72, 0.53, 0.38], [0.55, 0.38, 0.26], [0.4, 0.27, 0.18]], 1);
    let hair = pick(&[[0.12, 0.09, 0.07], [0.3, 0.19, 0.1], [0.58, 0.4, 0.2], [0.78, 0.62, 0.36], [0.5, 0.2, 0.1]], 2);
    let shirt = Color::hsl(hue, 0.55, 0.55).to_srgba();
    let shirt = [shirt.red, shirt.green, shirt.blue];
    let sleeve = shirt.map(|c| c * 0.88);
    let trousers = pick(&[[0.2, 0.25, 0.4], [0.28, 0.24, 0.2], [0.22, 0.24, 0.22], [0.34, 0.36, 0.4]], 3);
    let part = |name: &str, size: [f32; 3], at: [f32; 3], color: [f32; 3], leg: bool| Part {
        name: name.into(),
        size: Vec3::from_array(size),
        at: Vec3::from_array(at),
        color,
        leg,
        head: false,
        glow: false,
        paint: None,
    };
    let mut parts = vec![
        part("leg cloth", [0.25, 0.8, 0.27], [-0.13, 0.4, 0.0], trousers, true),
        part("leg cloth", [0.25, 0.8, 0.27], [0.13, 0.4, 0.0], trousers, true),
        part("sleeve cloth", [0.18, 0.5, 0.2], [0.375, 1.22, 0.0], sleeve, true),
        part("sleeve cloth", [0.18, 0.5, 0.2], [-0.375, 1.22, 0.0], sleeve, true),
        part("hand", [0.16, 0.16, 0.18], [0.375, 0.89, 0.0], skin, true),
        part("hand", [0.16, 0.16, 0.18], [-0.375, 0.89, 0.0], skin, true),
        part("shirt cloth", [0.54, 0.68, 0.3], [0.0, 1.14, 0.0], shirt, false),
        part("belt", [0.555, 0.07, 0.31], [0.0, 0.84, 0.0], [0.3, 0.2, 0.12], false),
        part("face", [0.44, 0.44, 0.42], [0.0, 1.7, 0.0], skin, false),
        part("hair", [0.47, 0.12, 0.45], [0.0, 1.94, 0.01], hair, false),
        part("hair", [0.47, 0.3, 0.1], [0.0, 1.8, 0.2], hair, false),
        part("hair", [0.47, 0.08, 0.04], [0.0, 1.86, -0.215], hair, false),
        part("face", [0.13, 0.03, 0.02], [0.0, 1.6, -0.215], skin.map(|c| c * 0.62), false),
    ];
    for side in [-1.0, 1.0] {
        parts.push(part("eye", [0.1, 0.07, 0.02], [0.1 * side, 1.74, -0.215], [0.95, 0.95, 0.92], false));
        parts.push(part("pupil", [0.05, 0.07, 0.02], [0.08 * side, 1.74, -0.225], [0.1, 0.08, 0.06], false));
    }
    parts
}

/// Dresses new people and walks the rest: arms and legs swing with the
/// distance moved, settling straight when they stand.
pub fn people(
    mut commands: Commands,
    time: Res<Time>,
    mut people: Query<(Entity, &mut Person, &Transform, Option<&Children>)>,
    mut limbs: Query<(&PartView, &mut Transform), Without<Person>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut cache: ResMut<PartMaterials>,
    rt: Option<Res<crate::render::Raytraced>>,
) {
    let mut looks = Looks { meshes: &mut meshes, mats: &mut mats, images: &mut images, cache: &mut cache, rt: rt.is_some_and(|r| r.0), lib: None };
    let dt = time.delta_secs().max(1e-4);
    for (e, mut p, t, children) in &mut people {
        if !p.dressed {
            p.dressed = true;
            let parts: Vec<_> = p.parts.iter().map(|q| (looks.mesh(q, Skin::of(&q.name)), looks.material(q, Skin::of(&q.name)), Transform::from_translation(q.at), looks.glint(q))).collect();
            commands.entity(e).with_children(|c| {
                for (i, (mesh, mat, t, glint)) in parts.into_iter().enumerate() {
                    let mut part = c.spawn((Mesh3d(mesh), MeshMaterial3d(mat), t, PartView { index: i }));
                    if let Some((mesh, mat, t)) = glint {
                        part.with_child((Mesh3d(mesh), MeshMaterial3d(mat), t));
                    }
                }
            });
            continue;
        }
        let moved = p.last.map_or(0.0, |l| (t.translation - l).xz().length());
        p.last = Some(t.translation);
        // a teleport isn't a step
        let moved = if moved > 2.0 { 0.0 } else { moved };
        p.phase += moved * 3.2;
        let want = (moved / dt / 4.0).min(1.0);
        p.stride += (want - p.stride) * (dt * 8.0).min(1.0);
        let Some(children) = children else { continue };
        for c in children.iter() {
            let Ok((v, mut lt)) = limbs.get_mut(c) else { continue };
            let (Some(q), Some(Some((pivot, start)))) = (p.parts.get(v.index), p.limbs.get(v.index)) else { continue };
            let rot = Quat::from_rotation_x((p.phase + start).sin() * 0.6 * p.stride);
            lt.translation = *pivot + rot * (q.at - *pivot);
            lt.rotation = rot;
        }
    }
}

/// Keeps the local player in the saddle. A friend's game hears where its
/// ride is twenty times a second, so there the view glides after it rather
/// than stepping (a balloon's rider would feel every step).
pub fn ride(herd: Res<Herd>, net: Option<Res<crate::net::Net>>, time: Res<Time>, mut me: ResMut<Me>) {
    if let Some(id) = me.riding {
        match herd.bodies.get(&id) {
            Some(b) => {
                let to = b.saddle();
                let mirrored = net.is_some_and(|n| matches!(n.role, crate::net::Role::Client(_)));
                me.pos = if mirrored && me.pos.distance(to) < 3.0 { me.pos.lerp(to, (time.delta_secs() * 12.0).min(1.0)) } else { to };
            }
            None => me.riding = None,
        }
    }
}
