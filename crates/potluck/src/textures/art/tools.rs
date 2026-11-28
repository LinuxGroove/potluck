//! Icons for the things players hold (`blocks::Block::item`): tools,
//! swords and hoes, in wood, stone, copper, iron and moonsilver. They're
//! never placed, so each tile is only a picture, drawn corner to corner
//! with the handle at the bottom left, in the blocks' palettes, with a dark
//! outline so it reads in the hotbar.

use super::*;

#[path = "armor.rs"]
mod armor;
#[path = "ranged.rs"]
mod ranged;

/// What a tool's head is made of.
#[derive(Clone, Copy, PartialEq)]
enum Made {
    Wood,
    Stone,
    Copper,
    Iron,
    Moonsilver,
}

#[derive(Clone, Copy, PartialEq)]
enum Tool {
    Pickaxe,
    Axe,
    Shovel,
    Sword,
    Hoe,
}

// Palettes, by index: 0 the handle, 1 the head, 2 the binding, 3 the outline.
const OUTLINE: &[u32] = &[0x141110, 0x1d1915];
const GRIP: &[u32] = &[0x2a1a10, 0x3a2416, 0x4f331f, 0x664329];

/// Along the handle (`u`, up and to the right) and across it (`v`), from
/// the tile's middle, in pixels.
fn frame(x: f32, y: f32) -> (f32, f32) {
    let (dx, dy) = (x + 0.5 - 32.0, y + 0.5 - 32.0);
    ((dx - dy) / std::f32::consts::SQRT_2, (dx + dy) / std::f32::consts::SQRT_2)
}

/// A rounded rod across `v` (lighter on its upper left side), for handles.
fn rod(v: f32, half: f32, pal: usize, grain: f32) -> Px {
    let round = (v / half).clamp(-1.0, 1.0);
    let t = 0.55 - 0.3 * round + 0.08 * grain;
    Px { pal, t, h: 0.5 + 0.35 * (1.0 - round * round), rough: 0.75, alpha: 1.0 }
}

/// The head's texel: wood with a grain, or chipped stone, `edge` 0 inside
/// to 1 at the working edge (which is lighter, worn bright).
fn head(made: Made, x: f32, y: f32, depth: f32, edge: f32, s: u32) -> Px {
    let (t, rough) = match made {
        Made::Wood => {
            let grain = ((x + y) * 0.9 + 3.0 * fbm(x, y, 4, 2, s)).sin();
            (0.5 + 0.1 * grain, 0.7)
        }
        Made::Stone => {
            let c = vn(x, y, 8, 8, s);
            (0.45 + 0.35 * (c - 0.5), 0.6)
        }
        // worked metal: smooth, with a sheen across it and faint hammer marks
        Made::Copper | Made::Iron | Made::Moonsilver => {
            let dents = vn(x, y, 16, 16, s);
            let sheen = ((x - y) * 0.12).sin();
            // iron polished brighter than stone, so the two read apart
            let lift = if made == Made::Iron { 0.14 } else { 0.0 };
            (0.42 + lift + 0.08 * (dents - 0.5) + 0.1 * sheen, 0.3)
        }
    };
    Px { pal: 1, t: (t + 0.3 * depth + 0.25 * edge).clamp(0.0, 1.0), h: 0.4 + 0.4 * depth, rough, alpha: 1.0 }
}

