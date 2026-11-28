//! Turning chunks of blocks into meshes: one face per side that can be seen,
//! with ambient occlusion at the corners and, at each corner, how much of
//! the sky it sees (`Sky`).

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::morph::{MeshMorphWeights, MorphAttributes, MorphWeights};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::tasks::ComputeTaskPool;

use std::f32::consts::{FRAC_PI_4, PI, TAU};

use crate::blocks::{self, BlockId, Shape, AIR, BIRCH_LEAVES, BUSH, CARROTS, FERN, GLASS, GRASS, LEAVES, PINE, WATER, WHEAT};
use crate::noise::{fbm, hash2, smooth, vnoise};
use crate::textures::BlockAtlas;
use crate::world::{Voxels, CHUNK};

/// How thickly grass covers the ground: the share of grass blocks that get a
/// tuft where it's lush (fewer on the Steam Deck), whether ground cover
/// near the player casts shadows, and whether leaf fins do. Set from the
/// lighting tier at start.
#[derive(Resource, Clone, Copy)]
pub struct Cover {
    pub density: f32,
    pub shadows: bool,
    /// Off on Low, where a forest's fins cost its shadow pass nearly half
    /// (6 of 13.6 ms on the p16g3's Intel chip): there trees' shadows are
    /// their leaf blocks', and the fins go in with the ground cover
    pub fin_shadows: bool,
    /// What's left out, for measuring what each costs:
    /// `POTLUCK_OFF=tufts,fins,lips,wind,lod,sky` (lod: every chunk in
    /// full; sky: no darker rooms under roofs).
    pub off: Off,
}

#[derive(Clone, Copy, Default)]
pub struct Off {
    pub tufts: bool,
    pub fins: bool,
    pub lips: bool,
    pub wind: bool,
    pub lod: bool,
    pub sky: bool,
    /// Leaves out of the ray traced scene (the tracer counts them as solid)
    pub rtleaves: bool,
    /// Leaves in the ray traced scene as the solid blocks they're drawn
    /// with, in place of the porous stand-in (`porous_leaves`)
    pub rtsolid: bool,
    /// Chunk meshes at full size on the graphics card (`pack`)
    pub pack: bool,
}

impl Off {
    fn from_env() -> Self {
        let v = std::env::var("POTLUCK_OFF").unwrap_or_default();
        let has = |w: &str| v.split(',').any(|o| o.trim() == w);
        Off { tufts: has("tufts"), fins: has("fins"), lips: has("lips"), wind: has("wind"), lod: has("lod"), sky: has("sky"), rtleaves: has("rtleaves"), rtsolid: has("rtsolid"), pack: has("pack") }
    }
}

impl Default for Cover {
    fn default() -> Self {
        Cover { density: 1.0, shadows: true, fin_shadows: true, off: Off::default() }
    }
}

impl Cover {
    pub fn of(tier: crate::render::Tier) -> Self {
        use crate::render::Tier;
        let (density, shadows) = match tier {
            Tier::Low => (0.45, false),
            Tier::Medium => (0.8, false),
            Tier::High | Tier::Ultra => (1.0, true),
        };
        Cover { density, shadows, fin_shadows: tier != Tier::Low, off: Off::from_env() }
    }
}

/// How much detail a chunk's meshes carry, by how far it is from the player.
/// Near, everything, swaying in the wind; further out, fewer, wider tufts
/// that keep still, and past the middle distance no leaf fins; furthest, no
/// tufts either. Out there each would be a few pixels, and tiny triangles
/// are what cards draw slowest.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Detail {
    Near,
    Mid,
    Far,
    Distant,
}

/// Where each level ends, in blocks across from the player.
const DETAIL_RANGE: [f32; 3] = [52.0, 112.0, 176.0];
/// How far past a boundary a chunk goes before it changes level, so walking
/// along one doesn't remesh the same chunks back and forth.
const DETAIL_MARGIN: f32 = 6.0;

impl Detail {
    const ALL: [Detail; 4] = [Detail::Near, Detail::Mid, Detail::Far, Detail::Distant];

    /// The level a chunk should have, leaning toward the one it has now.
    pub fn of(c: IVec3, focus: Vec3, now: Option<Detail>) -> Detail {
        let mid = (c * CHUNK).as_vec3() + Vec3::splat(CHUNK as f32 / 2.0);
        let across = Vec2::new(mid.x - focus.x, mid.z - focus.z).length();
        // only chunks level with the player are near: the morph targets'
        // buffer is small on some cards (see `WAVE`)
        let near = across.max((mid.y - focus.y).abs() * DETAIL_RANGE[0] / (CHUNK as f32 * 1.5));
        let at = now.map_or(usize::MAX, |d| d as usize);
        for (i, &r) in DETAIL_RANGE.iter().enumerate() {
            let r = if at == usize::MAX { r } else if at <= i { r + DETAIL_MARGIN } else { r - DETAIL_MARGIN };
            if (if i == 0 { near } else { across }) < r {
                return Detail::ALL[i];
            }
        }
        Detail::Distant
    }

    /// The share of tufts kept.
    fn tufts(self) -> f32 {
        match self {
            Detail::Near => 1.0,
            Detail::Mid => 0.5,
            Detail::Far => 0.2,
            Detail::Distant => 0.0,
        }
    }
}

/// The colour grass takes where it grows: lush green across most of the
/// island, with drier, yellower patches, and a little unevenness close up,
/// shifted by the climate there (`climate_tint`).
pub fn grass_tint(climate: Vec3, x: f32, z: f32) -> Vec3 {
    let dry = smooth(0.45, 0.72, fbm(x / 41.0, z / 41.0, 317, 3));
    Vec3::new(0.92, 1.04, 0.9).lerp(Vec3::new(1.16, 1.05, 0.64), dry) * (0.93 + 0.14 * vnoise(x / 5.0, z / 5.0, 319)) * climate
}

/// How a place's climate shifts the green of what grows there: yellower
/// where it's warm and dry, cooler and darker where it's cold, deeper where
/// it's wet. Exactly white in the island's mild climate (a half each), so
/// the island and the lands near it keep their colours.
pub fn climate_tint(c: crate::world::Climate) -> Vec3 {
    let (warm, wet) = (c.warm - 0.5, c.wet - 0.5);
    let dry = ((warm * 0.9 - wet) * 2.0).clamp(0.0, 1.0);
    let cold = (-warm * 2.0).clamp(0.0, 1.0);
    let lush = ((wet - warm.max(0.0)) * 2.0).clamp(0.0, 1.0);
    Vec3::ONE.lerp(Vec3::new(1.2, 1.02, 0.6), dry) * Vec3::ONE.lerp(Vec3::new(0.8, 0.93, 1.02), cold) * Vec3::ONE.lerp(Vec3::new(0.84, 0.98, 0.8), lush)
}

/// Leaves take less of the climate's shift than grass does.
pub fn leaf_tint(climate: Vec3) -> Vec3 {
    Vec3::ONE + (climate - Vec3::ONE) * 0.6
}

/// The climate's tint across one column (`climate_tint` at its corners,
/// blended between), worked out once for each chunk meshed: the climate
/// changes over hundreds of blocks, and working it out for every grass
/// block's corners would cost more than the rest of the meshing.
pub struct Tints {
    o: Vec2,
    c: [Vec3; 4],
}

impl Tints {
    pub fn of(seed: u32, key: IVec2) -> Tints {
        let o = (key * CHUNK).as_vec2();
        let at = |dx: f32, dz: f32| climate_tint(crate::world::climate(seed, o.x + dx, o.y + dz));
        let s = CHUNK as f32;
        Tints { o, c: [at(0.0, 0.0), at(s, 0.0), at(0.0, s), at(s, s)] }
    }

