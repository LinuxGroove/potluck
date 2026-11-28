//! What's in hand, drawn: the thing in the hotbar's chosen slot at the
//! bottom right of the view in first person, and in the right hand of
//! people's bodies (`Holds`: yours seen from behind, and other players'
//! once their games say what they hold). A block is a small cube in its
//! own faces; a tool, an item or a block that isn't a cube (a torch, a
//! plant, a model) is its icon cut out and given a pixel's thickness. A
//! flame in hand lights the way round the player. The view's thing never
//! sways or bobs: it only dips out and back as it changes.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use crate::blocks::{self, Shape};
use crate::items::{self, Thing};
use crate::player::{MainCamera, Me};
use crate::render::{Clock, Eye, LanternLight, NotRaytraced};
use crate::textures::{BlockAtlas, Outline};

/// What a person holds (`items::Thing`, 0 for nothing).
#[derive(Component, Default, Clone, Copy, PartialEq, Debug)]
pub struct Holds(pub Thing);

/// A thing drawn in a hand, and what it is.
#[derive(Component)]
pub struct InHand(pub Thing);

/// The first person view's hold on its thing: what's drawn, and how far
/// it's dipped out of view (0 up, 1 gone) while it changes.
#[derive(Component)]
pub struct InView {
    shown: Thing,
    dip: f32,
}

/// The light a flame in hand gives (one, for this player), and which
/// block's.
#[derive(Component)]
pub struct HandLight(pub blocks::BlockId);

/// How a thing in hand is drawn, made once per thing (and again when a
/// creation's block gets a new picture).
#[derive(Clone)]
pub struct Look {
    /// Its meshes and their materials: a model's flame glows apart from
    /// the rest.
    pub parts: Vec<(Handle<Mesh>, Handle<StandardMaterial>)>,
    /// A block (a cube or a model), rather than a cut out picture.
    pub cube: bool,
    /// The picture's pixels that show, for a cut out one: when a
    /// creation's block is drawn again they may change.
    stamp: u64,
}

/// What the material of a thing in hand does: as the chunks' layers, but
/// with no sky light (it's held up in the air), and pictures show both
/// sides.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Kind {
    Solid,
    Cut,
    /// A flame: as bright as one set down (`mesh::GLOW`).
    Glow,
    /// Glowing a little, as crystal does (`mesh::FAINT`).
    Faint,
}

#[derive(Resource, Default)]
pub struct HeldLooks {
    looks: HashMap<Thing, Option<Look>>,
    mats: HashMap<(Kind, bool), Handle<StandardMaterial>>,
    /// The first person view's own copies of a flame's materials
    /// (`Making::for_view`).
    view: HashMap<AssetId<StandardMaterial>, Handle<StandardMaterial>>,
}

/// How bright a flame in the view's hand lights its own stick (or the
/// lantern's frame), on the screen: the view's thing is drawn small and
/// close (`CLOSE`), too close for the light round the hand to light it as
/// it would at arm's length.
const FLAME_FILL: f32 = 0.3;

/// How bright a flame in the view's hand burns, as much as one set down:
/// up at the eye it would fill the view with white.
const VIEW_GLOW: f32 = 0.4;

/// What making how a thing in hand looks needs.
#[derive(bevy::ecs::system::SystemParam)]
pub struct Making<'w> {
    atlas: Res<'w, BlockAtlas>,
    looks: ResMut<'w, HeldLooks>,
    meshes: ResMut<'w, Assets<Mesh>>,
    mats: ResMut<'w, Assets<StandardMaterial>>,
    blocks: Option<Res<'w, crate::mesh::BlockMaterials>>,
    rt: Option<Res<'w, crate::render::Raytraced>>,
}

