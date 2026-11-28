//! What people wear, drawn: armor (a cap or helmet, a coat, leggings and
//! boots, in wool, leather, copper, iron or moonsilver) and a shield, on
//! other players' bodies and on yours seen from behind, as creations say
//! they wear them (`world.wear`, `Wears`). Each piece is a few boxes a
//! little bigger than the parts it covers, hung on those parts so it
//! swings with them, in the parts' own painted skins: a weave for wool,
//! smooth hide for leather, a soft sheen for metal (which shines a little) and
//! fine streaks for the shield's boards. A thing that isn't armor or a
//! shield isn't drawn.

use std::collections::HashMap;

use bevy::prelude::*;

use crate::creatures::{part_mesh, skin_textures, Part, PartMaterials, PartView, Person};
use crate::items::{self, Thing};
use crate::textures::skins::Skin;

/// What a person wears on their head, body, legs and feet and on their
/// arm (`world.wear`'s order), 0 for nothing.
#[derive(Component, Default, Clone, Copy, PartialEq, Debug)]
pub struct Wears(pub [Thing; 5]);

/// What's drawn on a person now, so it's only drawn again when that
/// changes.
#[derive(Component)]
pub struct WornDrawn(pub [Thing; 5]);

/// One box of a piece someone wears, and which piece.
#[derive(Component)]
pub struct Piece(pub Thing);

/// What a piece is made of.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Stuff {
    Wool,
    Leather,
    Copper,
    Iron,
    Moonsilver,
    /// The shield's boards (its rims and boss are iron).
    Wood,
}

impl Stuff {
    /// What `thing` is made of, from its picture's name (`armor_iron_head`,
    /// `shield`); None for anything that isn't armor.
    pub fn of(thing: Thing) -> Option<Stuff> {
        let icon = items::item(thing)?.icon;
        if icon == "shield" {
            return Some(Stuff::Wood);
        }
        Some(match icon.strip_prefix("armor_")?.split('_').next()? {
            "wool" => Stuff::Wool,
            "leather" => Stuff::Leather,
            "copper" => Stuff::Copper,
            "iron" => Stuff::Iron,
            "moonsilver" => Stuff::Moonsilver,
            _ => return None,
        })
    }

    fn metal(self) -> bool {
        matches!(self, Stuff::Copper | Stuff::Iron | Stuff::Moonsilver)
    }

    /// Its colour and its trim's (bands, rims, studs), in sRGB, from the
    /// icons' palettes (`textures/art/armor.rs`).
    fn colours(self) -> ([f32; 3], [f32; 3]) {
        match self {
            Stuff::Wool => ([0.9, 0.86, 0.78], [0.66, 0.6, 0.52]),
            Stuff::Leather => ([0.57, 0.35, 0.2], [0.34, 0.19, 0.1]),
            Stuff::Copper => ([0.82, 0.44, 0.21], [0.53, 0.24, 0.11]),
            Stuff::Iron => ([0.6, 0.62, 0.66], [0.36, 0.38, 0.42]),
            Stuff::Moonsilver => ([0.64, 0.74, 0.88], [0.4, 0.48, 0.64]),
            Stuff::Wood => ([0.56, 0.38, 0.22], [0.44, 0.46, 0.5]),
        }
    }

    /// The skin it wears (its trim's, for the shield's iron).
    fn skin(self, trim: bool) -> Skin {
        match self {
            Stuff::Wool => Skin::Cloth,
            Stuff::Leather => Skin::Bare,
            Stuff::Wood if !trim => Skin::Horn,
            // plate: a soft sheen with a few faint marks
            _ => Skin::Smooth,
        }
    }
}

/// The parts armor hangs on.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Spot {
    Head,
    Chest,
    Sleeve,
    Leg,
    /// The left hand, which holds the shield (the right holds what's in hand).
    LeftHand,
}

fn spot(p: &Part) -> Option<Spot> {
    match p.name.as_str() {
        // the head, not the mouth
        "face" if p.size.x > 0.3 => Some(Spot::Head),
        "shirt cloth" => Some(Spot::Chest),
        "sleeve cloth" => Some(Spot::Sleeve),
        "leg cloth" => Some(Spot::Leg),
        "hand" if p.at.x < 0.0 => Some(Spot::LeftHand),
        _ => None,
    }
}

