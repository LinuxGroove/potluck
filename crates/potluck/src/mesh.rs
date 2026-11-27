//! Turning chunks of blocks into meshes: one face per side that can be seen,
//! with ambient occlusion at the corners.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::morph::{MeshMorphWeights, MorphAttributes, MorphWeights};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::tasks::ComputeTaskPool;

use std::f32::consts::{FRAC_PI_4, PI};

use crate::blocks::{self, BlockId, Shape, AIR, BIRCH_LEAVES, BUSH, CARROTS, FERN, GRASS, LEAVES, PINE, WATER, WHEAT};
use crate::noise::{fbm, hash2, smooth, vnoise};
use crate::textures::BlockAtlas;
use crate::world::{Voxels, CHUNK};

/// How thickly grass covers the ground: the share of grass blocks that get a
/// tuft where it's lush (fewer on the Steam Deck), and whether ground cover
/// near the player casts shadows. Set from the lighting tier at start.
#[derive(Resource, Clone, Copy)]
pub struct Cover {
    pub density: f32,
    pub shadows: bool,
    /// What's left out, for measuring what each costs:
    /// `POTLUCK_OFF=tufts,fins,lips,wind,lod` (lod: every chunk in full).
    pub off: Off,
}

#[derive(Clone, Copy, Default)]
pub struct Off {
    pub tufts: bool,
    pub fins: bool,
    pub lips: bool,
    pub wind: bool,
    pub lod: bool,
}

impl Off {
    fn from_env() -> Self {
        let v = std::env::var("POTLUCK_OFF").unwrap_or_default();
        let has = |w: &str| v.split(',').any(|o| o.trim() == w);
        Off { tufts: has("tufts"), fins: has("fins"), lips: has("lips"), wind: has("wind"), lod: has("lod") }
    }
}

impl Default for Cover {
    fn default() -> Self {
        Cover { density: 1.0, shadows: true, off: Off::default() }
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
        Cover { density, shadows, off: Off::from_env() }
    }
}

/// How much detail a chunk's meshes carry, by how far it is from the player.
/// Near, everything, swaying in the wind; further out, fewer, wider tufts
/// that keep still; furthest, no tufts or leaf fins. Out there each would be
/// a few pixels, and tiny triangles are what cards draw slowest.
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
/// island, with drier, yellower patches, and a little unevenness close up.
pub fn grass_tint(x: f32, z: f32) -> Vec3 {
    let dry = smooth(0.45, 0.72, fbm(x / 41.0, z / 41.0, 317, 3));
    Vec3::new(0.92, 1.04, 0.9).lerp(Vec3::new(1.16, 1.05, 0.64), dry) * (0.93 + 0.14 * vnoise(x / 5.0, z / 5.0, 319))
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

    /// The mesh, with corner shading in its vertex colours unless the world
    /// is ray traced (which shades corners itself, and takes only position,
    /// normal, UV and tangent).
    pub fn into_mesh(self, raytraced: bool) -> Mesh {
        let targets = self.wind_targets();
        let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD)
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.pos)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.nrm)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, self.uv);
        if !raytraced {
            m.insert_attribute(Mesh::ATTRIBUTE_COLOR, self.col);
        }
        if let Some(t) = targets {
            m.set_morph_targets(t);
        }
        m.with_inserted_attribute(Mesh::ATTRIBUTE_TANGENT, self.tan).with_inserted_indices(Indices::U32(self.idx))
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

/// How much of red, green and blue each block of water soaks up, on the way
/// down to what's under it and back up.
const UNDERWATER: [f32; 3] = [0.25, 0.06, 0.045];

/// Water's colour and how much it lets through, by how many blocks deep it
/// is: the shallows over sand are pale and clear, the deep is dark blue. The
/// open sea past the island (`render::spawn_sea`) is the deepest of these.
pub fn water_tint(depth: f32) -> [f32; 4] {
    let t = crate::noise::smooth(0.5, 6.0, depth);
    let c = Vec3::new(0.16, 0.62, 0.6).lerp(Vec3::new(0.008, 0.05, 0.09), t);
    [c.x, c.y, c.z, 0.38 + 0.57 * t]
}

/// Grass over a grass block: a lip hanging over each open side, so terraces
/// don't show a line of dirt, and a tuft on most tops, tinted lush or dry by
/// where it grows.
fn ground_cover(m: &mut MeshData, w: &Voxels, atlas: &BlockAtlas, x: i32, y: i32, z: i32, cover: Cover, detail: Detail) {
    let above = w.get(x, y + 1, z);
    if blocks::is_opaque(above) || above == WATER {
        return;
    }
    let (fx, fy, fz) = (x as f32, y as f32, z as f32);
    let tint = grass_tint(fx + 0.5, fz + 0.5);
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
                let l = if b.glow { 1.0 } else { down * (0.62 + 0.38 * p.y) };
                [l, l, l, 1.0]
            });
            layers[layer].quad_uv(local.map(|p| (o + p).into()), f.n.map(|v| v as f32), uvs, tint, false);
        }
    }
}