fn paint(name: &'static str, tool: Tool, made: Made) -> Tex {
    let s = seed_of(name);
    let head_pal = match made {
        Made::Wood => PLANK,
        Made::Stone => STONE,
        Made::Copper => super::smelting::COPPER_BAR,
        Made::Iron => super::smelting::IRON_BAR,
        Made::Moonsilver => super::smelting::MOONSILVER_BAR,
    };
    let handle = if tool == Tool::Sword { GRIP } else { PLANK };
    let mut a = Art::new(name, &[handle, head_pal, TWINE, OUTLINE]);
    a.fill(|x, y, _| {
        let (u, v) = frame(x, y);
        let grain = (u * 0.8 + 2.0 * fbm(x, y, 4, 2, s ^ 3)).sin();
        let none = Px { pal: 0, t: 0.0, h: 0.0, rough: 1.0, alpha: 0.0 };
        match tool {
            Tool::Pickaxe => {
                // a curved head across the handle's top, its points bent back
                let mid = 17.0 - 0.022 * v * v;
                let half = 1.2 + 3.0 * (1.0 - (v.abs() / 22.0).min(1.0));
                if v.abs() <= 22.0 && (u - mid).abs() <= half {
                    let tip = smooth(15.0, 22.0, v.abs());
                    return head(made, x, y, 1.0 - ((u - mid) / half).abs(), tip, s);
                }
                if (u - 13.0).abs() <= 1.5 && v.abs() <= 3.2 {
                    return Px { pal: 2, t: 0.5 + 0.3 * ((u * 3.0).sin()), h: 0.7, rough: 0.9, alpha: 1.0 };
                }
                if v.abs() <= 2.2 && (-28.0..=20.0).contains(&u) {
                    return rod(v, 2.2, 0, grain);
                }
            }
            Tool::Axe => {
                // a blade on one side, widening to its edge
                let reach = v - 2.0;
                let half = 3.5 + 0.4 * reach.max(0.0);
                if (0.0..=13.0).contains(&reach) && (u - 14.0).abs() <= half {
                    let edge = smooth(9.0, 13.0, reach);
                    return head(made, x, y, 1.0 - reach / 13.0 * 0.6, edge, s);
                }
                // its back, a short block the other side
                if (-6.0..=-2.0).contains(&v) && (u - 14.0).abs() <= 2.5 {
                    return head(made, x, y, 0.6, 0.0, s);
                }
                if (u - 9.0).abs() <= 1.5 && v.abs() <= 3.2 {
                    return Px { pal: 2, t: 0.5 + 0.3 * ((u * 3.0).sin()), h: 0.7, rough: 0.9, alpha: 1.0 };
                }
                if v.abs() <= 2.2 && (-28.0..=19.0).contains(&u) {
                    return rod(v, 2.2, 0, grain);
                }
            }
            Tool::Shovel => {
                // a rounded spade on the end of a long handle
                // squared off where it meets the handle, rounded at the end
                let (du, dv) = ((u - 20.0) / 9.0, v / 6.0);
                let d = du.abs().powf(if u > 20.0 { 2.0 } else { 5.0 }) + dv.abs().powf(3.0);
                if d <= 1.0 && u >= 12.0 {
                    let edge = smooth(0.55, 1.0, d) * (u > 20.0) as i32 as f32;
                    return head(made, x, y, 1.0 - d, edge, s);
                }
                if (u - 11.0).abs() <= 1.5 && v.abs() <= 3.0 {
                    return Px { pal: 2, t: 0.5 + 0.3 * ((u * 3.0).sin()), h: 0.7, rough: 0.9, alpha: 1.0 };
                }
                if v.abs() <= 2.0 && (-28.0..=12.0).contains(&u) {
                    return rod(v, 2.0, 0, grain);
                }
                // a grip across the bottom end
                if (u + 26.5).abs() <= 1.8 && v.abs() <= 5.0 {
                    return rod(u + 26.5, 1.8, 0, grain);
                }
            }
            Tool::Hoe => {
                // a narrow blade off one side of the handle's top, its end
                // turned down a little
                if (u - 18.0).abs() <= 3.0 && (-13.0..=2.5).contains(&v) {
                    let edge = smooth(-9.0, -13.0, v);
                    return head(made, x, y, 1.0 - ((u - 18.0) / 3.0).abs(), edge, s);
                }
                if (u - 14.5).abs() <= 1.5 && (-13.0..=-10.0).contains(&v) {
                    return head(made, x, y, 0.5, 1.0, s);
                }
                if (u - 14.5).abs() <= 1.3 && v.abs() <= 3.0 {
                    return Px { pal: 2, t: 0.5 + 0.3 * ((u * 3.0).sin()), h: 0.7, rough: 0.9, alpha: 1.0 };
                }
                if v.abs() <= 2.0 && (-28.0..=20.0).contains(&u) {
                    return rod(v, 2.0, 0, grain);
                }
            }
            Tool::Sword => {
                // a blade to a point, a guard, a wrapped grip and a pommel
                let taper = 1.0 - smooth(20.0, 28.0, u);
                let half = 0.6 + 2.8 * taper;
                if (-7.0..=28.0).contains(&u) && v.abs() <= half {
                    let ridge = 1.0 - (v / half).abs();
                    let edge = 1.0 - ridge;
                    return head(made, x, y, ridge, 0.6 * edge, s);
                }
                if (u + 8.5).abs() <= 1.6 && v.abs() <= 8.0 {
                    return Px { pal: 1, t: 0.4 + 0.2 * (1.0 - v.abs() / 8.0), h: 0.7, rough: 0.6, alpha: 1.0 };
                }
                if (-23.0..=-10.0).contains(&u) && v.abs() <= 1.8 {
                    let wrap = ((u * 1.4).sin() > 0.3) as i32 as f32;
                    let mut p = rod(v, 1.8, 0, 0.0);
                    p.t -= 0.2 * wrap;
                    return p;
                }
                if (u + 25.0).abs() <= 2.2 && v.abs() <= 2.6 {
                    return head(made, x, y, 0.8, 0.0, s);
                }
            }
        }
        none
    });
    outline(&mut a);
    a.finish(1.5, 0.5)
}