/// A piece's boxes on one part (`creatures::person_parts`): each one's
/// size, its middle from the part's, and whether it's trim. Each reaches
/// a little past the top of what it covers, so no shirt shows through. `slot` is
/// `Wears`' (0 head to 4 arm) and `side` the part's (+1 right, -1 left).
fn boxes(slot: usize, stuff: Stuff, spot: Spot, side: f32) -> Vec<(Vec3, Vec3, bool)> {
    let b = |s: [f32; 3], a: [f32; 3], trim: bool| (Vec3::from_array(s), Vec3::from_array(a), trim);
    let metal = stuff.metal();
    match (slot, spot) {
        // a helmet: a dome, a guard down the back, the cheeks and the nose,
        // and a band round it
        (0, Spot::Head) if metal => vec![
            b([0.52, 0.2, 0.5], [0.0, 0.23, 0.01], false),
            b([0.52, 0.34, 0.07], [0.0, 0.06, 0.245], false),
            b([0.07, 0.3, 0.3], [0.255, 0.05, 0.06], false),
            b([0.07, 0.3, 0.3], [-0.255, 0.05, 0.06], false),
            b([0.05, 0.2, 0.04], [0.0, 0.04, -0.235], true),
            b([0.54, 0.05, 0.52], [0.0, 0.14, 0.01], true),
        ],
        // a cap over the hair, down the back, with a turned up band
        (0, Spot::Head) => vec![
            b([0.5, 0.17, 0.49], [0.0, 0.235, 0.01], false),
            b([0.5, 0.26, 0.07], [0.0, 0.1, 0.235], false),
            b([0.52, 0.06, 0.51], [0.0, 0.16, 0.01], true),
        ],
        // a breastplate with a ridge, over a skirt of plates
        (1, Spot::Chest) if metal => vec![
            b([0.6, 0.52, 0.37], [0.0, 0.09, 0.0], false),
            b([0.08, 0.44, 0.02], [0.0, 0.08, -0.195], true),
            b([0.6, 0.16, 0.37], [0.0, -0.26, 0.0], true),
        ],
        // a coat to the belt, with a collar and buttoned down the front
        (1, Spot::Chest) => vec![
            b([0.6, 0.7, 0.36], [0.0, 0.0, 0.0], false),
            b([0.38, 0.06, 0.38], [0.0, 0.32, 0.0], true),
            b([0.06, 0.6, 0.02], [0.0, -0.02, -0.19], true),
        ],
        // plate on the upper arm, under a shoulder guard
        (1, Spot::Sleeve) if metal => vec![b([0.22, 0.26, 0.24], [0.0, 0.06, 0.0], false), b([0.26, 0.14, 0.28], [0.015 * side, 0.21, 0.0], true)],
        // a sleeve with a cuff
        (1, Spot::Sleeve) => vec![b([0.22, 0.46, 0.24], [0.0, 0.03, 0.0], false), b([0.23, 0.05, 0.25], [0.0, -0.2, 0.0], true)],
        // leggings from the hip to the boots; metal ones guard the knee
        (2, Spot::Leg) => {
            let mut v = vec![b([0.27, 0.54, 0.31], [0.0, 0.13, 0.0], false)];
            if metal {
                v.push(b([0.2, 0.1, 0.04], [0.0, -0.04, -0.17], true));
            }
            v
        }
        // boots with toes, and a band round the top
        (3, Spot::Leg) => vec![
            b([0.27, 0.26, 0.31], [0.0, -0.27, 0.0], false),
            b([0.27, 0.1, 0.07], [0.0, -0.35, -0.185], false),
            b([0.28, 0.05, 0.32], [0.0, -0.15, 0.0], true),
        ],
        // a shield on the outside of the left hand: boards, an iron rim
        // and a boss
        (4, Spot::LeftHand) => vec![
            b([0.05, 0.52, 0.42], [-0.11, 0.08, 0.0], false),
            b([0.06, 0.04, 0.43], [-0.11, 0.34, 0.0], true),
            b([0.06, 0.04, 0.43], [-0.11, -0.18, 0.0], true),
            b([0.06, 0.52, 0.04], [-0.11, 0.08, 0.21], true),
            b([0.06, 0.52, 0.04], [-0.11, 0.08, -0.21], true),
            b([0.04, 0.12, 0.12], [-0.15, 0.08, 0.0], true),
        ],
        _ => vec![],
    }
}