    pub fn at(&self, x: f32, z: f32) -> Vec3 {
        let (u, v) = (((x - self.o.x) / CHUNK as f32).clamp(0.0, 1.0), ((z - self.o.y) / CHUNK as f32).clamp(0.0, 1.0));
        let top = self.c[0].lerp(self.c[1], u);
        top.lerp(self.c[2].lerp(self.c[3], u), v)
    }
}

fn rgba(c: Vec3) -> [f32; 4] {
    [c.x, c.y, c.z, 1.0]
}

#[derive(Default)]
pub struct MeshData {
    pos: Vec<[f32; 3]>,
    nrm: Vec<[f32; 3]>,
    uv: Vec<[f32; 2]>,
    col: Vec<[f32; 4]>,
    tan: Vec<[f32; 4]>,
    idx: Vec<u32>,
    /// How far each vertex moves in the wind (plant tips; zero elsewhere).
    sway: Vec<Vec3>,
    /// How much of the sky each vertex sees (`Sky`), 1 until worked out.
    sky: Vec<f32>,
}

/// Plants sway in a breeze. Each plant's tip has a sway (how far and which
/// way it can move), shaped by a wave running across the island into the
/// morph targets of its chunk's mesh; `wind` moves the wave along with time,
/// so gusts roll over the fields without any mesh being rebuilt. All morph
/// targets share one buffer that some cards bind 128 MiB of at most, so only
/// chunks near the player sway (`Detail::Near`).
const WAVE: (Vec2, f32) = (Vec2::new(0.13, 0.06), 1.3);
/// Which way the wind blows, mostly.
const WIND: Vec3 = Vec3::new(0.91, 0.0, 0.41);
const WIND_TARGETS: usize = 3;

/// The entity whose morph weights every swaying mesh follows.
#[derive(Resource)]
pub struct Wind(pub Entity);

pub fn spawn_wind(commands: &mut Commands) {
    let e = commands.spawn(MorphWeights::new(vec![0.0; WIND_TARGETS], None).unwrap()).id();
    commands.insert_resource(Wind(e));
}

/// Moves the wave along: plants follow `cos(k . p - w t)`, leaning a little
/// more in gusts.
pub fn wind(time: Res<Time>, wind: Option<Res<Wind>>, mut weights: Query<&mut MorphWeights>) {
    let Some(wind) = wind else { return };
    let Ok(mut w) = weights.get_mut(wind.0) else { return };
    let t = time.elapsed_secs();
    let a = WAVE.1 * t;
    let gust = 0.6 + 0.4 * (0.21 * t).sin() * (0.13 * t).cos();
    w.weights_mut().copy_from_slice(&[a.cos(), a.sin(), gust]);
}

impl MeshData {
    pub fn is_empty(&self) -> bool {
        self.idx.is_empty()
    }

    /// Turns each quad's normal to the way it faces. Plants are shaded as if
    /// they faced up, which reads well with shadow maps, but the ray tracer
    /// checks normals against the surface and turns those pixels black.
    pub fn face_normals(&mut self) {
        for q in 0..self.pos.len() / 4 {
            let k = q * 4;
            let [a, b, c] = [self.pos[k], self.pos[k + 1], self.pos[k + 2]].map(Vec3::from);
            let n = (b - a).cross(c - a).normalize_or_zero();
            let up = Vec3::from(self.pos[k + 3]) - a;
            for i in k..k + 4 {
                self.nrm[i] = n.into();
                let t = Vec3::new(self.tan[i][0], self.tan[i][1], self.tan[i][2]);
                self.tan[i][3] = if n.cross(t).dot(up) >= 0.0 { 1.0 } else { -1.0 };
            }
        }
    }

    fn quad(&mut self, corners: [[f32; 3]; 4], n: [f32; 3], rect: [f32; 4], light: [f32; 4], flip: bool) {
        self.quad_tinted(corners, n, rect, light.map(|l| [l, l, l, 1.0]), flip);
    }

    /// A quad with a colour at each corner.
    fn quad_tinted(&mut self, corners: [[f32; 3]; 4], n: [f32; 3], rect: [f32; 4], tint: [[f32; 4]; 4], flip: bool) {
        let [u0, v0, u1, v1] = rect;
        self.quad_uv(corners, n, [[u0, v1], [u1, v1], [u1, v0], [u0, v0]], tint, flip);
    }

    /// A quad with its own UVs at each corner (corner 0 to 1 runs along u).
    fn quad_uv(&mut self, corners: [[f32; 3]; 4], n: [f32; 3], uvs: [[f32; 2]; 4], tint: [[f32; 4]; 4], flip: bool) {
        let base = self.pos.len() as u32;
        let t = Vec3::from(corners[1]) - Vec3::from(corners[0]);
        let b = Vec3::from(corners[3]) - Vec3::from(corners[0]);
        let t = t.normalize_or_zero();
        // the bitangent points up the texture (towards v0)
        let w = if Vec3::from(n).cross(t).dot(b) >= 0.0 { 1.0 } else { -1.0 };
        for k in 0..4 {
            self.pos.push(corners[k]);
            self.nrm.push(n);
            self.uv.push(uvs[k]);
            self.col.push(tint[k]);
            self.tan.push([t.x, t.y, t.z, w]);
            self.sway.push(Vec3::ZERO);
            self.sky.push(1.0);
        }
        if flip {
            self.idx.extend_from_slice(&[base + 1, base + 2, base + 3, base + 1, base + 3, base]);
        } else {
            self.idx.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }
    }

    /// Lets the last quad's top edge (its last two corners) move `d` in the wind.
    fn sway_top(&mut self, d: Vec3) {
        let n = self.sway.len();
        self.sway[n - 2] = d;
        self.sway[n - 1] = d;
    }

    /// The wind's morph targets (see `WAVES`), if anything here sways.
    fn wind_targets(&self) -> Option<Vec<MorphAttributes>> {
        if self.sway.iter().all(|d| *d == Vec3::ZERO) {
            return None;
        }
        let mut out = Vec::with_capacity(self.pos.len() * WIND_TARGETS);
        for t in 0..WIND_TARGETS {
            for (p, d) in self.pos.iter().zip(&self.sway) {
                let at = Vec2::new(p[0], p[2]);
                let f = match t {
                    0 => WAVE.0.dot(at).cos(),
                    1 => WAVE.0.dot(at).sin(),
                    _ => 1.0,
                };
                out.push(MorphAttributes::new(*d * f, Vec3::ZERO, Vec3::ZERO));
            }
        }
        Some(out)
    }

    /// The mesh, with corner shading in its vertex colours and the sky each
    /// corner sees in its second UVs, unless the world is ray traced (which
    /// shades corners and finds the sky itself, and takes only position,
    /// normal, UV and tangent).
    pub fn into_mesh(self, raytraced: bool) -> Mesh {
        let targets = self.wind_targets();
        let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.pos)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.nrm)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.uv);
        if !raytraced {
            m.insert_attribute(Mesh::ATTRIBUTE_COLOR, self.col);
            m.insert_attribute(Mesh::ATTRIBUTE_UV_1, self.sky.iter().map(|&s| [sky_u(s), 0.5]).collect::<Vec<_>>());
        }
        if let Some(t) = targets {
            m.set_morph_targets(t);
        }
        m.with_inserted_attribute(Mesh::ATTRIBUTE_TANGENT, self.tan).with_inserted_indices(Indices::U32(self.idx))
    }
}