/// A dark line round what's painted (palette 3), so it reads on any
/// background.
fn outline(a: &mut Art) {
    let shown: Vec<bool> = a.px.iter().map(|p| p.alpha > 0.0).collect();
    for y in 0..N {
        for x in 0..N {
            let i = (y * N + x) as usize;
            if shown[i] {
                continue;
            }
            let near = [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().any(|(dx, dy)| {
                let (nx, ny) = (x + dx, y + dy);
                (0..N).contains(&nx) && (0..N).contains(&ny) && shown[(ny * N + nx) as usize]
            });
            if near {
                a.px[i] = Px { pal: 3, t: 0.0, h: 0.3, rough: 1.0, alpha: 1.0 };
            }
        }
    }
}

/// The tools for the land and water, and fish: each its own shape.
#[derive(Clone, Copy, PartialEq)]
enum Gear {
    Shears,
    Sickle,
    Hammer,
    Rod,
    Bucket(Pail),
    Can,
    Fish { cooked: bool },
}

/// What's in a bucket.
#[derive(Clone, Copy, PartialEq)]
enum Pail {
    Empty,
    Water,
    Milk,
}

const MILK: &[u32] = &[0xb4b8bc, 0xd0d4d8, 0xe4e7ea, 0xf4f6f8];
const WATER: &[u32] = &[0x0f2f5c, 0x163f78, 0x1f5496, 0x2f6db3, 0x4b8bcc, 0x76aee0];
const FISH: &[u32] = &[0x2d3a4a, 0x45566a, 0x617489, 0x8297aa, 0xa8bccb, 0xd2dfe8];
const COOKED: &[u32] = &[0x3a1c0a, 0x5e3112, 0x84491c, 0xa86529, 0xc6833b, 0xdfa557];
const BOBBER: &[u32] = &[0x6a1012, 0x9a1c1d, 0xc93a33, 0xe9e2d2];

/// A ring round (`cx`, `cy`) of radius `r`, `half` thick: how far into it a
/// point is, 0 at its edges to 1 along its middle, or None outside it.
fn ring(x: f32, y: f32, cx: f32, cy: f32, r: f32, half: f32) -> Option<f32> {
    let d = ((x - cx).powi(2) + (y - cy).powi(2)).sqrt();
    ((d - r).abs() <= half).then(|| 1.0 - (d - r).abs() / half)
}

fn gear(name: &'static str, g: Gear) -> Tex {
    let s = seed_of(name);
    // palettes: 0 the handle or body, 1 the metal or what it's full of, 2 a
    // third colour, 3 the outline
    let pals: [&'static [u32]; 4] = match g {
        Gear::Shears => [super::WOOL_RED, super::smelting::IRON_BAR, NAIL, OUTLINE],
        Gear::Sickle => [PLANK, super::smelting::COPPER_BAR, TWINE, OUTLINE],
        Gear::Hammer => [PLANK, super::smelting::IRON_BAR, TWINE, OUTLINE],
        Gear::Rod => [PLANK, super::WOOL_CREAM, BOBBER, OUTLINE],
        Gear::Bucket(Pail::Milk) => [super::smelting::IRON_BAR, MILK, NAIL, OUTLINE],
        Gear::Bucket(_) => [super::smelting::IRON_BAR, WATER, NAIL, OUTLINE],
        Gear::Can => [super::smelting::COPPER_BAR, WATER, NAIL, OUTLINE],
        Gear::Fish { cooked: false } => [FISH, FISH, NAIL, OUTLINE],
        Gear::Fish { cooked: true } => [COOKED, COOKED, NAIL, OUTLINE],
    };
    let mut a = Art::new(name, &pals);
    let metal = |x: f32, y: f32, depth: f32, edge: f32| head(Made::Iron, x, y, depth, edge, s);
    let px = |pal: usize, t: f32, h: f32, rough: f32| Px { pal, t: t.clamp(0.0, 1.0), h, rough, alpha: 1.0 };
    a.fill(|x, y, _| {
        let (u, v) = frame(x, y);
        let grain = (u * 0.8 + 2.0 * fbm(x, y, 4, 2, s ^ 3)).sin();
        let none = Px { pal: 0, t: 0.0, h: 0.0, rough: 1.0, alpha: 0.0 };
        match g {
            Gear::Shears => {
                // two blades crossing at a rivet, and a loop for each finger
                if ring(u, v, 2.0, 0.0, 0.0, 1.8).is_some() {
                    return px(2, 0.7, 0.9, 0.4);
                }
                for side in [-1.0f32, 1.0] {
                    let along = v - side * 0.13 * (u - 2.0);
                    let half = 0.5 + 2.4 * (1.0 - smooth(16.0, 27.0, u));
                    if (2.0..=27.0).contains(&u) && along.abs() <= half {
                        let ridge = 1.0 - (along / half).abs();
                        return metal(x, y, ridge, 0.5 * (1.0 - ridge));
                    }
                    if let Some(k) = ring(u, v, -15.0, side * 6.5, 4.6, 1.7) {
                        return px(0, 0.35 + 0.4 * k, 0.5 + 0.3 * k, 0.6);
                    }
                    let shaft = v - side * 6.5 * (2.0 - u) / 13.0;
                    if (-11.0..=2.0).contains(&u) && shaft.abs() <= 1.4 {
                        return metal(x, y, 0.6, 0.0);
                    }
                }
            }
            Gear::Sickle => {
                // a hooked blade curving round from the top of a short handle
                let (cu, cv, r) = (-3.0, -11.0, 11.0);
                let d = ((u - cu).powi(2) + (v - cv).powi(2)).sqrt();
                let ang = (v - cv).atan2(u - cu).to_degrees();
                // from the handle's top (90) round through the front (0) to the hook
                let along = if ang > 90.0 { -1.0 } else if ang >= -150.0 { (90.0 - ang) / 240.0 } else { -1.0 };
                if along >= 0.0 {
                    let half = 0.6 + 2.6 * (1.0 - along);
                    if (d - r).abs() <= half {
                        let inner = ((r - d) / half).clamp(-1.0, 1.0);
                        return metal(x, y, 1.0 - inner.abs(), smooth(0.0, 1.0, inner));
                    }
                }
                if (u + 4.5).abs() <= 1.4 && v.abs() <= 2.6 {
                    return px(2, 0.5 + 0.3 * (u * 3.0).sin(), 0.7, 0.9);
                }
                if v.abs() <= 2.2 && (-26.0..=-3.0).contains(&u) {
                    return rod(v, 2.2, 0, grain);
                }
            }
            Gear::Hammer => {
                // a heavy block of a head across the top of a long handle
                if (u - 17.0).abs() <= 5.0 && v.abs() <= 9.5 {
                    let face = smooth(7.0, 9.5, v.abs());
                    return metal(x, y, 1.0 - ((u - 17.0) / 5.0).abs() * 0.5, face);
                }
                if (u - 10.5).abs() <= 1.5 && v.abs() <= 3.0 {
                    return px(2, 0.5 + 0.3 * (u * 3.0).sin(), 0.7, 0.9);
                }
                if v.abs() <= 2.2 && (-28.0..=12.0).contains(&u) {
                    return rod(v, 2.2, 0, grain);
                }
            }
            Gear::Rod => {
                // a long rod, a reel near the grip, and a line down from the tip to a bobber
                let (tx, ty) = (55.0, 9.0);
                if ((x - tx).powi(2) + (y - 46.0).powi(2)).sqrt() <= 3.2 {
                    return px(2, if y < 46.0 { 0.6 } else { 0.98 }, 0.7, 0.5);
                }
                if (x - tx).abs() <= 0.6 && (ty..=43.0).contains(&y) {
                    return px(1, 0.7, 0.4, 0.8);
                }
                if let Some(k) = ring(u, v, -17.0, 3.8, 1.7, 1.3) {
                    return metal(x, y, k, 0.0);
                }
                let half = 1.9 - 1.2 * smooth(-28.0, 33.0, u);
                if v.abs() <= half && (-30.0..=33.0).contains(&u) {
                    let mut p = rod(v, half, 0, grain);
                    // a dark grip at the bottom
                    if u < -21.0 {
                        p.t -= 0.25;
                    }
                    return p;
                }
            }
            Gear::Bucket(pail) => {
                // a pail seen a little from above: sides narrowing to the
                // bottom, a rim, its inside (or water) and a handle over it
                let (top, bottom) = (20.0, 53.0);
                let k = ((y - top) / (bottom - top)).clamp(0.0, 1.0);
                let half = 17.0 - 4.0 * k;
                let (dx, dy) = ((x - 32.0) / 17.0, (y - top) / 4.5);
                if dx * dx + dy * dy <= 1.0 {
                    let rim = dx * dx + dy * dy > 0.62;
                    if rim {
                        return metal(x, y, 0.9, 0.6);
                    }
                    return match pail {
                        Pail::Water => px(1, 0.55 + 0.2 * (x * 0.6 + y).sin() * 0.5, 0.3, 0.1),
                        Pail::Milk => px(1, 0.7 - 0.25 * dx.abs(), 0.3, 0.3),
                        Pail::Empty => px(2, 0.1, 0.2, 0.8),
                    };
                }
                if (top..=bottom).contains(&y) && (x - 32.0).abs() <= half {
                    let round = ((x - 32.0) / half).abs();
                    let band = ((y - 31.0).abs() <= 1.2 || (y - 45.0).abs() <= 1.2) as i32 as f32;
                    return metal(x, y, 1.0 - round * round, 0.25 * band);
                }
                if y < top {
                    if let Some(k) = ring(x, y, 32.0, top, 16.0, 1.2) {
                        return px(2, 0.5 + 0.4 * k, 0.6, 0.5);
                    }
                }
            }
            Gear::Can => {
                // a round-shouldered can, a spout up to a rose, a handle on top
                if (x - 34.0).powi(2) / 196.0 + (y - 40.0).powi(2) / 169.0 <= 1.0 && y >= 28.0 {
                    let round = ((x - 34.0) / 14.0).abs();
                    return head(Made::Copper, x, y, 1.0 - round * round, 0.0, s);
                }
                // the spout, low on the left, up to its rose
                let (sx, sy, ex, ey) = (22.0, 46.0, 9.0, 24.0);
                let (lx, ly) = (ex - sx, ey - sy);
                let t = (((x - sx) * lx + (y - sy) * ly) / (lx * lx + ly * ly)).clamp(0.0, 1.0);
                let dist = ((x - sx - t * lx).powi(2) + (y - sy - t * ly).powi(2)).sqrt();
                if dist <= 1.8 {
                    return head(Made::Copper, x, y, 0.7, 0.0, s);
                }
                if ((x - ex).powi(2) + (y - ey).powi(2)).sqrt() <= 4.0 {
                    return head(Made::Copper, x, y, 0.9, 0.4, s);
                }
                if y < 30.0 {
                    if let Some(k) = ring(x, y, 36.0, 28.0, 10.0, 1.6) {
                        return head(Made::Copper, x, y, k, 0.2, s);
                    }
                }
            }
            Gear::Fish { cooked } => {
                // a fish side on, head to the right: back darker, belly
                // lighter, a forked tail, a fin and an eye (or grill marks)
                let (cx, cy) = (35.0, 33.0);
                let (dx, dy) = ((x - cx) / 19.0, (y - cy) / 9.5);
                let body = dx * dx + dy * dy;
                if body <= 1.0 {
                    if !cooked && ((x - 47.0).powi(2) + (y - 30.5).powi(2)).sqrt() <= 1.6 {
                        return px(2, 0.0, 0.8, 0.3);
                    }
                    let scales = vn(x, y, 16, 16, s);
                    let mut t = 0.3 + 0.5 * (dy + 1.0) * 0.5 + 0.08 * (scales - 0.5);
                    if cooked && ((x + y) * 0.5).rem_euclid(7.0) < 1.3 && dx < 0.6 {
                        t -= 0.4;
                    }
                    return px(0, t, 0.4 + 0.4 * (1.0 - body), if cooked { 0.8 } else { 0.35 });
                }
                // the tail, a fork behind
                let tx = 18.0 - x;
                if (0.0..=11.0).contains(&tx) && (y - cy).abs() <= 1.5 + 0.9 * tx && !((y - cy).abs() < 0.8 * tx - 4.0 && tx > 7.0) {
                    return px(1, 0.4 + 0.02 * tx, 0.4, 0.5);
                }
                // a fin on its back
                let fx = x - 32.0;
                if (-6.0..=6.0).contains(&fx) && y < cy - 8.0 && y > cy - 13.0 + fx.abs() * 0.5 {
                    return px(1, 0.35, 0.4, 0.5);
                }
            }
        }
        none
    });
    outline(&mut a);
    a.finish(1.5, 0.5)
}

/// Every icon: what it's called, the tool and what it's made of. There
/// are no wooden or stone hoes (a hand tills one block as well).
const ICONS: &[(&str, Tool, Made)] = &[
    ("pickaxe_wood", Tool::Pickaxe, Made::Wood),
    ("axe_wood", Tool::Axe, Made::Wood),
    ("shovel_wood", Tool::Shovel, Made::Wood),
    ("sword_wood", Tool::Sword, Made::Wood),
    ("pickaxe_stone", Tool::Pickaxe, Made::Stone),
    ("axe_stone", Tool::Axe, Made::Stone),
    ("shovel_stone", Tool::Shovel, Made::Stone),
    ("sword_stone", Tool::Sword, Made::Stone),
    ("pickaxe_copper", Tool::Pickaxe, Made::Copper),
    ("axe_copper", Tool::Axe, Made::Copper),
    ("shovel_copper", Tool::Shovel, Made::Copper),
    ("sword_copper", Tool::Sword, Made::Copper),
    ("hoe_copper", Tool::Hoe, Made::Copper),
    ("pickaxe_iron", Tool::Pickaxe, Made::Iron),
    ("axe_iron", Tool::Axe, Made::Iron),
    ("shovel_iron", Tool::Shovel, Made::Iron),
    ("sword_iron", Tool::Sword, Made::Iron),
    ("hoe_iron", Tool::Hoe, Made::Iron),
    ("pickaxe_moonsilver", Tool::Pickaxe, Made::Moonsilver),
    ("axe_moonsilver", Tool::Axe, Made::Moonsilver),
    ("shovel_moonsilver", Tool::Shovel, Made::Moonsilver),
    ("sword_moonsilver", Tool::Sword, Made::Moonsilver),
    ("hoe_moonsilver", Tool::Hoe, Made::Moonsilver),
];

/// The rest of the things players hold, each its own shape.
const GEAR: &[(&str, Gear)] = &[
    ("shears", Gear::Shears),
    ("bucket", Gear::Bucket(Pail::Empty)),
    ("bucket_water", Gear::Bucket(Pail::Water)),
    ("bucket_milk", Gear::Bucket(Pail::Milk)),
    ("fishing_rod", Gear::Rod),
    ("watering_can", Gear::Can),
    ("sickle", Gear::Sickle),
    ("hammer", Gear::Hammer),
    ("fish_raw", Gear::Fish { cooked: false }),
    ("fish_cooked", Gear::Fish { cooked: true }),
];

/// Every tool's, sword's and hoe's icon, and the rest of the gear's.
pub(super) fn tiles() -> Vec<(&'static str, Tex)> {
    ICONS.iter().map(|&(name, tool, made)| (name, paint(name, tool, made))).chain(GEAR.iter().map(|&(name, g)| (name, gear(name, g)))).chain(armor::tiles()).chain(ranged::tiles()).collect()
}
