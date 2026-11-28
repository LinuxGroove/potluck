//! Icons for armor (the Armor creation wears them): a cap or helmet, a
//! coat, leggings and boots in wool, copper, iron and moonsilver, and a
//! shield. Each is drawn upright, as it would hang on a peg, with the same
//! dark outline as the tools.

use super::super::*;
use super::{head, outline, Made, OUTLINE};

#[derive(Clone, Copy, PartialEq)]
enum Piece {
    Head,
    Body,
    Legs,
    Feet,
}

/// Knitted wool: rows of little stitches.
fn knit(x: f32, y: f32) -> f32 {
    let stitch = ((x * 1.1).sin() * 0.5 + 0.5) * ((y * 1.5 + 0.8 * (x * 1.1).cos()).sin() * 0.5 + 0.5);
    0.4 + 0.3 * stitch
}

/// One texel of the piece's stuff: knitted wool, or metal plate `depth` 0
/// at its edges to 1 in its middle, with `trim` along its edges.
fn stuff(made: Option<Made>, x: f32, y: f32, depth: f32, trim: f32, s: u32) -> Px {
    match made {
        None => Px { pal: 0, t: (knit(x, y) + 0.2 * depth - 0.25 * trim).clamp(0.0, 1.0), h: 0.4 + 0.3 * depth, rough: 0.95, alpha: 1.0 },
        Some(m) => {
            let mut p = head(m, x, y, depth, 0.35 * trim, s);
            p.pal = 0;
            p
        }
    }
}

fn paint(name: &'static str, piece: Piece, made: Option<Made>) -> Tex {
    let s = seed_of(name);
    let pal = match made {
        None => WOOL_CREAM,
        Some(Made::Copper) => super::super::smelting::COPPER_BAR,
        Some(Made::Iron) => super::super::smelting::IRON_BAR,
        Some(_) => super::super::smelting::MOONSILVER_BAR,
    };
    let mut a = Art::new(name, &[pal, pal, NAIL, OUTLINE]);
    a.fill(|x, y, _| {
        let none = Px { pal: 0, t: 0.0, h: 0.0, rough: 1.0, alpha: 0.0 };
        let (cx, dx) = (x + 0.5 - 32.0, (x + 0.5 - 32.0).abs());
        match piece {
            Piece::Head => {
                // a dome over a band (a cap's turned-up brim, a helmet's rim)
                let (rx, ry) = (19.0, 20.0);
                let d = (cx / rx).powi(2) + ((y - 38.0) / ry).powi(2);
                if y <= 38.0 && d <= 1.0 {
                    if made.is_none() && ((x - 32.0).powi(2) + (y - 15.0).powi(2)).sqrt() <= 5.0 {
                        // a cap's bobble
                        return stuff(made, x * 1.7, y * 1.7, 0.8, 0.0, s);
                    }
                    // a helmet's ridge down the middle
                    let ridge = if made.is_some() { (1.0 - dx / 2.0).max(0.0) } else { 0.0 };
                    return stuff(made, x, y, 1.0 - d + 0.4 * ridge, smooth(0.8, 1.0, d), s);
                }
                if (38.0..=45.0).contains(&y) && dx <= 21.0 {
                    let edge = ((y - 38.0) / 7.0 - 0.5).abs() * 2.0;
                    return stuff(made, x, y * 3.0, 0.6, edge, s);
                }
                // a helmet's nose guard
                if made.is_some() && (45.0..=52.0).contains(&y) && dx <= 2.0 {
                    return stuff(made, x, y, 0.5, 0.5, s);
                }
            }
            Piece::Body => {
                // a coat: shoulders, a body, two sleeves and a neck
                if ((x - 32.0) / 8.0).powi(2) + ((y - 12.0) / 5.0).powi(2) <= 1.0 {
                    return none;
                }
                let body = dx <= 15.0 && (12.0..=54.0).contains(&y);
                let shoulders = dx <= 22.0 && (12.0..=21.0).contains(&y);
                let sleeve = (17.0..=25.0).contains(&dx) && (14.0..=40.0).contains(&y) && dx - 17.0 <= (y - 10.0) * 0.6;
                if body || shoulders || sleeve {
                    let edge = if body { smooth(12.0, 15.0, dx).max(smooth(51.0, 54.0, y)) } else { 0.3 };
                    // plates across a metal coat, buttons down a woollen one
                    let seam = if made.is_some() { ((y - 12.0).rem_euclid(10.0) < 1.2) as i32 as f32 } else { 0.0 };
                    if made.is_none() && dx <= 1.2 && (y - 22.0).rem_euclid(9.0) < 2.0 && y > 18.0 {
                        return Px { pal: 2, t: 0.5, h: 0.8, rough: 0.6, alpha: 1.0 };
                    }
                    return stuff(made, x, y, 1.0 - dx / 25.0, (edge + seam).min(1.0), s);
                }
            }
            Piece::Legs => {
                // leggings: a waistband and two legs apart below the hips
                if dx <= 16.0 && (10.0..=17.0).contains(&y) {
                    return stuff(made, x, y * 2.5, 0.7, 0.6, s);
                }
                let legs = dx <= 16.0 && (17.0..=56.0).contains(&y) && (dx >= 2.5 || y < 26.0);
                if legs {
                    let mid = (dx - 9.25).abs() / 6.75;
                    return stuff(made, x, y, 1.0 - mid, smooth(51.0, 56.0, y), s);
                }
            }
            Piece::Feet => {
                // two boots side by side, toes to the left
                for bx in [20.0f32, 42.0] {
                    let shaft = (x - bx).abs() <= 6.0 && (20.0..=46.0).contains(&y);
                    let foot = (bx - 12.0..=bx + 6.0).contains(&x) && (44.0..=53.0).contains(&y);
                    if shaft || foot {
                        let cuff = (20.0..=25.0).contains(&y) && shaft;
                        let sole = y > 50.5;
                        if sole {
                            return Px { pal: 2, t: 0.25, h: 0.5, rough: 0.9, alpha: 1.0 };
                        }
                        return stuff(made, x, y, 1.0 - ((x - bx) / 6.0).abs().min(1.0), if cuff { 0.8 } else { 0.0 }, s);
                    }
                }
            }
        }
        none
    });
    outline(&mut a);
    a.finish(1.5, 0.5)
}