/// Packs a chunk mesh smaller for the graphics card, half the bytes a
/// vertex: normals and tangents as two 16-bit numbers each, UVs as 16-bit
/// fractions, colours as half floats (grass tints go past 1), and 16-bit
/// indices where they fit. Bevy's shaders unpack them. Positions stay full
/// floats, so the edges of neighbouring chunks meet exactly. The ray tracer
/// reads meshes itself, so the ones it sees stay as they are.
pub fn pack(mesh: &mut Mesh) {
    use bevy::mesh::{AttributeQuantization, MeshAttributeCompressionFlags as F, MeshCompressionArgs};
    let args = MeshCompressionArgs {
        compress_indices: true,
        quantize_attributes: (&[(Mesh::ATTRIBUTE_COLOR.id, AttributeQuantization::Float16)]).into(),
        compress_attributes: F::COMPRESS_NORMAL | F::COMPRESS_TANGENT | F::COMPRESS_UV0 | F::COMPRESS_UV1,
    };
    if let Err(e) = mesh.compress_mesh(&args) {
        warn!("Couldn't pack a chunk mesh: {e:?}");
    }
}

/// A chunk's meshes: opaque blocks, cut-out blocks (leaves, glass), water,
/// plants (cut out too, but kept apart so ray tracing can leave them out:
/// it counts everything as solid, so plants would cast shadows like cards),
/// glowing blocks (lanterns: ray tracing treats every glowing mesh as a
/// lamp, so only these glow, or the sun would be one lamp among hundreds),
/// ground cover (tufts and grass lips, which aren't blocks: thousands of
/// them, so they cast shadows only on desktop cards) and blocks under water
/// (their corners darken with depth, but ray traced meshes have no colours,
/// so there the material tints them).
pub struct ChunkMesh {
    pub layers: [MeshData; LAYERS],
    /// Whether anything in it changes with its `Detail` (grass, leaves, plants).
    pub varies: bool,
}

pub const LAYERS: usize = 7;
/// The ground cover layer.
pub const COVER: usize = 5;
/// Faces of solid blocks under water.
pub const UNDER: usize = 6;

/// Which material each layer of a chunk uses.
pub const LAYER_MATERIAL: [usize; LAYERS] = [0, 1, 2, 1, 3, 1, 4];

struct Face {
    n: [i32; 3],
    c: [[i32; 3]; 4],
    /// Which of the block's textures: 0 top, 1 side, 2 bottom.
    s: usize,
}

const FACES: [Face; 6] = [
    Face { n: [1, 0, 0], c: [[1, 0, 1], [1, 0, 0], [1, 1, 0], [1, 1, 1]], s: 1 },
    Face { n: [-1, 0, 0], c: [[0, 0, 0], [0, 0, 1], [0, 1, 1], [0, 1, 0]], s: 1 },
    Face { n: [0, 1, 0], c: [[0, 1, 1], [1, 1, 1], [1, 1, 0], [0, 1, 0]], s: 0 },
    Face { n: [0, -1, 0], c: [[0, 0, 0], [1, 0, 0], [1, 0, 1], [0, 0, 1]], s: 2 },
    Face { n: [0, 0, 1], c: [[0, 0, 1], [1, 0, 1], [1, 1, 1], [0, 1, 1]], s: 1 },
    Face { n: [0, 0, -1], c: [[1, 0, 0], [0, 0, 0], [0, 1, 0], [1, 1, 0]], s: 1 },
];

const AO: [f32; 4] = [1.0, 0.78, 0.6, 0.45];

/// How far sky light is looked for, up and across, in blocks.
pub const SKY_REACH: i32 = 12;
/// How many steps the ambient light curve's texture has (`sky_ambient`).
pub const SKY_STEPS: usize = 64;

/// How much of the ambient light (the sky's and the flat fill) a face gets
/// for the share of the sky it sees: a little always, since light bounces in
/// through doors and windows, and all of it in the open.
pub fn sky_ambient(s: f32) -> f32 {
    0.13 + 0.87 * s.clamp(0.0, 1.0)
}

/// Where a share of sky falls in the curve's texture.
fn sky_u(s: f32) -> f32 {
    (0.5 + s.clamp(0.0, 1.0) * (SKY_STEPS - 1) as f32) / SKY_STEPS as f32
}

/// How much light a block lets through on the way to the sky: solid blocks
/// none, leaves, glass and water some.
fn sky_through(b: BlockId) -> f32 {
    match b {
        AIR => 1.0,
        // crowns are full of gaps: a forest floor keeps a good share of sky
        LEAVES | BIRCH_LEAVES | PINE => 0.8,
        GLASS => 0.85,
        WATER => 0.8,
        _ if blocks::is_opaque(b) => 0.0,
        _ => 1.0,
    }
}

/// How much of the sky the open cells round a chunk see, which is how much
/// of the ambient light reaches the faces beside them. Rooms under a roof
/// would otherwise be lit by the open sky as if they had none. Only ambient
/// light is scaled (through the chunk materials' occlusion texture, read
/// with the second UVs), so sun through a skylight still lands bright on a
/// library floor whose walls are in shade. Rays go up from each cell:
/// straight up, eight halfway to the horizon and four lower ones that find
/// doors and windows; further out, where rooms can't be seen into, four
/// halfway instead of eight.
struct Sky {
    /// The blocks round the chunk the rays can reach, copied out of the
    /// world so tracing stays in the cache.
    ids: Vec<BlockId>,
    /// Each cell's share of sky (for cells in and just round the chunk),
    /// NaN until it's worked out and -1 for solid blocks.
    cells: Vec<f32>,
    rays: &'static [SkyRay],
    /// Where each ray's cells are in `ids`, from the cell it starts in.
    steps: Vec<Vec<usize>>,
    /// The corner of `ids`, and of `cells`.
    lo: IVec3,
    cells_lo: IVec3,
}

/// A ray to the sky: the cells it crosses from a cell's middle, in order,
/// and how much of the sky it stands for.
struct SkyRay {
    cells: Vec<IVec3>,
    weight: f32,
}

/// How far past the chunk's edges cells are worked out (vertices of fins,
/// lips and models reach a little outside it).
const SKY_PAD: i32 = 3;
const SKY_SPAN: i32 = CHUNK + 2 * SKY_PAD;
/// How far round the chunk the copy of blocks reaches: far enough for every
/// ray from every cell worked out.
const SKY_AROUND: IVec3 = IVec3::new(SKY_PAD + SKY_REACH, SKY_PAD, SKY_PAD + SKY_REACH);
const SKY_REGION: IVec3 = IVec3::new(CHUNK + 2 * SKY_AROUND.x, CHUNK + 2 * SKY_PAD + SKY_REACH + 1, CHUNK + 2 * SKY_AROUND.z);

/// How much sky light each block lets through (`sky_through`), by id.
fn sky_through_all() -> &'static [f32; 256] {
    static ALL: std::sync::OnceLock<[f32; 256]> = std::sync::OnceLock::new();
    ALL.get_or_init(|| std::array::from_fn(|i| sky_through(i as BlockId)))
}

/// How much of the sky a point sees, 0 to 1, by the same rays chunks are
/// shaded with: for the eye adjusting under trees and roofs (`render::sky`).
pub fn sky_seen(w: &Voxels, p: Vec3) -> f32 {
    let pass = sky_through_all();
    let at = p.floor().as_ivec3();
    let mut seen = 0.0;
    for ray in sky_rays(true) {
        let mut through = 1.0;
        for c in &ray.cells {
            let q = at + *c;
            through *= pass[w.get(q.x, q.y, q.z) as usize];
            if through < 0.03 {
                through = 0.0;
                break;
            }
        }
        seen += through * ray.weight;
    }
    seen
}

