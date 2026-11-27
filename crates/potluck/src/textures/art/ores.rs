//! What's found underground: the six ores (stone with what each holds
//! showing through), the old chests left in old workings, glowcaps, and the
//! icons of what ores give. Ores are painted over the stone tile, so a vein
//! of them sits in the rock round about; crystal and moonsilver glow a
//! little, so they're found in the dark.

use super::*;

// Palettes, dark to light. Glowing tiles glow where a colour's red and
// green add to more than 300, so the stone under crystal and moonsilver
// stops short of the stone palette's lightest two.
const STONE_DIM: &[u32] = &[0x34373f, 0x464a53, 0x5a5e67, 0x6f747b, 0x868a90];
const COAL: &[u32] = &[0x0b0b0d, 0x141417, 0x1e1e23, 0x2b2b32, 0x3c3c46, 0x5a5a68, 0x8c8c9c];
const COPPER: &[u32] = &[0x4f200e, 0x7a3515, 0xa84c1c, 0xd06624, 0xec8a34, 0xffb25a];
const VERDIGRIS: &[u32] = &[0x24594b, 0x377a66, 0x4f9a82, 0x6fb79e];
const IRON: &[u32] = &[0x4a3a36, 0x6c5650, 0x91766c, 0xb5978a, 0xd3b8a8, 0xecd8ca];
const GOLD: &[u32] = &[0x5e3e08, 0x8a5e10, 0xb6841a, 0xdaa82a, 0xf0c84a, 0xfde58a];
const CRYSTAL: &[u32] = &[0x22183e, 0x3a2a6c, 0x58429a, 0x7a62c6, 0xa688ec, 0xccb6ff, 0xeee4ff];
const MOONSILVER: &[u32] = &[0x26324a, 0x3e5072, 0x5f7598, 0x8aa2c4, 0xb4c8e4, 0xdde9f8];
const OUTLINE: &[u32] = &[0x121014, 0x1c1a1e];
const OLD_WOOD: &[u32] = &[0x24180e, 0x352416, 0x46311e, 0x573e27, 0x684c31, 0x7a5b3b];
const BAND: &[u32] = &[0x1d1c1e, 0x2e2d30, 0x434145, 0x5d5a5c, 0x7c776f];
const STEM: &[u32] = &[0x3e3a33, 0x58524a, 0x6f685d, 0x857c6e];
const CAP: &[u32] = &[0x0f3a40, 0x17585c, 0x23807a, 0x3aaa98, 0x6ad4b8, 0xa8f4dc];

fn clear() -> Px {
    Px { pal: 0, t: 0.0, h: 0.0, rough: 1.0, alpha: 0.0 }
}

/// What an ore holds, and how it shows in the rock.
#[derive(Clone, Copy, PartialEq)]
enum Ore {
    Coal,
    Copper,
    Iron,
    Gold,
    Crystal,
    Moonsilver,
}

impl Ore {
    fn pal(self) -> &'static [u32] {
        match self {
            Ore::Coal => COAL,
            Ore::Copper => COPPER,
            Ore::Iron => IRON,
            Ore::Gold => GOLD,
            Ore::Crystal => CRYSTAL,
            Ore::Moonsilver => MOONSILVER,
        }
    }

    fn glows(self) -> bool {
        matches!(self, Ore::Crystal | Ore::Moonsilver)
    }
}

/// A lump of what an ore holds, `d` of the way from its middle to its rim:
/// a dome (the finish lights its top left), with a dark rim, raised from
/// the rock.
fn lump(d: f32, pal: usize, glint: f32) -> Px {
    if d > 0.88 {
        return Px { pal, t: 0.05, h: 0.45, rough: 0.6, alpha: 1.0 };
    }
    let t = 0.3 + 0.45 * (1.0 - d) + 0.6 * glint;
    Px { pal, t, h: 0.6 + 0.35 * (1.0 - d * d), rough: 0.45, alpha: 1.0 }
}