impl Making<'_> {
    /// How `thing` looks in hand: None for nothing, or a thing with no
    /// picture.
    pub fn look(&mut self, thing: Thing) -> Option<Look> {
        if thing == 0 {
            return None;
        }
        let stamp = self.atlas.outlines.get(items::icon(thing)).map(|o| {
            use std::hash::{Hash, Hasher};
            let mut h = std::hash::DefaultHasher::new();
            o.solid.hash(&mut h);
            h.finish()
        });
        if let Some(l) = self.looks.looks.get(&thing) {
            if l.as_ref().is_none_or(|l| l.cube || Some(l.stamp) == stamp) {
                return l.clone();
            }
        }
        let look = self.make(thing, stamp.unwrap_or(0));
        self.looks.looks.insert(thing, look.clone());
        look
    }

    fn make(&mut self, thing: Thing, stamp: u64) -> Option<Look> {
        let atlas = &self.atlas;
        let block = items::block(thing).map(blocks::block);
        // a flame glows as one set down does, an ore that glows a little as
        // it does in the rock
        let glows = |names: &[&str]| names.iter().any(|n| atlas.has(n) && atlas.glows(atlas.tile(n)));
        let lit = |cut: Kind| match block {
            Some(b) if glows(&b.faces) => {
                if b.light {
                    Kind::Glow
                } else {
                    Kind::Faint
                }
            }
            _ => cut,
        };
        if let Some(b) = block.filter(|b| matches!(b.shape, Shape::Solid | Shape::Cutout)) {
            let faces = b.faces.map(|f| BlockAtlas::uv(atlas.tile(f)));
            let kind = lit(if b.shape == Shape::Cutout { Kind::Cut } else { Kind::Solid });
            let mesh = cube(faces).with_generated_tangents().expect("a cube has UVs and normals");
            return Some(Look { parts: vec![(self.meshes.add(mesh), self.material(kind, false))], cube: true, stamp });
        }
        // a model (a torch, a lantern, a fence) is itself, small, its flame
        // glowing as one set down does
        if let (Some(b), Some(id)) = (block.filter(|b| b.shape == Shape::Model), items::block(thing)) {
            let (plain, glow) = crate::mesh::model_alone(atlas, Vec3::splat(-0.5), id);
            let mut parts = Vec::new();
            for (data, kind) in [(plain, Kind::Cut), (glow, if b.light { Kind::Glow } else { Kind::Faint })] {
                if !data.is_empty() {
                    parts.push((self.meshes.add(data.into_mesh(true)), self.material(kind, false)));
                }
            }
            return (!parts.is_empty()).then_some(Look { parts, cube: true, stamp });
        }
        let name = items::icon(thing);
        let outline = atlas.outlines.get(name)?;
        let mesh = sprite(outline, BlockAtlas::uv(atlas.tile(name)));
        let kind = lit(Kind::Cut);
        Some(Look { parts: vec![(self.meshes.add(mesh), self.material(kind, true))], cube: false, stamp })
    }

    /// How `look` is drawn in the first person view: a flame's stick (or
    /// frame) lit by its flame, and the flame no brighter than the eye can
    /// take so near. Anything else is as it is.
    pub fn for_view(&mut self, look: &Look) -> Look {
        let glows: Vec<AssetId<StandardMaterial>> = self.looks.mats.iter().filter(|((k, _), _)| *k == Kind::Glow).map(|(_, h)| h.id()).collect();
        if !look.parts.iter().any(|(_, m)| glows.contains(&m.id())) {
            return look.clone();
        }
        let rt = self.rt.as_ref().is_some_and(|r| r.0);
        let mut l = look.clone();
        for (_, m) in &mut l.parts {
            if let Some(v) = self.looks.view.get(&m.id()) {
                *m = v.clone();
                continue;
            }
            let Some(mut copy) = self.mats.get(&*m).cloned() else { continue };
            if glows.contains(&m.id()) {
                // ray traced, the eye's exposure takes care of it
                if !rt {
                    copy.emissive = copy.emissive * VIEW_GLOW;
                }
            } else {
                // warm light on its own colours: on the screen drawn
                // normally, and ray traced as bright at night as that
                // (the tracer's glow goes through the eye's exposure)
                copy.emissive_texture = copy.base_color_texture.clone();
                copy.emissive = LinearRgba::rgb(1.0, 0.72, 0.42) * FLAME_FILL * if rt { 290.0 } else { 1.0 };
            }
            let h = self.mats.add(copy);
            self.looks.view.insert(m.id(), h.clone());
            *m = h;
        }
        l
    }

    /// The chunks' own material for `kind` (so flames are as bright as
    /// they are set down, on every tier), without the sky light, and for a
    /// picture both sides and no bumps.
    fn material(&mut self, kind: Kind, picture: bool) -> Handle<StandardMaterial> {
        if let Some(m) = self.looks.mats.get(&(kind, picture)) {
            return m.clone();
        }
        let layer = match kind {
            Kind::Solid => 0,
            Kind::Cut => 1,
            Kind::Glow => 3,
            Kind::Faint => 5,
        };
        let glows = matches!(kind, Kind::Glow | Kind::Faint);
        let mut m = self.blocks.as_ref().and_then(|b| self.mats.get(&b.layers[layer]).cloned()).unwrap_or_else(|| StandardMaterial {
            base_color_texture: Some(self.atlas.color.clone()),
            emissive_texture: glows.then(|| self.atlas.glow.clone()),
            emissive: if glows { LinearRgba::WHITE } else { LinearRgba::BLACK },
            perceptual_roughness: 0.9,
            reflectance: 0.3,
            alpha_mode: if kind == Kind::Solid { AlphaMode::Opaque } else { AlphaMode::Mask(0.5) },
            ..default()
        });
        m.occlusion_texture = None;
        if picture {
            // its edges are a pixel thick, too thin for bumps
            m.normal_map_texture = None;
            m.alpha_mode = AlphaMode::Mask(0.5);
            m.double_sided = true;
            m.cull_mode = None;
        }
        let h = self.mats.add(m);
        self.looks.mats.insert((kind, picture), h.clone());
        h
    }
}