/// The rays near the player and further out (see `Sky`).
fn sky_rays(full: bool) -> &'static [SkyRay] {
    static RAYS: std::sync::OnceLock<[Vec<SkyRay>; 2]> = std::sync::OnceLock::new();
    let make = |full: bool| {
        let mut dirs = vec![(Vec3::Y, 2.0)];
        let mut ring = |up: f32, n: usize, weight: f32| {
            for k in 0..n {
                let a = k as f32 * TAU / n as f32;
                dirs.push((Vec3::new(a.cos() * up.cos(), up.sin(), a.sin() * up.cos()), weight));
            }
        };
        if full {
            ring(FRAC_PI_4, 8, 0.75);
        } else {
            ring(FRAC_PI_4, 4, 1.5);
        }
        ring(0.33, 4, 0.8);
        let total: f32 = dirs.iter().map(|d| d.1).sum();
        dirs.into_iter()
            .map(|(d, weight)| {
                // every cell the ray passes through, in small enough steps not
                // to slip between two blocks meeting at an edge
                let mut cells: Vec<IVec3> = Vec::new();
                let mut s = 0.25;
                while s <= SKY_REACH as f32 {
                    let p = (Vec3::splat(0.5) + d * s).floor().as_ivec3();
                    if p != IVec3::ZERO && cells.last() != Some(&p) {
                        cells.push(p);
                    }
                    s += 0.25;
                }
                SkyRay { cells, weight: weight / total }
            })
            .collect::<Vec<_>>()
    };
    &RAYS.get_or_init(|| [make(true), make(false)])[if full { 0 } else { 1 }]
}

impl Sky {
    fn new(w: &Voxels, o: IVec3, detail: Detail) -> Self {
        let rays = sky_rays(matches!(detail, Detail::Near | Detail::Mid));
        let lo = o - SKY_AROUND;
        let n = SKY_REGION;
        let mut ids = Vec::with_capacity((n.x * n.y * n.z) as usize);
        for y in lo.y..lo.y + n.y {
            for z in lo.z..lo.z + n.z {
                for x in lo.x..lo.x + n.x {
                    ids.push(w.get(x, y, z));
                }
            }
        }
        let steps = rays.iter().map(|r| r.cells.iter().map(|d| ((d.y * n.z + d.z) * n.x + d.x) as isize as usize).collect()).collect();
        Sky { ids, cells: vec![f32::NAN; (SKY_SPAN * SKY_SPAN * SKY_SPAN) as usize], rays, steps, lo, cells_lo: o - IVec3::splat(SKY_PAD) }
    }

    /// The share of sky a cell sees, or None for a solid block (or a cell
    /// too far out of the chunk to have been copied).
    fn cell(&mut self, c: IVec3) -> Option<f32> {
        let l = c - self.cells_lo;
        if l.cmplt(IVec3::ZERO).any() || l.cmpge(IVec3::splat(SKY_SPAN)).any() {
            return None;
        }
        let i = ((l.y * SKY_SPAN + l.z) * SKY_SPAN + l.x) as usize;
        if self.cells[i].is_nan() {
            let r = c - self.lo;
            let at = ((r.y * SKY_REGION.z + r.z) * SKY_REGION.x + r.x) as usize;
            self.cells[i] = if blocks::is_opaque(self.ids[at]) { -1.0 } else { self.trace(at) };
        }
        let v = self.cells[i];
        (v >= 0.0).then_some(v)
    }

    /// The share of sky seen from the cell at `at` in the copied blocks.
    fn trace(&self, at: usize) -> f32 {
        let pass = sky_through_all();
        let mut seen = 0.0;
        for (ray, steps) in self.rays.iter().zip(&self.steps) {
            let mut through = 1.0;
            for &d in steps {
                let t = pass[self.ids[at.wrapping_add(d)] as usize];
                if t < 1.0 {
                    through *= t;
                    if through < 0.03 {
                        through = 0.0;
                        break;
                    }
                }
            }
            seen += through * ray.weight;
        }
        seen
    }

    /// The sky seen at a vertex: the open cells round the point half a block
    /// out along its normal, blended by how near each is.
    fn at(&mut self, p: Vec3, n: Vec3) -> f32 {
        let q = p + n * 0.5 - Vec3::splat(0.5);
        let base = q.floor();
        let f = q - base;
        let b = base.as_ivec3();
        let (wx, wy, wz) = ([1.0 - f.x, f.x], [1.0 - f.y, f.y], [1.0 - f.z, f.z]);
        let (mut sum, mut weights) = (0.0, 0.0);
        for dz in 0..2 {
            for dy in 0..2 {
                for dx in 0..2 {
                    let wgt = wx[dx] * wy[dy] * wz[dz];
                    if wgt < 1e-4 {
                        continue;
                    }
                    if let Some(s) = self.cell(b + IVec3::new(dx as i32, dy as i32, dz as i32)) {
                        sum += wgt * s;
                        weights += wgt;
                    }
                }
            }
        }
        if weights > 0.0 { sum / weights } else { 1.0 }
    }

    /// Works out every vertex's sky in a chunk's layers (water reflects the
    /// sky rather than taking light from it).
    fn shade(&mut self, layers: &mut [MeshData; LAYERS]) {
        for (i, l) in layers.iter_mut().enumerate() {
            if i == 2 {
                continue;
            }
            for v in 0..l.pos.len() {
                l.sky[v] = self.at(Vec3::from(l.pos[v]), Vec3::from(l.nrm[v]));
            }
        }
    }
}

/// How much of red, green and blue each block of water soaks up, on the way
/// down to what's under it and back up.
pub(crate) const UNDERWATER: [f32; 3] = [0.25, 0.06, 0.045];

/// Water's colour and how much it lets through, by how many blocks deep it
/// is: the shallows over sand are pale and clear, the deep is dark blue. The
/// far sea (`render::far`) takes these too, as deep as it is.
pub fn water_tint(depth: f32) -> [f32; 4] {
    let t = crate::noise::smooth(0.5, 6.0, depth);
    let c = Vec3::new(0.16, 0.62, 0.6).lerp(Vec3::new(0.008, 0.05, 0.09), t);
    [c.x, c.y, c.z, 0.38 + 0.57 * t]
}

/// Grass over a grass block: a lip hanging over each open side, so terraces
/// don't show a line of dirt, and a tuft on most tops, tinted lush or dry by
/// where it grows.
fn ground_cover(m: &mut MeshData, w: &Voxels, atlas: &BlockAtlas, x: i32, y: i32, z: i32, cover: Cover, detail: Detail, tints: &Tints) {
    let above = w.get(x, y + 1, z);
    if blocks::is_opaque(above) || above == WATER {
        return;
    }
    let (fx, fy, fz) = (x as f32, y as f32, z as f32);
    let tint = grass_tint(tints.at(fx + 0.5, fz + 0.5), fx + 0.5, fz + 0.5);
    let up = [0.0, 1.0, 0.0];
    let lip = BlockAtlas::uv(atlas.tile("grass_lip"));
    for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
        if cover.off.lips || blocks::is_opaque(w.get(x + dx, y, z + dz)) {
            continue;
        }
        let out = Vec3::new(dx as f32, 0.0, dz as f32);
        let side = Vec3::new(-dz as f32, 0.0, dx as f32);
        let edge = Vec3::new(fx + 0.5, fy + 1.02, fz + 0.5) + out * 0.53;
        let (tl, tr) = (edge - side * 0.52, edge + side * 0.52);
        let down = out * 0.1 - Vec3::Y * 0.42;
        // lit as if it faced up, like the top it hangs from
        m.quad_tinted([(tl + down).into(), (tr + down).into(), tr.into(), tl.into()], up, lip, [rgba(tint * 0.72), rgba(tint * 0.72), rgba(tint), rgba(tint)], false);
    }
    let keep = if cover.off.lod { 1.0 } else { detail.tufts() };
    if above != AIR || cover.density <= 0.0 || keep <= 0.0 || cover.off.tufts {
        return;
    }
    let hh = |s: i32| hash2(x.wrapping_add(y.wrapping_mul(7919)), z, s);
    let lush = fbm(fx / 26.0, fz / 26.0, 311, 3);
    if hh(81) > cover.density * keep * (0.3 + 0.7 * smooth(0.28, 0.6, lush)) {
        return;
    }
    let field = fbm(fx / 15.0, fz / 15.0, 313, 3);
    let (kind, scale) = if field > 0.6 && hh(82) < 0.6 {
        ("tuft_flowers", 1.0)
    } else if lush > 0.55 && hh(83) < 0.3 {
        ("tuft_tall", 1.45)
    } else {
        ("tuft", 1.0)
    };
    let rect = BlockAtlas::uv(atlas.tile(kind));
    let tall = (0.45 + 0.35 * hh(84)) * (0.75 + 0.5 * lush) * scale;
    // fewer tufts further out are wider, so the ground looks as covered
    let hw = (0.5 + 0.12 * hh(85)) * (1.0 / keep).sqrt().min(1.5);
    let c = Vec3::new(fx + 0.5 + (hh(86) - 0.5) * 0.35, fy + 0.98, fz + 0.5 + (hh(87) - 0.5) * 0.35);
    let top = Vec3::Y * tall + Vec3::new(hh(88) - 0.5, 0.0, hh(89) - 0.5) * 0.25 * tall;
    let mut t = tint * (0.88 + 0.22 * hh(91));
    if kind == "tuft_flowers" {
        // petals keep their colours
        t = t.lerp(Vec3::ONE, 0.5);
    }
    let a0 = hh(90) * PI;
    for i in 0..3 {
        let ang = a0 + i as f32 * PI / 3.0;
        let d = Vec3::new(ang.cos(), 0.0, ang.sin()) * hw;
        let (a, b) = (c - d, c + d);
        m.quad_tinted([a.into(), b.into(), (b + top).into(), (a + top).into()], up, rect, [rgba(t * 0.55), rgba(t * 0.55), rgba(t), rgba(t)], false);
        m.sway_top(WIND * tall * (0.09 + 0.06 * hh(92 + i)));
    }
}