fn ore(name: &'static str, kind: Ore) -> Tex {
    let s = seed_of(name);
    let stone = seed_of("stone2");
    let rock = if kind.glows() { STONE_DIM } else { STONE };
    let mut a = Art::new(name, &[rock, kind.pal(), VERDIGRIS]);
    // how many cells across, how many of them hold some, and how big
    let (cells, some, size) = match kind {
        Ore::Coal => (5, 0.65, 5.5),
        Ore::Copper => (5, 0.6, 5.0),
        Ore::Iron => (6, 0.6, 4.2),
        Ore::Gold => (6, 0.5, 3.6),
        Ore::Crystal => (4, 0.65, 6.5),
        Ore::Moonsilver => (5, 0.55, 4.5),
    };
    a.fill(|x, y, _| {
        let mut p = stone_px(x, y, stone);
        if kind.glows() {
            p.t *= 0.85;
        }
        let c = cell(x, y, cells, cells, 0.8, s);
        if c.rand(1) >= some {
            return p;
        }
        let r = size * (0.7 + 0.6 * c.rand(2)) * (0.75 + 0.5 * fbm(x, y, 8, 2, s ^ 5));
        if c.d > r {
            return p;
        }
        let d = c.d / r;
        let sparkle = hash(x as i32, y as i32, s ^ 41);
        match kind {
            Ore::Crystal => {
                // a shard with two facets, one lit and one in shade
                let facet = if hash(c.id as i32, ((x + y) / 3.0) as i32, s) > 0.5 { 0.25 } else { -0.05 };
                let t = 0.4 + 0.45 * (1.0 - d) + facet + if sparkle > 0.94 { 0.4 } else { 0.0 };
                Px { pal: 1, t, h: 0.75, rough: 0.25, alpha: 1.0 }
            }
            Ore::Moonsilver => {
                // threads of it through the rock, catching the light
                let ang = c.rand(3) * std::f32::consts::TAU;
                let thread = ((x * ang.cos() + y * ang.sin()) * 0.9).sin().abs();
                let t = 0.45 + 0.4 * (1.0 - d) + 0.2 * thread + if sparkle > 0.93 { 0.4 } else { 0.0 };
                Px { pal: 1, t, h: 0.7, rough: 0.3, alpha: 1.0 }
            }
            _ => {
                let glint = if matches!(kind, Ore::Coal | Ore::Gold) && sparkle > 0.9 { 1.0 - d } else { 0.0 };
                let mut q = lump(d, 1, glint);
                // copper goes green in places
                if kind == Ore::Copper && q.t > 0.1 && fbm(x, y, 8, 2, s ^ 13) > 0.56 {
                    q.pal = 2;
                }
                q
            }
        }
    });
    let mut t = a.finish(2.0, 0.9);
    t.glow = kind.glows();
    t
}

/// Draws a dark line round what's painted, so an icon reads on any
/// background.
fn outline(a: &mut Art, pal: usize) {
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
                a.px[i] = Px { pal, t: 0.0, h: 0.3, rough: 1.0, alpha: 1.0 };
            }
        }
    }
}

/// A rough lump of something, as an icon: round but knobbly, lit from the
/// top left, with pits where its cells meet. Coal glints; raw metal is
/// duller with darker pits.
fn chunk(name: &'static str, pal: &'static [u32], glints: bool) -> Tex {
    let s = seed_of(name);
    let mut a = Art::new(name, &[pal, OUTLINE]);
    a.fill(|x, y, _| {
        let (dx, dy) = (x + 0.5 - 32.0, y + 0.5 - 34.0);
        let ang = dy.atan2(dx);
        let edge = 19.0 + 4.0 * (vn(ang * 10.2 + 32.0, 8.0, 16, 4, s) - 0.5) * 2.0 + 2.0 * (3.0 * ang).sin();
        let d = (dx * dx + dy * dy).sqrt();
        if d > edge {
            return clear();
        }
        let c = cell(x, y, 5, 5, 0.9, s ^ 3);
        let inside = 1.0 - d / edge;
        let lit = -(dx + dy) / (edge * 1.5);
        let mut t = 0.35 + 0.25 * inside + 0.35 * lit + 0.1 * (c.rand(1) - 0.5);
        let mut h = 0.4 + 0.5 * inside.sqrt();
        if c.edge < 1.2 {
            t -= 0.2;
            h -= 0.15;
        }
        if glints && hash(x as i32, y as i32, s ^ 7) > 0.93 && lit > -0.1 {
            t = 1.0;
        }
        Px { pal: 0, t, h, rough: if glints { 0.35 } else { 0.6 }, alpha: 1.0 }
    });
    outline(&mut a, 1);
    a.finish(1.5, 0.6)
}