/// Collects quads: positions, normals and UVs, two triangles each.
#[derive(Default)]
struct Quads {
    pos: Vec<[f32; 3]>,
    normal: Vec<[f32; 3]>,
    uv: Vec<[f32; 2]>,
    index: Vec<u32>,
}

impl Quads {
    /// A `w` by `h` quad round `c` facing `n`, its picture upright along
    /// `up` as seen from in front; `uv` for its corners from the bottom
    /// left, counterclockwise.
    fn push(&mut self, c: Vec3, n: Vec3, up: Vec3, w: f32, h: f32, uv: [[f32; 2]; 4]) {
        let r = up.cross(n) * w * 0.5;
        let u = up * h * 0.5;
        let i = self.pos.len() as u32;
        for (p, t) in [c - r - u, c + r - u, c + r + u, c - r + u].into_iter().zip(uv) {
            self.pos.push(p.to_array());
            self.normal.push(n.to_array());
            self.uv.push(t);
        }
        self.index.extend([i, i + 1, i + 2, i, i + 2, i + 3]);
    }

    fn mesh(self) -> Mesh {
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.pos)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.normal)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.uv)
            .with_inserted_indices(Indices::U32(self.index))
    }
}

/// The corners' UVs of an atlas tile's rectangle, from the bottom left.
fn corners([u0, v0, u1, v1]: [f32; 4]) -> [[f32; 2]; 4] {
    [[u0, v1], [u1, v1], [u1, v0], [u0, v0]]
}

/// A block a unit across round the origin, in its faces' tiles (top,
/// side, bottom, as `Block::faces`).
pub fn cube(faces: [[f32; 4]; 3]) -> Mesh {
    let mut q = Quads::default();
    for n in [Vec3::X, Vec3::NEG_X, Vec3::Z, Vec3::NEG_Z] {
        q.push(n * 0.5, n, Vec3::Y, 1.0, 1.0, corners(faces[1]));
    }
    q.push(Vec3::Y * 0.5, Vec3::Y, Vec3::NEG_Z, 1.0, 1.0, corners(faces[0]));
    q.push(Vec3::NEG_Y * 0.5, Vec3::NEG_Y, Vec3::Z, 1.0, 1.0, corners(faces[2]));
    q.mesh()
}

/// A picture a unit across round the origin in the XY plane, a pixel
/// thick, from the atlas tile at `rect`: its front and back show the
/// whole picture (cut out by the material), and every edge between a
/// pixel that shows and one that doesn't is a strip in that pixel's
/// colour.
pub fn sprite(o: &Outline, rect: [f32; 4]) -> Mesh {
    let n = o.n as f32;
    let t = 1.0 / n;
    let [u0, v0, u1, v1] = rect;
    let mut q = Quads::default();
    q.push(Vec3::Z * t * 0.5, Vec3::Z, Vec3::Y, 1.0, 1.0, corners(rect));
    // seen from behind it's the same picture through the thing
    q.push(Vec3::NEG_Z * t * 0.5, Vec3::NEG_Z, Vec3::Y, 1.0, 1.0, [[u1, v1], [u0, v1], [u0, v0], [u1, v0]]);
    for y in 0..o.n as i32 {
        for x in 0..o.n as i32 {
            if !o.at(x, y) {
                continue;
            }
            let (x0, y1) = (-0.5 + x as f32 / n, 0.5 - y as f32 / n);
            let mid = Vec3::new(x0 + t * 0.5, y1 - t * 0.5, 0.0);
            let uv = [u0 + (u1 - u0) * (x as f32 + 0.5) / n, v0 + (v1 - v0) * (y as f32 + 0.5) / n];
            let uv = [uv; 4];
            if !o.at(x - 1, y) {
                q.push(mid - Vec3::X * t * 0.5, Vec3::NEG_X, Vec3::Y, t, t, uv);
            }
            if !o.at(x + 1, y) {
                q.push(mid + Vec3::X * t * 0.5, Vec3::X, Vec3::Y, t, t, uv);
            }
            if !o.at(x, y - 1) {
                q.push(mid + Vec3::Y * t * 0.5, Vec3::Y, Vec3::NEG_Z, t, t, uv);
            }
            if !o.at(x, y + 1) {
                q.push(mid - Vec3::Y * t * 0.5, Vec3::NEG_Y, Vec3::Z, t, t, uv);
            }
        }
    }
    q.mesh()
}