/// A model's boxes (`blocks::Cuboid`). Each face shows the part of its
/// texture it covers, as if the box were cut from a full block, so faces
/// line up across boxes; faces against a full block are left out. Corners
/// darken toward the floor, since models get no corner shading of their own.
fn model(layers: &mut [MeshData; LAYERS], w: &Voxels, atlas: &BlockAtlas, x: i32, y: i32, z: i32, id: BlockId) {
    let def = blocks::block(id);
    let next = |d: usize| {
        let s = blocks::SIDES[d];
        w.get(x + s[0], y + s[1], z + s[2])
    };
    let o = Vec3::new(x as f32, y as f32, z as f32);
    // standing on the ground, darker toward it (stacked, one model's light runs on into the next)
    let floor = blocks::is_opaque(w.get(x, y - 1, z));
    for b in def.model {
        if !b.when.holds(id, next) {
            continue;
        }
        let lo = Vec3::from(b.from.map(|v| v as f32 / 16.0));
        let hi = Vec3::from(b.to.map(|v| v as f32 / 16.0));
        let layer = if b.glow { 4 } else { 0 };
        for f in &FACES {
            let axis = (0..3).find(|&i| f.n[i] != 0).unwrap();
            let out = if f.n[axis] > 0 { hi[axis] >= 1.0 } else { lo[axis] <= 0.0 };
            if out && blocks::is_opaque(w.get(x + f.n[0], y + f.n[1], z + f.n[2])) {
                continue;
            }
            let name = if b.tex[f.s].is_empty() { def.faces[f.s] } else { b.tex[f.s] };
            let [u0, v0, u1, v1] = BlockAtlas::uv(atlas.tile(name));
            // where each corner is in the block, and the UV a full block's
            // face would have there (corner k of a full face has these)
            let unit = [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];
            let local = f.c.map(|c| Vec3::new([lo.x, hi.x][c[0] as usize], [lo.y, hi.y][c[1] as usize], [lo.z, hi.z][c[2] as usize]));
            let uvs = local.map(|p| {
                let mut uv = [0.0f32; 2];
                for (k, c) in f.c.iter().enumerate() {
                    let wgt: f32 = (0..3).filter(|&i| i != axis).map(|i| if c[i] == 1 { p[i] } else { 1.0 - p[i] }).product();
                    uv[0] += wgt * unit[k][0];
                    uv[1] += wgt * unit[k][1];
                }
                [u0 + (u1 - u0) * uv[0], v0 + (v1 - v0) * uv[1]]
            });
            let down = if f.n[1] < 0 { 0.6 } else { 1.0 };
            let tint = local.map(|p| {
                let l = if b.glow { 1.0 } else if floor { down * (0.62 + 0.38 * p.y) } else { down };
                [l, l, l, 1.0]
            });
            layers[layer].quad_uv(local.map(|p| (o + p).into()), f.n.map(|v| v as f32), uvs, tint, false);
        }
    }
}

/// Surf where water meets the land: a strip of foam on the water along each
/// side of the block that has the shore beside it, its foamy edge to the
/// land. Near the player it laps in and out with the wind's wave. It goes
/// with the plants (cut out, and kept out of ray tracing).
fn shore_foam(m: &mut MeshData, w: &Voxels, atlas: &BlockAtlas, x: i32, y: i32, z: i32, detail: Detail) {
    let [u0, v0, u1, v1] = BlockAtlas::uv(atlas.tile("foam"));
    let yy = y as f32 + 0.9;
    let c = Vec3::new(x as f32 + 0.5, yy, z as f32 + 0.5);
    for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
        if !blocks::is_opaque(w.get(x + dx, y, z + dz)) {
            continue;
        }
        let d = Vec3::new(dx as f32, 0.0, dz as f32);
        // along the shore, turning left from facing it
        let along = Vec3::new(dz as f32, 0.0, -dx as f32);
        let e0 = c + d * 0.5 - along * 0.5;
        let e1 = c + d * 0.5 + along * 0.5;
        let q = [e0, e1, e1 - d, e0 - d].map(|p| p.into());
        m.quad_uv(q, [0.0, 1.0, 0.0], [[u0, v0], [u1, v0], [u1, v1], [u0, v1]], [[1.0; 4]; 4], false);
        if detail == Detail::Near {
            let n = m.sway.len();
            for s in &mut m.sway[n - 4..] {
                *s = d * 0.07;
            }
        }
    }
}

/// Leaf fins: two sheets of leaves standing out of a leaf block's open face,
/// crossed and turned at random, so a canopy's outline is ragged clumps of
/// leaves rather than the edge of a cube.
fn leaf_fins(m: &mut MeshData, corners: [[f32; 3]; 4], n: [f32; 3], rect: [f32; 4], light: [f32; 4], tint: Vec3, h: f32) {
    let c = corners.iter().fold(Vec3::ZERO, |a, p| a + Vec3::from(*p)) / 4.0;
    let n3 = Vec3::from(n);
    // two axes across the face, turned by the hash
    let a = (Vec3::from(corners[1]) - Vec3::from(corners[0])).normalize();
    let b = n3.cross(a);
    let ang = h * PI;
    let reach = 0.42 + 0.2 * (h * 17.0).fract();
    let base = c - n3 * 0.2 + (a * ((h * 5.0).fract() - 0.5) + b * ((h * 9.0).fract() - 0.5)) * 0.3;
    let [u0, v0, u1, v1] = rect;
    for (k, turn) in [0.0, PI / 2.0].into_iter().enumerate() {
        let across = (a * (ang + turn).cos() + b * (ang + turn).sin()) * 0.62;
        let out = n3 * (0.2 + reach);
        let q = [base - across, base + across, base + across + out, base - across + out];
        // the sheet's outer edge is the leaves' top
        let rect = if k == 0 { rect } else { [u1, v0, u0, v1] };
        let side = across.cross(out).normalize_or_zero();
        m.quad_tinted(q.map(|p| p.into()), side.into(), rect, light.map(|l| rgba(tint * l)), false);
    }
}

