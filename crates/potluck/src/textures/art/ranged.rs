//! Icons for what's shot and thrown (the Bows, slings and spears creation
//! uses them): a bow, an arrow, a sling and a spear, drawn corner to corner
//! like the tools, with the same dark outline.

use super::super::*;
use super::{frame, head, outline, rod, Made, OUTLINE};

#[derive(Clone, Copy, PartialEq)]
enum Ranged {
    Bow,
    Arrow,
    Sling,
    Spear,
}

const FEATHER: &[u32] = &[0x5e1a16, 0x8a2a22, 0xb8463a, 0xe0d6c4];

/// How far a point is from the segment `a` to `b`.
fn to_line(x: f32, y: f32, a: (f32, f32), b: (f32, f32)) -> f32 {
    let (px, py, dx, dy) = (x - a.0, y - a.1, b.0 - a.0, b.1 - a.1);
    let k = ((px * dx + py * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
    ((px - dx * k).powi(2) + (py - dy * k).powi(2)).sqrt()
}

fn paint(name: &'static str, r: Ranged) -> Tex {
    let s = seed_of(name);
    let pals: [&'static [u32]; 4] = match r {
        Ranged::Bow => [PLANK, WOOL_CREAM, TWINE, OUTLINE],
        Ranged::Arrow => [PLANK, super::super::smelting::FLINT, FEATHER, OUTLINE],
        Ranged::Sling => [TWINE, WOOL_CREAM, COBBLE, OUTLINE],
        Ranged::Spear => [PLANK, super::super::smelting::IRON_BAR, TWINE, OUTLINE],
    };
    let mut a = Art::new(name, &pals);
    let px = |pal: usize, t: f32, h: f32, rough: f32| Px { pal, t: t.clamp(0.0, 1.0), h, rough, alpha: 1.0 };
    a.fill(|x, y, _| {
        let (u, v) = frame(x, y);
        let grain = (u * 0.8 + 2.0 * fbm(x, y, 4, 2, s ^ 3)).sin();
        let none = Px { pal: 0, t: 0.0, h: 0.0, rough: 1.0, alpha: 0.0 };
        match r {
            Ranged::Bow => {
                // a stave bent toward the top left, its string straight
                // across, and a wrapped grip in the middle
                let span = 27.0;
                if u.abs() <= span {
                    let k = u / span;
                    let bend = -11.0 * (1.0 - k * k);
                    let half = 2.2 - 1.2 * k * k;
                    if (v - bend).abs() <= half {
                        if u.abs() <= 4.0 {
                            return px(2, 0.5 + 0.25 * (u * 2.2).sin(), 0.8, 0.9);
                        }
                        return rod(v - bend, half, 0, grain);
                    }
                    if v.abs() <= 0.55 {
                        return px(1, 0.75, 0.3, 0.9);
                    }
                }
            }
            Ranged::Arrow => {
                // a shaft, a flint head at the top right and feathers at the
                // bottom left
                if (21.0..=31.0).contains(&u) && v.abs() <= (31.0 - u) * 0.5 {
                    let edge = v.abs() / ((31.0 - u) * 0.5).max(0.5);
                    let chip = vn(x, y, 8, 8, s);
                    return px(1, 0.35 + 0.35 * (1.0 - edge) + 0.2 * (chip - 0.5), 0.7, 0.4);
                }
                if (-29.0..=-15.0).contains(&u) && (1.0..=5.0).contains(&v.abs()) && v.abs() - 1.0 <= (u + 29.0) * 0.45 {
                    let barbs = ((u + v.abs()) * 1.6).sin() * 0.5 + 0.5;
                    return px(2, 0.45 + 0.3 * barbs, 0.5, 0.95);
                }
                if v.abs() <= 1.1 && (-30.0..=22.0).contains(&u) {
                    return rod(v, 1.1, 0, grain);
                }
            }
            Ranged::Sling => {
                // a leather pouch with a stone in it at the bottom left, and
                // two cords up to the top right
                let (pcx, pcy) = (21.0, 43.0);
                let (dx, dy) = (x - pcx, y - pcy);
                // the pouch lies along the diagonal
                let (pu, pv) = ((dx - dy) / std::f32::consts::SQRT_2, (dx + dy) / std::f32::consts::SQRT_2);
                let e = (pu / 10.0).powi(2) + (pv / 6.5).powi(2);
                if ((x - 20.0).powi(2) + (y - 41.0).powi(2)).sqrt() <= 4.5 {
                    let c = vn(x, y, 8, 8, s ^ 5);
                    return px(2, 0.45 + 0.3 * (c - 0.5) + 0.15 * (41.0 - y) / 4.5, 0.85, 0.7);
                }
                if e <= 1.0 {
                    let stitch = ((pu * 1.3).sin() * 0.5 + 0.5) * 0.15;
                    return px(0, 0.35 + 0.3 * (1.0 - e) + stitch, 0.5 + 0.3 * (1.0 - e), 0.85);
                }
                for (a, b) in [((28.0, 36.0), (50.0, 12.0)), ((14.0, 49.0), (8.0, 50.0)), ((27.0, 50.0), (53.0, 18.0))] {
                    if to_line(x, y, a, b) <= 0.9 {
                        return px(1, 0.6 + 0.15 * (u * 1.5).sin(), 0.4, 0.9);
                    }
                }
                // a loop for a finger at the end of one cord
                if ((x - 52.0).powi(2) + (y - 14.0).powi(2)).sqrt() - 3.0 <= 0.8 && ((x - 52.0).powi(2) + (y - 14.0).powi(2)).sqrt() >= 2.2 {
                    return px(1, 0.65, 0.4, 0.9);
                }
            }
            Ranged::Spear => {
                // a long shaft, a binding and a leaf shaped iron head
                if (17.0..=31.0).contains(&u) {
                    let k = (u - 17.0) / 14.0;
                    let half = 3.6 * (std::f32::consts::PI * k).sin().powf(0.7) * (1.0 - 0.3 * k);
                    if v.abs() <= half.max(0.8) {
                        let ridge = 1.0 - (v.abs() / half.max(0.8));
                        return head(Made::Iron, x, y, ridge, smooth(0.6, 1.0, 1.0 - ridge) * 0.6, s);
                    }
                }
                if (13.5..=17.0).contains(&u) && v.abs() <= 1.9 {
                    return px(2, 0.45 + 0.25 * (u * 2.5).sin(), 0.8, 0.9);
                }
                if v.abs() <= 1.4 && (-31.0..=14.0).contains(&u) {
                    return rod(v, 1.4, 0, grain);
                }
            }
        }
        none
    });
    outline(&mut a);
    a.finish(1.5, 0.5)
}

const RANGED: [(&str, Ranged); 4] = [("bow", Ranged::Bow), ("arrow", Ranged::Arrow), ("sling", Ranged::Sling), ("spear", Ranged::Spear)];

/// Each one's icon.
pub(super) fn tiles() -> Vec<(&'static str, Tex)> {
    RANGED.iter().map(|&(name, r)| (name, paint(name, r))).collect()
}