/// How much nearer the eye than it looks the view's thing is drawn, and
/// smaller by as much, so it looks the same but stays closer than a wall
/// the player stands against (`player::HALF_W` from the eye).
const CLOSE: f32 = 0.35;

/// Where a thing sits in the first person view (the camera looks down -z).
fn in_view(cube: bool) -> Transform {
    let t = if cube {
        Transform::from_xyz(0.4, -0.33, -0.68).with_rotation(Quat::from_rotation_y(0.6)).with_scale(Vec3::splat(0.27))
    } else {
        // seen from behind, turned so the top of the picture (a tool's
        // head) reaches up, left and away, and its bottom (the handle)
        // comes down to the hand at the bottom right
        Transform::from_xyz(0.42, -0.3, -0.62).with_rotation(Quat::from_rotation_y(std::f32::consts::PI - 0.7)).with_scale(Vec3::splat(0.42))
    };
    Transform { translation: t.translation * CLOSE, scale: t.scale * CLOSE, ..t }
}

/// Where a thing sits in a person's right hand (a body faces -z): a block
/// in the palm, a tool gripped near its picture's bottom left corner,
/// pointing forward and up.
fn in_a_hand(cube: bool) -> Transform {
    if cube {
        Transform::from_xyz(0.0, -0.08, -0.14).with_rotation(Quat::from_rotation_y(0.3)).with_scale(Vec3::splat(0.25))
    } else {
        // turned a little out from the body, so it shows from in front too
        let s = 0.55;
        Transform::from_xyz(0.0, -0.03 + 0.3 * s, -0.3 * s).with_rotation(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2 - 0.5)).with_scale(Vec3::splat(s))
    }
}

/// How long the view's thing takes to dip out, and again to come back.
const DIP: f32 = 0.12;

/// Draws what this player holds: in the view in first person, and on
/// their body when the camera's behind them (`net::MyView`).
#[allow(clippy::too_many_arguments)]
pub fn mine(
    mut commands: Commands,
    me: Res<Me>,
    settings: Res<crate::settings::Settings>,
    focus: Res<crate::ui::Focus>,
    info: Res<crate::ui::WorldInfo>,
    time: Res<Time>,
    mut making: Making,
    cam: Query<Entity, With<MainCamera>>,
    mut view: Query<(Entity, &mut InView, &mut Transform, &mut Visibility, Option<&Children>)>,
    things: Query<(), With<InHand>>,
    mut body: Query<&mut Holds, With<crate::net::MyView>>,
) {
    let held = me.held(settings.survival);
    if let Ok(mut h) = body.single_mut() {
        h.set_if_neq(Holds(held));
    }
    let Ok(cam) = cam.single() else { return };
    let Ok((e, mut v, mut t, mut vis, children)) = view.single_mut() else {
        commands.entity(cam).with_child((InView { shown: 0, dip: 1.0 }, Transform::default(), Visibility::Hidden));
        return;
    };
    let show = !me.third_person && focus.screen != crate::ui::Screen::Title && info.in_world && !crate::ui::draw::blurs(&focus.screen);
    vis.set_if_neq(if show { Visibility::Inherited } else { Visibility::Hidden });
    let step = time.delta_secs() / DIP;
    let have = children.is_some_and(|c| c.iter().any(|c| things.contains(c)));
    if v.shown != held || (held != 0 && !have) {
        // dip out, change, come back
        v.dip = (v.dip + step).min(1.0);
        if v.dip >= 1.0 || !show {
            if let Some(c) = children {
                for c in c.iter().filter(|c| things.contains(*c)) {
                    commands.entity(c).despawn();
                }
            }
            v.shown = held;
            if let Some(l) = making.look(held) {
                let l = making.for_view(&l);
                let thing = commands.spawn((in_view(l.cube), Visibility::Inherited, InHand(held), ChildOf(e))).id();
                draw(&mut commands, thing, &l, true);
            }
        }
    } else {
        v.dip = (v.dip - step).max(0.0);
    }
    let k = v.dip * v.dip * (3.0 - 2.0 * v.dip);
    t.translation = Vec3::new(0.0, -0.35 * CLOSE * k, 0.0);
}