/// A shield: planks in an iron rim, to a point at the bottom, with a boss
/// in the middle.
fn shield(name: &'static str) -> Tex {
    let s = seed_of(name);
    let mut a = Art::new(name, &[PLANK, super::super::smelting::IRON_BAR, NAIL, OUTLINE]);
    a.fill(|x, y, _| {
        let none = Px { pal: 0, t: 0.0, h: 0.0, rough: 1.0, alpha: 0.0 };
        let dx = (x + 0.5 - 32.0).abs();
        let half = if y < 32.0 { 21.0 } else { 21.0 * (1.0 - ((y - 32.0) / 24.0).powi(2)).max(0.0).sqrt() };
        if !(8.0..=56.0).contains(&y) || dx > half {
            return none;
        }
        let rim = dx > half - 3.0 || y < 11.0;
        if rim {
            return head(Made::Iron, x, y, 0.6, 0.4, s);
        }
        if ((x - 32.0).powi(2) + (y - 28.0).powi(2)).sqrt() <= 6.0 {
            let d = ((x - 32.0).powi(2) + (y - 28.0).powi(2)).sqrt() / 6.0;
            return head(Made::Iron, x, y, 1.0 - d, 0.0, s);
        }
        // planks down it, with their grain
        let gap = ((x - 32.0 + 3.5).rem_euclid(7.0) < 0.9) as i32 as f32;
        let grain = (y * 0.7 + 2.0 * fbm(x, y, 4, 2, s ^ 7)).sin();
        Px { pal: 0, t: (0.5 + 0.1 * grain - 0.3 * gap).clamp(0.0, 1.0), h: 0.5 - 0.3 * gap, rough: 0.8, alpha: 1.0 }
    });
    outline(&mut a);
    a.finish(1.5, 0.5)
}

const PIECES: [Piece; 4] = [Piece::Head, Piece::Body, Piece::Legs, Piece::Feet];
/// Each material, and its pieces' icons in the order of `PIECES`.
const MATERIALS: &[(Option<Made>, [&str; 4])] = &[
    (None, ["armor_wool_head", "armor_wool_body", "armor_wool_legs", "armor_wool_feet"]),
    (Some(Made::Copper), ["armor_copper_head", "armor_copper_body", "armor_copper_legs", "armor_copper_feet"]),
    (Some(Made::Iron), ["armor_iron_head", "armor_iron_body", "armor_iron_legs", "armor_iron_feet"]),
    (Some(Made::Moonsilver), ["armor_moonsilver_head", "armor_moonsilver_body", "armor_moonsilver_legs", "armor_moonsilver_feet"]),
];

/// Every piece's icon, and the shield's.
pub(super) fn tiles() -> Vec<(&'static str, Tex)> {
    let mut out: Vec<(&'static str, Tex)> = MATERIALS.iter().flat_map(|&(made, names)| PIECES.iter().zip(names).map(move |(&piece, name)| (name, paint(name, piece, made)))).collect();
    out.push(("shield", shield("shield")));
    out
}