/// The ray tracer's stand-in for a chunk's leaves. It counts everything as
/// solid, so a canopy of whole leaf blocks would shut out the sky and leave
/// the forest floor black; here about half the leaf blocks are left out and
/// the rest are smaller boxes, nudged about inside their cells, so sun and
/// sky come down through the gaps in patches, as they do through the drawn
/// leaves' cut-outs.
pub fn porous_leaves(w: &Voxels, c: IVec3) -> MeshData {
    let mut m = MeshData::default();
    let o = c * CHUNK;
    for y in o.y..o.y + CHUNK {
        for z in o.z..o.z + CHUNK {
            for x in o.x..o.x + CHUNK {
                if !matches!(w.get(x, y, z), LEAVES | BIRCH_LEAVES | PINE) {
                    continue;
                }
                let r = |k: i32| hash2(x * 31 + k, z * 17 - y * 5 + k * 3, 911 + y);
                if r(4) < 0.55 {
                    continue;
                }
                let size = 0.52 + 0.1 * r(0);
                let room = 1.0 - size;
                let mid = Vec3::new(x as f32 + 0.5 + (r(1) - 0.5) * room, y as f32 + 0.5 + (r(2) - 0.5) * room * 0.6, z as f32 + 0.5 + (r(3) - 0.5) * room);
                for f in &FACES {
                    let corners = f.c.map(|cc| (mid + (Vec3::new(cc[0] as f32, cc[1] as f32, cc[2] as f32) - 0.5) * size).to_array());
                    m.quad(corners, f.n.map(|v| v as f32), [0.0, 0.0, 1.0, 1.0], [1.0; 4], false);
                }
            }
        }
    }
    m
}

/// A chunk's meshes, with as much detail as `detail` says; near the player,
/// its plants move in the wind.
pub fn mesh_chunk(w: &Voxels, atlas: &BlockAtlas, c: IVec3, cover: Cover, detail: Detail) -> ChunkMesh {
    let detail = if cover.off.lod { Detail::Near } else { detail };
    // past the middle distance a fin is a few pixels
    let fins = matches!(detail, Detail::Near | Detail::Mid) && !cover.off.fins;
    let mut varies = false;
    let mut layers: [MeshData; LAYERS] = Default::default();
    let o = c * CHUNK;
    let tints = Tints::of(w.seed, IVec2::new(c.x, c.z));
    let opaque = |x: i32, y: i32, z: i32| blocks::is_opaque(w.get(x, y, z));
    for y in o.y..o.y + CHUNK {
        for z in o.z..o.z + CHUNK {
            for x in o.x..o.x + CHUNK {
                let id = w.get(x, y, z);
                if id == AIR {
                    continue;
                }
                let def = blocks::block(id);
                let (fx, fy, fz) = (x as f32, y as f32, z as f32);
                match def.shape {
                    Shape::Air => {}
                    Shape::Water => {
                        let above = w.get(x, y + 1, z);
                        if above == WATER || blocks::is_opaque(above) {
                            continue;
                        }
                        let yy = fy + 0.88;
                        let rect = BlockAtlas::uv(atlas.tile("water"));
                        // each corner takes the depth of the four columns round it
                        // (past the world's edge is the open sea)
                        let depth = |cx: i32, cz: i32| {
                            if !Voxels::inside(cx, y, cz) {
                                return 8.0;
                            }
                            (0..8).take_while(|d| w.get(cx, y - d, cz) == WATER).count() as f32
                        };
                        let mut around = [[0.0f32; 3]; 3];
                        for (j, row) in around.iter_mut().enumerate() {
                            for (i, d) in row.iter_mut().enumerate() {
                                *d = depth(x + i as i32 - 1, z + j as i32 - 1);
                            }
                        }
                        let corner = |i: usize, j: usize| water_tint((around[j][i] + around[j][i + 1] + around[j + 1][i] + around[j + 1][i + 1]) / 4.0);
                        layers[2].quad_tinted(
                            [[fx, yy, fz + 1.0], [fx + 1.0, yy, fz + 1.0], [fx + 1.0, yy, fz], [fx, yy, fz]],
                            [0.0, 1.0, 0.0],
                            rect,
                            [corner(0, 1), corner(1, 1), corner(1, 0), corner(0, 0)],
                            false,
                        );
                        shore_foam(&mut layers[3], w, atlas, x, y, z, detail);
                    }
                    Shape::Model => model(&mut layers, w, atlas, x, y, z, id),
                    Shape::Cross => {
                        varies = true;
                        let rect = BlockAtlas::uv(atlas.tile(def.faces[1]));
                        // a little offset per block so a meadow isn't a grid
                        let h = hash2(x, z, 77);
                        let (ox, oz) = ((h - 0.5) * 0.3, (hash2(z, x, 78) - 0.5) * 0.3);
                        // half the width of each sheet, how tall, how many sheets
                        let (hw, tall, sheets, a0) = match id {
                            FERN => (0.72, 0.7 + h * 0.35, 3, h * PI),
                            BUSH => (0.82, 0.95 + h * 0.3, 3, h * PI),
                            _ => (0.636, 0.75 + h * 0.3, 2, FRAC_PI_4),
                        };
                        let (cx, cz) = (fx + 0.5 + ox, fz + 0.5 + oz);
                        // darker where the stems meet the ground
                        let shade = [0.7, 0.7, 1.0, 1.0];
                        if matches!(id, WHEAT | CARROTS) {
                            // crops grow in rows: four sheets in a #, square to the field
                            let tall = if id == WHEAT { 0.95 + h * 0.15 } else { 0.55 + h * 0.1 };
                            let up = [0.0, 1.0, 0.0];
                            for k in 0..4 {
                                let at = if k % 2 == 0 { 0.25 } else { 0.75 };
                                let (a, b) = if k < 2 { ([fx + at, fy, fz], [fx + at, fy, fz + 1.0]) } else { ([fx, fy, fz + at], [fx + 1.0, fy, fz + at]) };
                                let q = [a, b, [b[0], fy + tall, b[2]], [a[0], fy + tall, a[2]]];
                                layers[3].quad(q, up, rect, shade, false);
                                layers[3].sway_top(WIND * tall * 0.05);
                                layers[3].quad([q[1], q[0], q[3], q[2]], up, rect, shade, false);
                                layers[3].sway_top(WIND * tall * 0.05);
                            }
                            continue;
                        }
                        let sway = WIND * tall * if id == BUSH { 0.035 } else if id == FERN { 0.06 } else { 0.1 };
                        for k in 0..sheets {
                            let ang = a0 + k as f32 * PI / sheets as f32;
                            let (ax, az) = (ang.cos() * hw, ang.sin() * hw);
                            let a = [cx - ax, fy, cz - az];
                            let b = [cx + ax, fy, cz + az];
                            let up = [0.0, 1.0, 0.0];
                            let q = [a, b, [b[0], fy + tall, b[2]], [a[0], fy + tall, a[2]]];
                            layers[3].quad(q, up, rect, shade, false);
                            layers[3].sway_top(sway);
                            layers[3].quad([q[1], q[0], q[3], q[2]], up, rect, shade, false);
                            layers[3].sway_top(sway);
                        }
                    }
                    Shape::Solid | Shape::Cutout => {
                        // glass goes with the plants, which stay out of the ray
                        // traced scene: the tracer would count it solid
                        let layer = if def.light { 4 } else if def.shape == Shape::Solid { 0 } else if id == GLASS { 3 } else { 1 };
                        for f in &FACES {
                            let (nx, ny, nz) = (x + f.n[0], y + f.n[1], z + f.n[2]);
                            let nb = w.get(nx, ny, nz);
                            if blocks::is_opaque(nb) || (def.shape == Shape::Cutout && nb == id) {
                                continue;
                            }
                            let rect = BlockAtlas::uv(atlas.tile(def.faces[f.s]));
                            let mut light = [1.0; 4];
                            let mut occ = [0usize; 4];
                            let axes: Vec<usize> = (0..3).filter(|&i| f.n[i] == 0).collect();
                            for k in 0..4 {
                                let cc = f.c[k];
                                let step = |i: usize| if cc[i] == 1 { 1 } else { -1 };
                                let mut s1 = [nx, ny, nz];
                                s1[axes[0]] += step(axes[0]);
                                let mut s2 = [nx, ny, nz];
                                s2[axes[1]] += step(axes[1]);
                                let mut s3 = [nx, ny, nz];
                                s3[axes[0]] += step(axes[0]);
                                s3[axes[1]] += step(axes[1]);
                                let a = opaque(s1[0], s1[1], s1[2]) as usize;
                                let b = opaque(s2[0], s2[1], s2[2]) as usize;
                                let d = opaque(s3[0], s3[1], s3[2]) as usize;
                                occ[k] = if a == 1 && b == 1 { 3 } else { a + b + d };
                                if !def.light {
                                    light[k] = AO[occ[k]];
                                }
                            }
                            let corners = f.c.map(|cc| [fx + cc[0] as f32, fy + cc[1] as f32, fz + cc[2] as f32]);
                            let n = f.n.map(|v| v as f32);
                            let flip = occ[0] + occ[2] > occ[1] + occ[3];
                            // leaves take a little of the climate's tint
                            let leaf = matches!(id, LEAVES | BIRCH_LEAVES | PINE).then(|| leaf_tint(tints.at(fx + 0.5, fz + 0.5)));
                            if let (Some(t), true) = (leaf, nb == AIR && fins) {
                                leaf_fins(&mut layers[if cover.fin_shadows { layer } else { COVER }], corners, n, rect, light, t, hash2(x * 3 + f.n[0], y * 5 + f.n[1], z * 7 + f.n[2]));
                            }
                            if id == GRASS && f.s == 0 && nb != WATER {
                                // grass takes its colour from where it grows
                                let tint = corners.map(|p| grass_tint(tints.at(p[0], p[2]), p[0], p[2]));
                                layers[layer].quad_tinted(corners, n, rect, [0, 1, 2, 3].map(|k| rgba(tint[k] * light[k])), flip);
                            } else if nb == WATER {
                                // under water: the water above takes the red out first, then the green
                                let deep = (0..8).take_while(|d| w.get(nx, ny + d, nz) == WATER).count() as f32;
                                let t = UNDERWATER.map(|k| (-k * 2.0 * deep).exp());
                                let layer = if layer == 0 { UNDER } else { layer };
                                layers[layer].quad_tinted(corners, n, rect, light.map(|l| [l * t[0], l * t[1], l * t[2], 1.0]), flip);
                            } else if let Some(t) = leaf {
                                layers[layer].quad_tinted(corners, n, rect, light.map(|l| rgba(t * l)), flip);
                            } else {
                                layers[layer].quad(corners, n, rect, light, flip);
                            }
                        }
                        if id == GRASS {
                            ground_cover(&mut layers[COVER], w, atlas, x, y, z, cover, detail, &tints);
                        }
                        varies |= matches!(id, GRASS | LEAVES | BIRCH_LEAVES | PINE);
                    }
                }
            }
        }
    }
    if detail != Detail::Near || cover.off.wind {
        for l in &mut layers {
            l.sway.fill(Vec3::ZERO);
        }
    }
    if !cover.off.sky && layers.iter().enumerate().any(|(i, l)| i != 2 && !l.is_empty()) {
        Sky::new(w, o, detail).shade(&mut layers);
    }
    ChunkMesh { layers, varies }
}