#[derive(Resource, Default)]
pub struct WornLooks {
    mats: HashMap<(Stuff, bool), Handle<StandardMaterial>>,
    meshes: HashMap<([u16; 3], Skin), Handle<Mesh>>,
}

/// What drawing armor needs.
#[derive(bevy::ecs::system::SystemParam)]
pub struct Dressing<'w> {
    looks: ResMut<'w, WornLooks>,
    parts: ResMut<'w, PartMaterials>,
    images: ResMut<'w, Assets<Image>>,
    meshes: ResMut<'w, Assets<Mesh>>,
    mats: ResMut<'w, Assets<StandardMaterial>>,
    rt: Option<Res<'w, crate::render::Raytraced>>,
}

impl Dressing<'_> {
    /// A box of `size` in `stuff` (or its trim): made once each.
    fn look(&mut self, size: Vec3, stuff: Stuff, trim: bool) -> (Handle<Mesh>, Handle<StandardMaterial>) {
        let skin = stuff.skin(trim);
        // ray traced meshes carry no colours (`mesh::pack`)
        let colours = !self.rt.as_ref().is_some_and(|r| r.0);
        let key = (size.to_array().map(|v| (v * 256.0).round().clamp(1.0, 65535.0) as u16), skin);
        let meshes = &mut *self.meshes;
        let mesh = self.looks.meshes.entry(key).or_insert_with(|| meshes.add(part_mesh(size, skin, colours))).clone();
        if let Some(m) = self.looks.mats.get(&(stuff, trim)) {
            return (mesh, m.clone());
        }
        let [color, normal, rough] = skin_textures(&mut self.parts, &mut self.images, skin);
        let (main, band) = stuff.colours();
        let c = if trim { band } else { main };
        // metal shines a little; cloth and hide are as rough as their skins say
        let shines = stuff.metal() || (stuff == Stuff::Wood && trim);
        let m = self.mats.add(StandardMaterial {
            base_color: Color::srgb(c[0], c[1], c[2]),
            base_color_texture: Some(color),
            normal_map_texture: Some(normal),
            metallic_roughness_texture: (!shines).then_some(rough),
            metallic: if shines { 0.6 } else { 0.0 },
            perceptual_roughness: if shines { 0.4 } else if stuff == Stuff::Leather { 0.7 } else { 1.0 },
            reflectance: if shines { 0.5 } else { 0.35 },
            ..default()
        });
        self.looks.mats.insert((stuff, trim), m.clone());
        (mesh, m)
    }
}

/// Dresses each person in what they wear: takes off what's drawn and
/// hangs the new pieces on their parts whenever `Wears` changes.
pub fn wear(
    mut commands: Commands,
    mut dressing: Dressing,
    people: Query<(Entity, &Person, &Wears, Option<&WornDrawn>, Option<&Children>)>,
    parts: Query<(&PartView, Option<&Children>)>,
    pieces: Query<(), With<Piece>>,
) {
    for (e, person, wears, drawn, children) in &people {
        if drawn.is_some_and(|d| d.0 == wears.0) {
            continue;
        }
        let Some(children) = children else { continue };
        let spots: Vec<(Entity, Spot, f32)> = children
            .iter()
            .filter_map(|c| {
                let p = person.part(parts.get(c).ok()?.0)?;
                Some((c, spot(p)?, p.at.x.signum()))
            })
            .collect();
        // not dressed yet (`creatures::people`)
        if spots.is_empty() {
            continue;
        }
        for (part, ..) in &spots {
            if let Ok((_, Some(inside))) = parts.get(*part) {
                for c in inside.iter().filter(|c| pieces.contains(*c)) {
                    commands.entity(c).despawn();
                }
            }
        }
        for (slot, &thing) in wears.0.iter().enumerate() {
            let Some(stuff) = Stuff::of(thing) else { continue };
            for &(part, spot, side) in &spots {
                for (size, at, trim) in boxes(slot, stuff, spot, side) {
                    let (mesh, mat) = dressing.look(size, stuff, trim);
                    commands.spawn((Mesh3d(mesh), MeshMaterial3d(mat), Transform::from_translation(at), Visibility::Inherited, Piece(thing), ChildOf(part)));
                }
            }
        }
        commands.entity(e).insert(WornDrawn(wears.0));
    }
}
