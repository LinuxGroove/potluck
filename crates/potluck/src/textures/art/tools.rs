//! Icons for the things players hold (`blocks::Block::item`): tools and
//! swords. They're never placed, so each tile is only a picture, drawn
//! corner to corner with the handle at the bottom left, in the blocks'
//! palettes, with a dark outline so it reads in the hotbar.

use super::*;

/// What a tool's head is made of.
#[derive(Clone, Copy, PartialEq)]
enum Made {
    Wood,
    Stone,
}

#[derive(Clone, Copy, PartialEq)]
enum Tool {
    Pickaxe,
    Axe,
    Shovel,
    Sword,
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
    };
    Px { pal: 1, t: (t + 0.3 * depth + 0.25 * edge).clamp(0.0, 1.0), h: 0.4 + 0.4 * depth, rough, alpha: 1.0 }
}

fn paint(name: &'static str, tool: Tool, made: Made) -> Tex {
    let s = seed_of(name);
    let head_pal = match made {
        Made::Wood => PLANK,
        Made::Stone => STONE,
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
    // a dark line round it, so it reads on any background
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
    a.finish(1.5, 0.5)
}

/// Every tool's and sword's icon.
pub(super) fn tiles() -> Vec<(&'static str, Tex)> {
    vec![
        ("pickaxe_wood", paint("pickaxe_wood", Tool::Pickaxe, Made::Wood)),
        ("pickaxe_stone", paint("pickaxe_stone", Tool::Pickaxe, Made::Stone)),
        ("axe_wood", paint("axe_wood", Tool::Axe, Made::Wood)),
        ("axe_stone", paint("axe_stone", Tool::Axe, Made::Stone)),
        ("shovel_wood", paint("shovel_wood", Tool::Shovel, Made::Wood)),
        ("shovel_stone", paint("shovel_stone", Tool::Shovel, Made::Stone)),
        ("sword_wood", paint("sword_wood", Tool::Sword, Made::Wood)),
        ("sword_stone", paint("sword_stone", Tool::Sword, Made::Stone)),
    ]
}