#[derive(Resource)]
pub struct BlockMaterials {
    pub layers: [Handle<StandardMaterial>; 5],
    /// The ray tracer's stand-in canopy's (`porous_leaves`): plain leaf green.
    pub canopy: Handle<StandardMaterial>,
    /// The water layer's, which keeps its own normals (`render::WaterMaterial`).
    pub water: Handle<crate::render::WaterMaterial>,
}

/// How far chunks are drawn round the player: a square this many columns
/// each way, from the lighting tier. The island is drawn whole whenever
/// the player is on it or can see it, as it always was.
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub struct View(pub i32);

impl Default for View {
    fn default() -> Self {
        View(7)
    }
}

impl View {
    pub fn of(tier: crate::render::Tier) -> Self {
        use crate::render::Tier;
        View(match tier {
            Tier::Low => 5,
            Tier::Medium => 7,
            Tier::High => 8,
            Tier::Ultra => 9,
        })
    }

    /// How far it reaches, in blocks.
    pub fn blocks(self) -> f32 {
        (self.0 * CHUNK) as f32
    }

    /// Whether a column should be drawn with the player's eye at `focus`.
    pub fn wants(self, k: IVec2, focus: Vec3, slack: i32) -> bool {
        let f = crate::world::stream::column_of(focus);
        if (k - f).abs().max_element() <= self.0 + slack {
            return true;
        }
        // the island whole, from anywhere it can be seen
        let out = IVec2::new((-f.x).max(f.x - (crate::world::CX - 1)).max(0), (-f.y).max(f.y - (crate::world::CZ - 1)).max(0));
        crate::world::is_island(k) && out.max_element() <= self.0 + 1 + slack
    }
}

#[derive(Resource, Default)]
pub struct ChunkEntities {
    pub map: HashMap<IVec3, [Option<Entity>; LAYERS]>,
    /// The columns whose chunks are drawn (or waiting to be).
    pub shown: std::collections::HashSet<IVec2>,
    /// Counts changes to `shown`.
    pub shown_rev: u64,
    /// What `show` last looked at: the world, how many columns had come
    /// in, and the column the player was in.
    looked: (u64, u64, IVec2),
    /// How many chunks have been meshed since the world loaded.
    pub meshed: usize,
    /// The detail each chunk with grass, leaves or plants was meshed with.
    pub detail: HashMap<IVec3, Detail>,
    /// Each chunk's layers' sizes on the graphics card, for the memory
    /// report: vertices, and bytes of vertices, indices and wind targets.
    pub sizes: HashMap<IVec3, [[u64; 4]; LAYERS]>,
}

/// What each layer of a chunk holds, for the memory report.
pub const LAYER_NAMES: [&str; LAYERS] = ["solid blocks", "leaves and glass", "water", "plants", "glowing blocks", "ground cover", "under water"];

impl ChunkEntities {
    /// Every chunk's sizes added up, by layer (see `sizes`).
    pub fn totals(&self) -> [[u64; 4]; LAYERS] {
        let mut out = [[0; 4]; LAYERS];
        for layers in self.sizes.values() {
            for (o, l) in out.iter_mut().zip(layers) {
                for k in 0..4 {
                    o[k] += l[k];
                }
            }
        }
        out
    }
}

#[derive(Component)]
pub struct ChunkPart;

/// The point chunks are meshed outward from (the player's eye).
#[derive(Resource, Default)]
pub struct MeshFocus(pub Vec3);