/// Hangs a look's meshes on `thing`. Nothing in hand is ray traced (it's
/// too small to light anything), and the view's casts no shadow (it's
/// only drawn in front of the eye).
fn draw(commands: &mut Commands, thing: Entity, look: &Look, view: bool) {
    for (mesh, mat) in &look.parts {
        let mut e = commands.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(mat.clone()), Transform::IDENTITY, NotRaytraced, ChildOf(thing)));
        if view {
            e.insert(NotShadowCaster);
        }
    }
}

/// Puts what each person holds in their right hand.
pub fn hands(
    mut commands: Commands,
    mut making: Making,
    people: Query<(&crate::creatures::Person, &Holds, &Children)>,
    parts: Query<(&crate::creatures::PartView, Option<&Children>)>,
    things: Query<&InHand>,
) {
    for (person, holds, children) in &people {
        let Some(hand) = children.iter().find(|c| parts.get(*c).is_ok_and(|(p, _)| person.is_right_hand(p))) else { continue };
        let Ok((_, inside)) = parts.get(hand) else { continue };
        let drawn: Vec<(Entity, Thing)> = inside.map(|c| c.iter().filter_map(|c| things.get(c).ok().map(|t| (c, t.0))).collect()).unwrap_or_default();
        if drawn.len() == usize::from(holds.0 != 0) && drawn.iter().all(|(_, t)| *t == holds.0) {
            continue;
        }
        for (c, _) in drawn {
            commands.entity(c).despawn();
        }
        if let Some(l) = making.look(holds.0) {
            let thing = commands.spawn((in_a_hand(l.cube), Visibility::Inherited, InHand(holds.0), ChildOf(hand))).id();
            draw(&mut commands, thing, &l, false);
        }
    }
}

/// A flame in this player's hand (a torch, a lantern, a candle) lights
/// the way round them, as bright as one set down would be (and, ray
/// traced, through a stand-in as one set down does: `mesh::stand_in`).
#[allow(clippy::too_many_arguments)]
pub fn hand_light(
    mut commands: Commands,
    me: Res<Me>,
    settings: Res<crate::settings::Settings>,
    clock: Res<Clock>,
    eye: Res<Eye>,
    rt: Option<Res<crate::render::Raytraced>>,
    blocks_mats: Option<Res<crate::mesh::BlockMaterials>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut light: Query<(Entity, &HandLight, &mut Transform, &mut PointLight, &mut LanternLight)>,
) {
    let flame = items::block(me.held(settings.survival)).filter(|b| blocks::block(*b).light);
    for (e, h, ..) in &light {
        if Some(h.0) != flame {
            commands.entity(e).despawn();
        }
    }
    let Some(b) = flame else { return };
    let (k, _, color, flicker) = crate::render::lamp(b);
    // a little ahead of the eye and to the right, where the hand is
    let at = me.eye() + Quat::from_rotation_y(me.yaw) * Vec3::new(0.35, -0.3, -0.45);
    let lamp = LanternLight { base: crate::render::flame_lumens(&clock, &eye) * k, flicker, phase: 17.0 };
    let range = 14.0 * k.sqrt().max(0.5);
    if let Some((_, _, mut t, mut l, mut f)) = light.iter_mut().find(|(_, h, ..)| h.0 == b) {
        t.translation = at;
        l.range = range;
        *f = lamp;
        return;
    }
    let mut e = commands.spawn((HandLight(b), PointLight { intensity: lamp.base, range, color, shadow_maps_enabled: false, ..default() }, Transform::from_translation(at), lamp));
    if let (true, Some(m)) = (rt.is_some_and(|r| r.0), blocks_mats) {
        let mut data = crate::mesh::MeshData::default();
        crate::mesh::stand_in_round(&mut data, Vec3::ZERO, b);
        crate::render::trace_only(&mut e, meshes.add(data.into_mesh(true)), m.lamps.clone());
    }
}