/// Three crystal shards from one foot, as an icon.
fn crystal() -> Tex {
    let mut a = Art::new("crystal", &[CRYSTAL, OUTLINE]);
    // each shard: its foot, which way it points (radians from up), length, half width
    let shards: [(Vec2, f32, f32, f32); 3] = [(Vec2::new(30.0, 52.0), -0.05, 40.0, 7.0), (Vec2::new(26.0, 52.0), -0.55, 26.0, 5.0), (Vec2::new(36.0, 52.0), 0.5, 28.0, 5.5)];
    a.fill(|x, y, _| {
        let p = Vec2::new(x + 0.5, y + 0.5);
        for &(foot, turn, len, half) in &shards {
            let up = Vec2::new(turn.sin(), -turn.cos());
            let side = Vec2::new(-up.y, up.x);
            let (u, v) = ((p - foot).dot(up), (p - foot).dot(side));
            // a prism with a pointed end
            let w = half * (1.0 - smooth(len * 0.72, len, u));
            if (0.0..=len).contains(&u) && v.abs() <= w {
                let face = if v < 0.0 { 0.25 } else { -0.05 };
                let shine = if (v + w * 0.4).abs() < 0.8 && u < len * 0.75 { 0.3 } else { 0.0 };
                return Px { pal: 0, t: 0.45 + face + shine + 0.15 * (u / len), h: 0.6 + 0.3 * (1.0 - (v / half).abs()), rough: 0.2, alpha: 1.0 };
            }
        }
        clear()
    });
    outline(&mut a, 1);
    a.finish(1.5, 0.5)
}

/// Old boards with dark iron bands, for an old chest's sides: the lid's
/// rows (8 to 24) over the box's (24 to 64), a latch in the middle.
fn old_chest_side() -> Tex {
    let s = seed_of("old_chest");
    let mut a = Art::new("old_chest", &[OLD_WOOD, BAND]);
    a.fill(|x, y, _| {
        let grain = ((x * 0.35 + 2.5 * fbm(x, y, 4, 2, s)) * 2.0).sin();
        // boards running across, a dark line between them
        let board = if y < 24.0 { ((y - 8.0) / 8.0).floor() } else { ((y - 24.0) / 10.0).floor() + 2.0 };
        let seam = if y < 24.0 { (y - 8.0).rem_euclid(8.0) < 1.0 } else { (y - 24.0).rem_euclid(10.0) < 1.0 };
        let mut p = px(0, 0.45 + 0.12 * grain + 0.1 * (hash(board as i32, 0, s) - 0.5), 0.5 + 0.1 * grain, 0.85);
        if seam {
            p = px(0, 0.05, 0.1, 0.9);
        }
        // the lid's edge
        if (22.0..24.0).contains(&y) {
            p = px(0, 0.02, 0.05, 0.9);
        }
        // iron bands down each side, and the latch
        let band = (6.0..11.0).contains(&x) || (53.0..58.0).contains(&x);
        let latch = (27.0..37.0).contains(&x) && (17.0..31.0).contains(&y);
        if band || latch {
            let rivet = band && ((y - 4.0).rem_euclid(10.0) < 2.0) && ((x - 7.5).abs() < 1.0 || (x - 54.5).abs() < 1.0);
            let lock = latch && (31.0..33.0).contains(&x) && (23.0..27.0).contains(&y);
            let t = if lock { 0.0 } else if rivet { 0.95 } else { 0.4 + 0.15 * fbm(x, y, 8, 2, s ^ 3) };
            p = Px { pal: 1, t, h: if rivet { 0.95 } else { 0.75 }, rough: 0.5, alpha: 1.0 };
        }
        if y < 8.0 {
            p = clear();
        }
        p
    });
    a.finish(1.6, 0.8)
}

/// An old chest's lid from above: boards with two bands across.
fn old_chest_top() -> Tex {
    let s = seed_of("old_chest_top");
    let mut a = Art::new("old_chest_top", &[OLD_WOOD, BAND]);
    a.fill(|x, y, _| {
        let grain = ((y * 0.35 + 2.5 * fbm(x, y, 4, 2, s)) * 2.0).sin();
        let mut p = px(0, 0.45 + 0.12 * grain + 0.1 * (hash((x / 13.0) as i32, 1, s) - 0.5), 0.5 + 0.1 * grain, 0.85);
        if x.rem_euclid(13.0) < 1.0 {
            p = px(0, 0.05, 0.1, 0.9);
        }
        if (14.0..19.0).contains(&y) || (45.0..50.0).contains(&y) {
            let rivet = (x - 6.0).rem_euclid(13.0) < 2.0 && ((y - 16.5).abs() < 1.0 || (y - 47.5).abs() < 1.0);
            p = Px { pal: 1, t: if rivet { 0.95 } else { 0.4 + 0.15 * fbm(x, y, 8, 2, s ^ 3) }, h: 0.8, rough: 0.5, alpha: 1.0 };
        }
        p
    });
    a.finish(1.6, 0.8)
}