/// Chooses which columns are drawn: those within the view of the player
/// whose neighbours are all here (so their edges and sky come out right),
/// and drops the meshes of those left behind.
pub fn show(mut commands: Commands, mut voxels: ResMut<Voxels>, focus: Res<MeshFocus>, view: Res<View>, mut chunks: ResMut<ChunkEntities>) {
    let at = crate::world::stream::column_of(focus.0);
    let now = (voxels.id, voxels.serial, at);
    if chunks.looked == now {
        return;
    }
    if chunks.looked.0 != voxels.id {
        // a new world: what was drawn was the old one's
        for (_, slots) in chunks.map.drain() {
            for e in slots.into_iter().flatten() {
                commands.entity(e).despawn();
            }
        }
        chunks.shown.clear();
        chunks.shown_rev += 1;
        chunks.detail.clear();
        chunks.sizes.clear();
        chunks.meshed = 0;
    }
    chunks.looked = now;
    let v = *view;
    // left behind, or let go
    let gone: Vec<IVec2> = chunks.shown.iter().copied().filter(|k| !voxels.has(*k) || !v.wants(*k, focus.0, 1)).collect();
    for k in &gone {
        chunks.shown.remove(k);
    }
    if !gone.is_empty() {
        chunks.shown_rev += 1;
        let drop: Vec<IVec3> = chunks.map.keys().copied().filter(|c| gone.contains(&IVec2::new(c.x, c.z))).collect();
        for c in drop {
            if let Some(slots) = chunks.map.remove(&c) {
                for e in slots.into_iter().flatten() {
                    commands.entity(e).despawn();
                }
            }
            chunks.detail.remove(&c);
            chunks.sizes.remove(&c);
        }
    }
    // newly in view, with everything round them here
    let r = v.0;
    let mut add: Vec<IVec2> = (-r..=r).flat_map(|dz| (-r..=r).map(move |dx| at + IVec2::new(dx, dz))).collect();
    if v.wants(IVec2::ZERO, focus.0, 0) {
        add.extend((0..crate::world::CZ).flat_map(|z| (0..crate::world::CX).map(move |x| IVec2::new(x, z))));
    }
    for k in add {
        if chunks.shown.contains(&k) || !voxels.has(k) {
            continue;
        }
        if (-1..=1).all(|dz| (-1..=1).all(|dx| voxels.has(k + IVec2::new(dx, dz)))) {
            chunks.shown.insert(k);
            chunks.shown_rev += 1;
            for y in crate::world::CY0..crate::world::CY1 {
                voxels.dirty.insert(IVec3::new(k.x, y, k.y));
            }
        }
    }
}

/// Meshes the dirty chunks nearest the player, several at once on the
/// task pool, a few per frame so the game never stalls.
pub fn remesh(
    mut commands: Commands,
    mut voxels: ResMut<Voxels>,
    atlas: Res<BlockAtlas>,
    mats: Res<BlockMaterials>,
    focus: Res<MeshFocus>,
    mut chunks: ResMut<ChunkEntities>,
    mut meshes: ResMut<Assets<Mesh>>,
    rt: Res<crate::render::Raytraced>,
    cover: Res<Cover>,
    wind: Option<Res<Wind>>,
) {
    // chunks the player has come near or left behind change their detail
    let changed: Vec<IVec3> = chunks.detail.iter().filter(|(c, d)| Detail::of(**c, focus.0, Some(**d)) != **d).map(|(c, _)| *c).collect();
    voxels.dirty.extend(changed);
    let f = (focus.0 / CHUNK as f32).floor().as_ivec3();
    if voxels.dirty.is_empty() {
        // then a few of those a change opened to the sky or roofed over,
        // nearest first, so placing a block never meshes a dozen at once
        let near = |c: &IVec3| {
            let d = *c - f;
            d.x * d.x + d.y * d.y * 2 + d.z * d.z
        };
        let mut shade: Vec<IVec3> = voxels.shade.iter().copied().collect();
        shade.sort_by_key(near);
        for c in shade.into_iter().take(3) {
            voxels.shade.remove(&c);
            voxels.dirty.insert(c);
        }
    }
    // only what's drawn; chunks with nothing in them to draw need no job
    let shown = &chunks.shown;
    let empty: Vec<IVec3> = voxels.dirty.iter().copied().filter(|c| !shown.contains(&IVec2::new(c.x, c.z)) || voxels.nothing_to_draw(*c)).collect();
    for c in empty {
        voxels.dirty.remove(&c);
        voxels.shade.remove(&c);
        if !chunks.shown.contains(&IVec2::new(c.x, c.z)) {
            continue;
        }
        if let Some(slots) = chunks.map.insert(c, [None; LAYERS]) {
            for e in slots.into_iter().flatten() {
                commands.entity(e).despawn();
            }
        }
        chunks.sizes.remove(&c);
        chunks.detail.remove(&c);
    }
    if voxels.dirty.is_empty() {
        return;
    }
    let mut list: Vec<IVec3> = voxels.dirty.iter().copied().collect();
    list.sort_by_key(|c| {
        let d = *c - f;
        d.x * d.x + d.y * d.y * 2 + d.z * d.z
    });
    // more per frame while the world first loads
    let budget = if chunks.meshed < 400 { 48 } else { 12 };
    list.truncate(budget);
    for c in &list {
        voxels.dirty.remove(c);
    }
    let w = &*voxels;
    let a = &*atlas;
    let cv = *cover;
    let jobs: Vec<(IVec3, Detail)> = list.iter().map(|&c| (c, Detail::of(c, focus.0, chunks.detail.get(&c).copied()))).collect();
    let porous = rt.0 && !cv.off.rtleaves && !cv.off.rtsolid;
    let done: Vec<(IVec3, Detail, ChunkMesh, Option<MeshData>)> = ComputeTaskPool::get().scope(|s| {
        for &(c, d) in &jobs {
            s.spawn(async move { (c, d, mesh_chunk(w, a, c, cv, d), porous.then(|| porous_leaves(w, c))) });
        }
    });
    for (c, detail, m, mut canopy) in done {
        chunks.meshed += 1;
        if m.varies {
            chunks.detail.insert(c, detail);
        } else {
            chunks.detail.remove(&c);
        }
        let mut sizes = [[0u64; 4]; LAYERS];
        let slot = chunks.map.entry(c).or_insert([None; LAYERS]);
        for (i, mut data) in m.layers.into_iter().enumerate() {
            if let Some(e) = slot[i].take() {
                commands.entity(e).despawn();
            }
            if data.is_empty() {
                continue;
            }
            let apart = matches!(i, 2 | 3 | COVER);
            if rt.0 && apart && i != 2 {
                data.face_normals();
            }
            let traced = rt.0 && !apart;
            let mut mesh = data.into_mesh(traced);
            if !traced && !cover.off.pack {
                pack(&mut mesh);
            }
            let sways = mesh.has_morph_targets();
            let verts = mesh.count_vertices() as u64;
            sizes[i] = [
                verts,
                verts * mesh.get_vertex_size(),
                mesh.indices().map_or(0, |x| x.len() as u64 * 4),
                mesh.morph_targets().map_or(0, |t| (t.len() * std::mem::size_of::<MorphAttributes>()) as u64),
            ];
            let mut e = commands.spawn((
                // water and plants stay out of the ray traced scene, so they keep their colours
                Mesh3d(meshes.add(mesh)),
                Transform::IDENTITY,
                ChunkPart,
            ));
            if i == 2 {
                e.insert(MeshMaterial3d(mats.water.clone()));
            } else {
                e.insert(MeshMaterial3d(mats.layers[LAYER_MATERIAL[i]].clone()));
            }
            if let (true, Some(w)) = (sways, &wind) {
                e.insert(MeshMorphWeights::Reference(w.0));
            }
            match i {
                2 => {
                    e.insert((bevy::light::NotShadowCaster, crate::render::NotRaytraced));
                }
                1 if cover.off.rtleaves => {
                    e.insert(crate::render::NotRaytraced);
                }
                // the tracer sees a porous stand-in in their place, so light
                // comes down through the canopy in patches
                1 if porous => {
                    e.insert(crate::render::NotRaytraced);
                    if let Some(stand_in) = canopy.take().filter(|d| !d.is_empty()) {
                        crate::render::trace_only(&mut e, meshes.add(stand_in.into_mesh(true)), mats.canopy.clone());
                    }
                }
                3 => {
                    e.insert(crate::render::NotRaytraced);
                }
                COVER => {
                    e.insert(crate::render::NotRaytraced);
                    // further out, the shadow cascades are too coarse for tufts to show
                    if !cover.shadows || detail != Detail::Near {
                        e.insert(bevy::light::NotShadowCaster);
                    }
                }
                _ => {}
            }
            slot[i] = Some(e.id());
        }
        chunks.sizes.insert(c, sizes);
    }
}