/// Leaf fins: two sheets of leaves standing out of a leaf block's open face,
/// crossed and turned at random, so a canopy's outline is ragged clumps of
/// leaves rather than the edge of a cube.
fn leaf_fins(m: &mut MeshData, corners: [[f32; 3]; 4], n: [f32; 3], rect: [f32; 4], light: [f32; 4], h: f32) {
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
        m.quad(q.map(|p| p.into()), side.into(), rect, light, false);
    }
}

/// A chunk's meshes, with as much detail as `detail` says; near the player,
/// its plants move in the wind.
pub fn mesh_chunk(w: &Voxels, atlas: &BlockAtlas, c: IVec3, cover: Cover, detail: Detail) -> ChunkMesh {
    let detail = if cover.off.lod { Detail::Near } else { detail };
    let fins = detail != Detail::Distant && !cover.off.fins;
    let mut varies = false;
    let mut layers: [MeshData; LAYERS] = Default::default();
    let o = c * CHUNK;
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
                        let layer = if def.light { 4 } else if def.shape == Shape::Solid { 0 } else { 1 };
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
                            if matches!(id, LEAVES | BIRCH_LEAVES | PINE) && nb == AIR && fins {
                                leaf_fins(&mut layers[layer], corners, n, rect, light, hash2(x * 3 + f.n[0], y * 5 + f.n[1], z * 7 + f.n[2]));
                            }
                            if id == GRASS && f.s == 0 && nb != WATER {
                                // grass takes its colour from where it grows
                                let tint = corners.map(|p| grass_tint(p[0], p[2]));
                                layers[layer].quad_tinted(corners, n, rect, [0, 1, 2, 3].map(|k| rgba(tint[k] * light[k])), flip);
                            } else if nb == WATER {
                                // under water: the water above takes the red out first, then the green
                                let deep = (0..8).take_while(|d| w.get(nx, ny + d, nz) == WATER).count() as f32;
                                let t = UNDERWATER.map(|k| (-k * 2.0 * deep).exp());
                                let layer = if layer == 0 { UNDER } else { layer };
                                layers[layer].quad_tinted(corners, n, rect, light.map(|l| [l * t[0], l * t[1], l * t[2], 1.0]), flip);
                            } else {
                                layers[layer].quad(corners, n, rect, light, flip);
                            }
                        }
                        if id == GRASS {
                            ground_cover(&mut layers[COVER], w, atlas, x, y, z, cover, detail);
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
    ChunkMesh { layers, varies }
}

#[derive(Resource)]
pub struct BlockMaterials {
    pub layers: [Handle<StandardMaterial>; 5],
}

#[derive(Resource, Default)]
pub struct ChunkEntities {
    pub map: HashMap<IVec3, [Option<Entity>; LAYERS]>,
    /// How many chunks have been meshed since the world loaded.
    pub meshed: usize,
    /// The detail each chunk with grass, leaves or plants was meshed with.
    pub detail: HashMap<IVec3, Detail>,
}

#[derive(Component)]
pub struct ChunkPart;

/// The point chunks are meshed outward from (the player's eye).
#[derive(Resource, Default)]
pub struct MeshFocus(pub Vec3);

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
    if voxels.dirty.is_empty() {
        return;
    }
    let f = (focus.0 / CHUNK as f32).floor().as_ivec3();
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
    let done: Vec<(IVec3, Detail, ChunkMesh)> = ComputeTaskPool::get().scope(|s| {
        for &(c, d) in &jobs {
            s.spawn(async move { (c, d, mesh_chunk(w, a, c, cv, d)) });
        }
    });
    for (c, detail, m) in done {
        chunks.meshed += 1;
        if m.varies {
            chunks.detail.insert(c, detail);
        } else {
            chunks.detail.remove(&c);
        }
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
            let mesh = data.into_mesh(rt.0 && !apart);
            let sways = mesh.has_morph_targets();
            let mut e = commands.spawn((
                // water and plants stay out of the ray traced scene, so they keep their colours
                Mesh3d(meshes.add(mesh)),
                MeshMaterial3d(mats.layers[LAYER_MATERIAL[i]].clone()),
                Transform::IDENTITY,
                ChunkPart,
            ));
            if let (true, Some(w)) = (sways, &wind) {
                e.insert(MeshMorphWeights::Reference(w.0));
            }
            match i {
                2 => {
                    e.insert((bevy::light::NotShadowCaster, crate::render::NotRaytraced));
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
    }
}