/// Glowcaps: three little mushrooms with pale stems and caps whose spots
/// glow, drawn as a plant is (crossed, see-through round them).
fn glowcap() -> Tex {
    let s = seed_of("glowcap");
    let mut a = Art::new("glowcap", &[STEM, CAP]);
    // each: where its stem stands, its height, its cap's half width
    let caps = [(22.0, 30.0, 10.0), (40.0, 22.0, 8.0), (31.0, 14.0, 6.0)];
    a.fill(|x, y, _| {
        let up = 64.0 - (y + 0.5);
        let mut out = clear();
        for &(cx, tall, half) in &caps {
            let dx = x + 0.5 - cx;
            // the cap: a dome on top of the stem
            let cy = up - tall;
            let dome = (dx / half).powi(2) + (cy / (half * 0.65)).powi(2);
            if cy >= -1.5 && dome <= 1.0 {
                let spot = cell(x, y, 8, 8, 0.8, s);
                let lit = spot.d < 1.6 && spot.rand(1) < 0.6;
                let t = if lit { 1.0 } else { 0.25 + 0.35 * (1.0 - dome) + 0.2 * (-dx / half).max(0.0) };
                out = Px { pal: 1, t, h: 0.6, rough: 0.6, alpha: 1.0 };
                break;
            }
            // its stem, a little wider at the foot
            if up >= 0.0 && up < tall && dx.abs() <= 1.6 + 0.8 * (1.0 - up / tall) {
                out = Px { pal: 0, t: 0.5 - 0.15 * (dx / 2.0), h: 0.5, rough: 0.8, alpha: 1.0 };
            }
        }
        out
    });
    let mut t = a.finish(1.0, 0.4);
    t.glow = true;
    t
}

/// A torch's stick: wood up to a charred top, in the box 7 to 9 across
/// and 0 to 7 up (pixels 28 to 36, rows 36 to 64), coal bound on at the
/// top; the flame over it is the candle's.
fn torch() -> Tex {
    let s = seed_of("torch");
    // the stick's picture runs rows 36 to 64 (boxes 0 to 7), with the
    // candle's flame over it, so the Blocks sheet shows it lit
    let mut a = Art::new("torch", &[PLANK, COAL, super::props::FLAME]);
    a.fill(|x, y, _| {
        if !(28.0..36.0).contains(&x) || y < 36.0 {
            return super::props::flame_px(x, y).unwrap_or_else(clear);
        }
        let round = 0.15 - 0.05 * (x - 28.0);
        if y < 41.0 {
            let ember = hash(x as i32, y as i32, s) > 0.8;
            return px(1, if ember { 0.9 } else { 0.25 + round }, 0.7, 0.6);
        }
        px(0, 0.45 + round + 0.08 * ((y * 0.7).sin()), 0.6, 0.85)
    });
    a.finish(1.2, 0.4)
}

/// A torch's stick seen from above or below: the cut end.
fn torch_top() -> Tex {
    let mut a = Art::new("torch_top", &[PLANK]);
    a.fill(|x, y, _| if (28.0..36.0).contains(&x) && (28.0..36.0).contains(&y) { px(0, 0.3, 0.5, 0.9) } else { clear() });
    a.finish(1.0, 0.3)
}

/// Every ore's, chest's and glowcap's tile, the torch's, and what ores
/// give.
pub(super) fn tiles() -> Vec<(&'static str, Tex)> {
    vec![
        ("ore_coal", ore("ore_coal", Ore::Coal)),
        ("ore_copper", ore("ore_copper", Ore::Copper)),
        ("ore_iron", ore("ore_iron", Ore::Iron)),
        ("ore_gold", ore("ore_gold", Ore::Gold)),
        ("ore_crystal", ore("ore_crystal", Ore::Crystal)),
        ("ore_moonsilver", ore("ore_moonsilver", Ore::Moonsilver)),
        ("old_chest", old_chest_side()),
        ("old_chest_top", old_chest_top()),
        ("glowcap", glowcap()),
        ("torch", torch()),
        ("torch_top", torch_top()),
        ("coal", chunk("coal", COAL, true)),
        ("raw_copper", chunk("raw_copper", COPPER, false)),
        ("raw_iron", chunk("raw_iron", IRON, false)),
        ("raw_gold", chunk("raw_gold", GOLD, true)),
        ("crystal", crystal()),
        ("raw_moonsilver", chunk("raw_moonsilver", MOONSILVER, true)),
    ]
}
