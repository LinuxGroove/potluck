//! Creatures: bodies made of boxes that creations define, walking, hopping
//! and being ridden. The host moves them; everyone draws them.

use std::collections::{BTreeMap, HashMap};

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
}

#[derive(Clone, Debug)]
pub struct Body {
    pub id: u64,
    pub creation: String,
    pub kind: String,
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
    pub says: Option<(String, f32)>,
    pub on_ground: bool,
    pub face: Option<f32>,
    pub parts: Vec<Part>,
    /// Bumped when the look changes, so views rebuild.
    pub rev: u32,
    pub phase: f32,
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
        self.home = k.home;
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

    /// Where a ray from `from` along `dir` first meets this body, if it does.
    pub fn ray(&self, from: Vec3, dir: Vec3) -> Option<f32> {
        let lo = self.pos - Vec3::new(self.half.x, 0.0, self.half.z);
        let hi = self.pos + Vec3::new(self.half.x, self.half.y, self.half.z);
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
            says: None,
            on_ground: false,
            face: None,
            parts: vec![],
            rev: 0,
            phase: 0.0,
        };
        b.restyle(k);
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

/// Moves every creature one frame (on the host, or alone).
pub fn step(herd: &mut Herd, voxels: &Voxels, dt: f32, rng: &mut Rng) {
    let riders = herd.riders.clone();
    for b in herd.bodies.values_mut() {
        let mut want = Vec3::ZERO;
        if let Some(r) = b.rider {
            if let Some((mv, yaw)) = riders.get(&r) {
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
        let grip = if b.on_ground { 8.0 } else { 2.0 };
        b.vel.x += (want.x - b.vel.x) * (dt * grip).min(1.0);
        b.vel.z += (want.z - b.vel.z) * (dt * grip).min(1.0);
        let feet = b.pos.floor().as_ivec3();
        let wet = voxels.at(feet + IVec3::Y * (b.half.y * 0.5) as i32) == crate::blocks::WATER || voxels.at(feet) == crate::blocks::WATER;
        if wet {
            b.vel.y = (b.vel.y + 14.0 * dt).min(2.0);
        } else {
            b.vel.y = (b.vel.y - 30.0 * dt).max(-50.0);
        }
        let mut pos = b.pos;
        let hit = move_body(voxels, &mut pos, b.vel * dt, b.half.x, b.half.y);
        let landed = hit.y && b.vel.y < 0.0;
        if hit.y {
            b.vel.y = 0.0;
        }
        if (hit.x || hit.z) && (landed || b.on_ground) && want.length() > 0.1 {
            b.vel.y = 7.5; // hop up a step
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
        if b.pos.y < -10.0 {
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
            // lighter on top, darker underneath
            let k = if skin.shaded() { 0.74 + 0.26 * ((p.y + h.y) / size.y.max(1e-3)).clamp(0.0, 1.0) } else { 1.0 };
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
) {
    let mut looks = Looks { meshes: &mut meshes, mats: &mut mats, images: &mut images, cache: &mut cache, rt: rt.is_some_and(|r| r.0) };
    let mut seen = std::collections::HashSet::new();
    for (e, mut v, mut t, children) in &mut views {
        let Some(b) = herd.bodies.get(&v.id) else {
            commands.entity(e).despawn();
            continue;
        };
        seen.insert(b.id);
        t.translation = t.translation.lerp(b.pos, 0.6);
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

    fn mesh(&mut self, p: &Part, skin: Skin) -> Handle<Mesh> {
        let key = (p.size.to_array().map(|v| (v * 256.0).round().clamp(1.0, 65535.0) as u16), skin);
        let rt = self.rt;
        let meshes = &mut *self.meshes;
        self.cache.meshes.entry(key).or_insert_with(|| meshes.add(part_mesh(p.size, skin, !rt))).clone()
    }
}

fn spawn_parts(commands: &mut Commands, e: Entity, b: &Body, looks: &mut Looks) {
    let parts: Vec<(Handle<Mesh>, Handle<StandardMaterial>, Transform)> = b
        .parts
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let skin = Skin::of(&p.name);
            let mut t = Transform::from_translation(p.at);
            pose(p, i, b.phase, &mut t);
            (looks.mesh(p, skin), looks.material(p, skin), t)
        })
        .collect();
    commands.entity(e).with_children(|c| {
        for (i, (mesh, mat, t)) in parts.into_iter().enumerate() {
            c.spawn((Mesh3d(mesh), MeshMaterial3d(mat), t, PartView { index: i }));
        }
    });
}

/// Keeps the local player in the saddle.
pub fn ride(herd: Res<Herd>, mut me: ResMut<Me>) {
    if let Some(id) = me.riding {
        match herd.bodies.get(&id) {
            Some(b) => me.pos = b.saddle(),
            None => me.riding = None,
        }
    }
}
