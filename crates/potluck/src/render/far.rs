//! The land past the drawn columns, out to the far end of the view: the
//! ground the seed makes there (`world::wild::height`) on a grid a few
//! blocks apart, coloured by what's on top of it (grass tinted by the
//! climate, sand, snow, bare rock), with woods standing up as a darker
//! canopy, and the sea over it drawn as the near water is. It fills every
//! column that isn't drawn, so hills, woods and snowfields show on the
//! horizon long before you reach them, rather than land rising out of a
//! flat sea as it comes in. Toward the far end it sinks into the deep sea,
//! so the view ends at a dark horizon rather than at a cut through a hill.
//!
//! It's made again on the task pool as the player moves and columns are
//! drawn (`follow`), keeping what it worked out for each point of the grid,
//! so only new ground costs anything.

use std::collections::{HashMap, HashSet};

use bevy::prelude::*;
use bevy::tasks::{block_on, poll_once, AsyncComputeTaskPool, Task};

use super::{NotRaytraced, Tier, WaterMaterial};
use crate::blocks::{self, BlockId};
use crate::noise::{hash2, smooth};
use crate::world::{wild, Biome, Climate, CHUNK, CY0, CY1, SEA};

/// Blocks between the grid's points.
pub const STEP: i32 = 8;
/// Steps along a column's side.
const PER: i32 = CHUNK / STEP;
/// The water's top, as drawn (`mesh::mesh_chunk`).
const WATER_TOP: f32 = SEA as f32 + 0.88;
/// Where land sinks to at the far end of the view: deep enough that the
/// water over it is the open sea's darkest.
const SUNK: f32 = SEA as f32 - 12.0;
/// How far skirts hang under the edge next to drawn columns, so a far
/// hill a little higher than the blocks beside it shows no gap under it.
pub const SKIRT: f32 = 24.0;

/// How far the far land reaches from the player, in blocks: inside the
/// camera's far end (900) and Low's sky dome (870), and on Low no further
/// than its fog lets anything show.
pub fn reach(tier: Tier) -> f32 {
    if tier == Tier::Low { 360.0 } else { 830.0 }
}

/// How far the open sea reaches past the land on Medium and up, so it
/// meets the sky at the horizon rather than ending in a dark band (the
/// planet's own ground, under the atmosphere). The camera draws without a
/// far end (only culling uses it), and the sea is never culled.
const OPEN_SEA: f32 = 12_000.0;

/// What the grid knows of one point: worked out once from the seed.
#[derive(Clone, Copy, Debug)]
pub struct Spot {
    /// The top of the ground there (its top block's top face).
    pub ground: f32,
    pub biome: Biome,
    pub climate: Climate,
    /// How much trees cover, how high their canopy stands and their leaves
    /// (`wild::tree_cover`).
    pub trees: (f32, f32, [f32; 3]),
}

impl Spot {
    pub fn at(seed: u32, x: i32, z: i32) -> Spot {
        let (fx, fz) = (x as f32, z as f32);
        let h = wild::height(seed, fx, fz);
        let biome = wild::biome_with(seed, fx, fz, h.floor() as i32);
        // the island's own trees are planted, not from the wild's chances;
        // its meadows' are near enough from afar
        let woods = if biome == Biome::Home && h > (SEA + 1) as f32 { Biome::Meadow } else { biome };
        Spot { ground: h.floor() + 1.0, biome, climate: wild::climate(seed, fx, fz), trees: wild::tree_cover(seed, fx, fz, woods) }
    }

    /// How high the canopy (or the ground, where there are no trees) stands,
    /// a little uneven from point to point so woods look lumpy from afar.
    fn top(&self, g: IVec2) -> f32 {
        let (cover, tall, _) = self.trees;
        if cover <= 0.0 {
            return self.ground;
        }
        self.ground + tall * smooth(0.05, 0.55, cover) * (0.55 + 0.6 * hash2(g.x, g.y, 4421))
    }
}

/// The colours of the blocks far land is made of (linear), from the map's
/// palette (the blocks' own icons, seen from above).
#[derive(Clone, Copy)]
pub struct Colours {
    grass: Vec3,
    stone: Vec3,
    sand: Vec3,
    gravel: Vec3,
    snow: Vec3,
    leaves: [Vec3; 3],
    /// How far into autumn and winter the land is, as the drawn chunks have
    /// it (`mesh::Cover`).
    pub autumn: f32,
    pub winter: f32,
}

impl Colours {
    pub fn from_palette(p: &crate::map::Palette) -> Colours {
        let c = |b: BlockId| {
            let [r, g, b] = p.0[b as usize];
            let l = Color::srgb_u8(r, g, b).to_linear();
            Vec3::new(l.red, l.green, l.blue)
        };
        Colours {
            grass: c(blocks::GRASS),
            stone: c(blocks::STONE),
            sand: c(blocks::SAND),
            gravel: c(blocks::GRAVEL),
            snow: c(blocks::SNOW),
            leaves: [c(blocks::LEAVES), c(blocks::BIRCH_LEAVES), c(blocks::PINE)],
            autumn: 0.0,
            winter: 0.0,
        }
    }

    /// The colour of the top of the ground at grid point `g`, as
    /// `world::wild::lay` (and the island's own) would make it, with the
    /// slope (blocks a block) for where it's too steep for grass.
    fn ground(&self, s: &Spot, g: IVec2, slope: f32) -> Vec3 {
        let h = s.ground as i32 - 1;
        let steep = smooth(0.9, 1.6, slope);
        let (x, z) = ((g.x * STEP) as f32, (g.y * STEP) as f32);
        let grass = self.grass * crate::mesh::grass_tint(crate::mesh::climate_tint(s.climate), x, z) * crate::mesh::season_grass(self.autumn, self.winter);
        // snow lies on the grass as it does on the drawn grass
        let grass = if crate::mesh::snow_lies(g.x * STEP, g.y * STEP, self.winter) { self.snow } else { grass };
        let green = grass.lerp(self.stone, steep);
        match s.biome {
            Biome::Home => {
                if h <= SEA + 1 {
                    self.sand
                } else if h > 74 {
                    self.snow
                } else if h > 62 {
                    self.stone
                } else {
                    green
                }
            }
            Biome::Sea => {
                if h < SEA - 12 {
                    self.gravel
                } else {
                    self.sand
                }
            }
            Biome::Beach | Biome::Desert => self.sand,
            Biome::Mountain => {
                if h > SEA + 58 {
                    self.snow.lerp(self.stone, steep * 0.6)
                } else if h < SEA + 46 {
                    green
                } else {
                    self.stone
                }
            }
            Biome::Snowfield => self.snow.lerp(self.stone, steep),
            _ => green,
        }
    }

    /// The canopy's colour, from its mix of leaves, tinted by the climate as
    /// drawn leaves are (`mesh::leaf_tint`).
    fn canopy(&self, s: &Spot, g: IVec2) -> Vec3 {
        let [o, b, p] = s.trees.2;
        // the season's leaves (`mesh::leaf_tile`): an oak's turning and then
        // brown, a birch's gold, with snow over them all in winter
        let turned = self.autumn.max(self.winter) * 0.92;
        let fall = Vec3::new(0.36, 0.12, 0.02).lerp(Vec3::new(0.12, 0.06, 0.02), self.winter / (self.autumn + self.winter).max(1e-3));
        let oak = self.leaves[0].lerp(fall, turned);
        let birch = self.leaves[1].lerp(Vec3::new(0.45, 0.3, 0.03), turned);
        let leaves = (oak * o + birch * b + self.leaves[2] * p) * crate::mesh::leaf_tint(crate::mesh::climate_tint(s.climate));
        if crate::mesh::snow_lies(g.x * STEP, g.y * STEP, self.winter) {
            leaves.lerp(self.snow, 0.6)
        } else {
            leaves
        }
    }
}

/// One go at making the far land: everything it needs, moved to the task
/// pool, and the grid's points back with the meshes.
pub struct Job {
    pub seed: u32,
    /// Where the player is (x and z), to the grid's step.
    pub at: Vec2,
    pub reach: f32,
    /// How far the open sea goes on past the land, to meet the sky at the
    /// horizon (0 for none).
    pub sea: f32,
    /// The columns that are drawn, which the far land leaves holes for.
    pub drawn: HashSet<IVec2>,
    pub spots: HashMap<IVec2, Spot>,
    pub colours: Colours,
}

pub struct Built {
    pub land: Mesh,
    pub water: Mesh,
    pub spots: HashMap<IVec2, Spot>,
}

#[derive(Default)]
struct Sheet {
    pos: Vec<[f32; 3]>,
    nrm: Vec<[f32; 3]>,
    col: Vec<[f32; 4]>,
    idx: Vec<u32>,
}

impl Sheet {
    fn vertex(&mut self, p: Vec3, n: Vec3, c: [f32; 4]) -> u32 {
        self.pos.push(p.into());
        self.nrm.push(n.into());
        self.col.push(c);
        self.pos.len() as u32 - 1
    }

    /// A quad of four vertices already added, wound to face `n`.
    fn quad(&mut self, v: [u32; 4], n: Vec3) {
        let p = |i: u32| Vec3::from(self.pos[i as usize]);
        let faces = (p(v[1]) - p(v[0])).cross(p(v[2]) - p(v[0])).dot(n) > 0.0;
        if faces {
            self.idx.extend([v[0], v[1], v[2], v[0], v[2], v[3]]);
        } else {
            self.idx.extend([v[0], v[2], v[1], v[0], v[3], v[2]]);
        }
    }

    fn into_mesh(self) -> Mesh {
        Mesh::new(bevy::mesh::PrimitiveTopology::TriangleList, bevy::asset::RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.pos)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.nrm)
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.col)
            .with_inserted_indices(bevy::mesh::Indices::U32(self.idx))
    }
}

/// How many blocks of water there are over the ground at `ground`, as the
/// drawn water counts them (up to eight).
fn water_over(ground: f32) -> f32 {
    (SEA as f32 + 1.0 - ground).clamp(0.0, 8.0)
}

impl Job {
    /// Makes the far land round `at`: for each column in reach that isn't
    /// drawn, the ground (or canopy) on the grid, with skirts down its sides
    /// next to drawn columns, and the water over it where any of it is
    /// under the sea or it's sinking toward the far end.
    pub fn build(mut self) -> Built {
        let (seed, at, reach) = (self.seed, self.at, self.reach);
        let band = reach * 0.18;
        let c = CHUNK as f32;
        let k0 = ((at - reach) / c).floor().as_ivec2();
        let k1 = ((at + reach) / c).floor().as_ivec2();
        // how much land is left standing at a spot, 1 near and 0 at the far end
        let stand = |p: Vec2| smooth(reach, reach - band, p.distance(at));
        let mut land = Sheet::default();
        let mut water = Sheet::default();
        let mut made = HashSet::new();
        let spots = &mut self.spots;
        let mut spot = |g: IVec2| *spots.entry(g).or_insert_with(|| Spot::at(seed, g.x * STEP, g.y * STEP));
        let n = (PER + 1) as usize;
        for kz in k0.y..=k1.y {
            for kx in k0.x..=k1.x {
                let k = IVec2::new(kx, kz);
                if self.drawn.contains(&k) {
                    continue;
                }
                let lo = Vec2::new(kx as f32 * c, kz as f32 * c);
                let nearest = at.clamp(lo, lo + c);
                if nearest.distance(at) > reach {
                    continue;
                }
                made.insert(k);
                let g0 = k * PER;
                // heights round the column's points, a point further each
                // way for the slopes at its edges
                let mut top = [[0.0f32; PER as usize + 3]; PER as usize + 3];
                let mut here = [[None::<Spot>; PER as usize + 1]; PER as usize + 1];
                for (j, row) in top.iter_mut().enumerate() {
                    for (i, t) in row.iter_mut().enumerate() {
                        let g = g0 + IVec2::new(i as i32 - 1, j as i32 - 1);
                        let s = spot(g);
                        let f = stand((g * STEP).as_vec2());
                        *t = SUNK + (s.top(g) - SUNK) * f;
                        if (1..=n).contains(&i) && (1..=n).contains(&j) {
                            here[j - 1][i - 1] = Some(s);
                        }
                    }
                }
                let mut ids = [[0u32; PER as usize + 1]; PER as usize + 1];
                let mut wet = false;
                let mut sinking = false;
                for j in 0..n {
                    for i in 0..n {
                        let s = here[j][i].unwrap();
                        let g = g0 + IVec2::new(i as i32, j as i32);
                        let p = (g * STEP).as_vec2();
                        let y = top[j + 1][i + 1];
                        let (dx, dz) = (top[j + 1][i + 2] - top[j + 1][i], top[j + 2][i + 1] - top[j][i + 1]);
                        let normal = Vec3::new(-dx, 2.0 * STEP as f32, -dz).normalize();
                        let slope = Vec2::new(dx, dz).length() / (2.0 * STEP as f32);
                        let f = stand(p);
                        sinking |= f < 1.0;
                        let ground = SUNK + (s.ground - SUNK) * f;
                        let mut colour = self.colours.ground(&s, g, slope);
                        let (cover, _, _) = s.trees;
                        if cover > 0.0 {
                            // crowns in their own shade, mottled from point to point
                            let leaves = self.colours.canopy(&s, g) * (0.56 + 0.28 * hash2(g.x, g.y, 4423));
                            colour = colour.lerp(leaves, smooth(0.0, 0.3, cover));
                        }
                        let deep = water_over(ground);
                        if deep > 0.0 {
                            // the water over it takes the red out first, as near the shore
                            wet = true;
                            let t = crate::mesh::UNDERWATER.map(|k| (-k * 2.0 * deep).exp());
                            colour *= Vec3::from(t);
                        }
                        ids[j][i] = land.vertex(Vec3::new(p.x, y, p.y), normal, [colour.x, colour.y, colour.z, 1.0]);
                    }
                }
                for j in 0..PER as usize {
                    for i in 0..PER as usize {
                        land.quad([ids[j][i], ids[j][i + 1], ids[j + 1][i + 1], ids[j + 1][i]], Vec3::Y);
                    }
                }
                // skirts along the sides next to drawn columns, facing them
                for (side, out) in [(0, IVec2::NEG_Y), (1, IVec2::X), (2, IVec2::Y), (3, IVec2::NEG_X)] {
                    if !self.drawn.contains(&(k + out)) {
                        continue;
                    }
                    let edge: Vec<(usize, usize)> = (0..n)
                        .map(|t| match side {
                            0 => (t, 0),
                            1 => (PER as usize, t),
                            2 => (t, PER as usize),
                            _ => (0, t),
                        })
                        .collect();
                    let facing = out.as_vec2().extend(0.0).xzy();
                    for w in edge.windows(2) {
                        let mut v = [0u32; 4];
                        for (slot, ((i, j), drop)) in [(w[0], 0.0), (w[1], 0.0), (w[1], SKIRT), (w[0], SKIRT)].into_iter().enumerate() {
                            let p = Vec3::from(land.pos[ids[j][i] as usize]) - Vec3::Y * drop;
                            let c = land.col[ids[j][i] as usize];
                            v[slot] = land.vertex(p, facing, [c[0] * 0.6, c[1] * 0.6, c[2] * 0.6, 1.0]);
                        }
                        land.quad(v, facing);
                    }
                }
                if !wet && !sinking {
                    continue;
                }
                // the water over it, coloured as the drawn water is by how deep it is
                let mut wids = [[0u32; PER as usize + 1]; PER as usize + 1];
                for j in 0..n {
                    for i in 0..n {
                        let s = here[j][i].unwrap();
                        let g = g0 + IVec2::new(i as i32, j as i32);
                        let p = (g * STEP).as_vec2();
                        let ground = SUNK + (s.ground - SUNK) * stand(p);
                        wids[j][i] = water.vertex(Vec3::new(p.x, WATER_TOP, p.y), Vec3::Y, crate::mesh::water_tint(water_over(ground)));
                    }
                }
                for j in 0..PER as usize {
                    for i in 0..PER as usize {
                        water.quad([wids[j][i], wids[j][i + 1], wids[j + 1][i + 1], wids[j + 1][i]], Vec3::Y);
                    }
                }
            }
        }
        // past the land, the open sea out to near the horizon, a strip for
        // each run of columns along a row that nothing else covers
        if self.sea > 0.0 {
            let r = (self.sea / c).ceil() as i32;
            let mid = (at / c).floor().as_ivec2();
            let colour = crate::mesh::water_tint(8.0);
            let covered = |k: IVec2| made.contains(&k) || self.drawn.contains(&k);
            for dz in -r..=r {
                let z = (mid.y + dz) as f32 * c;
                // the row's reach across the circle
                let half = (self.sea * self.sea - (z + c / 2.0 - at.y).powi(2)).max(0.0).sqrt();
                if half <= 0.0 {
                    continue;
                }
                let (from, to) = (((at.x - half) / c).floor() as i32, ((at.x + half) / c).floor() as i32);
                let mut kx = from;
                while kx <= to {
                    if covered(IVec2::new(kx, mid.y + dz)) {
                        kx += 1;
                        continue;
                    }
                    let start = kx;
                    while kx <= to && !covered(IVec2::new(kx, mid.y + dz)) {
                        kx += 1;
                    }
                    let (x0, x1) = (start as f32 * c, kx as f32 * c);
                    let v = [(x0, z), (x1, z), (x1, z + c), (x0, z + c)].map(|(x, z)| water.vertex(Vec3::new(x, WATER_TOP, z), Vec3::Y, colour));
                    water.quad(v, Vec3::Y);
                }
            }
        }
        // forget ground well behind, so a long trip doesn't keep it all
        let keep = (reach + 2.0 * band + 4.0 * c) / STEP as f32;
        let mid = at / STEP as f32;
        self.spots.retain(|g, _| g.as_vec2().distance(mid) <= keep);
        Built { land: land.into_mesh(), water: water.into_mesh(), spots: self.spots }
    }
}

#[derive(Component)]
pub struct FarLand;

#[derive(Component)]
pub struct FarWater;

/// The far land's state: what it was last made for, what's being made, and
/// the grid's points while no job has them.
#[derive(Resource, Default)]
pub struct Far {
    world: u64,
    /// The columns counted as drawn: shown, and meshed since (they stay so
    /// while shown, so a block changed in one never opens the far land
    /// under it for a frame).
    drawn: HashSet<IVec2>,
    drawn_rev: u64,
    looked: Option<(u64, u64, IVec2, u32, (u32, u32))>,
    spots: HashMap<IVec2, Spot>,
    task: Option<Task<Built>>,
}

pub fn spawn(commands: &mut Commands, mats: &mut Assets<StandardMaterial>, waters: &mut Assets<WaterMaterial>) {
    let ground = mats.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.92, reflectance: 0.3, ..default() });
    // the drawn water's material without its bumps, which are far too
    // small to see out here (and they'd need the atlas's UVs)
    let sea = waters.add(WaterMaterial {
        base: StandardMaterial {
            base_color: Color::WHITE,
            alpha_mode: AlphaMode::Blend,
            perceptual_roughness: 0.07,
            reflectance: 0.35,
            double_sided: true,
            cull_mode: None,
            ..default()
        },
        extension: super::OwnNormals {},
    });
    commands.insert_resource(Far::default());
    commands.spawn((
        // its mesh comes with the first go (`follow`)
        MeshMaterial3d(ground),
        Transform::IDENTITY,
        bevy::light::NotShadowCaster,
        bevy::camera::visibility::NoFrustumCulling,
        NotRaytraced,
        FarLand,
    ));
    commands.spawn((
        MeshMaterial3d(sea),
        Transform::IDENTITY,
        bevy::light::NotShadowCaster,
        bevy::camera::visibility::NoFrustumCulling,
        NotRaytraced,
        FarWater,
    ));
}

/// Gives a far sheet its new mesh, or takes it away when there's nothing
/// in it (an empty mesh only earns a warning).
fn show(commands: &mut Commands, meshes: &mut Assets<Mesh>, (e, had): (Entity, Option<&Mesh3d>), mesh: Mesh) {
    if mesh.count_vertices() == 0 {
        if had.is_some() {
            commands.entity(e).remove::<Mesh3d>();
        }
        return;
    }
    if let Some(h) = had.filter(|m| meshes.contains(&m.0)) {
        if let Some(mut m) = meshes.get_mut(&h.0) {
            *m = mesh;
        }
    } else {
        commands.entity(e).insert(Mesh3d(meshes.add(mesh)));
    }
}

/// Keeps the far land round the player: counts columns as drawn once
/// they're meshed, and makes it again on the task pool when the player has
/// moved a step of the grid, a column is drawn or let go, or the view
/// changes; one job at a time.
#[allow(clippy::too_many_arguments)]
pub fn follow(
    mut far: ResMut<Far>,
    voxels: Res<crate::world::Voxels>,
    chunks: Res<crate::mesh::ChunkEntities>,
    focus: Res<crate::mesh::MeshFocus>,
    settings: Res<crate::settings::Settings>,
    map: Res<crate::map::MapImage>,
    cover: Res<crate::mesh::Cover>,
    land: Query<(Entity, Option<&Mesh3d>), (With<FarLand>, Without<FarWater>)>,
    water: Query<(Entity, Option<&Mesh3d>), (With<FarWater>, Without<FarLand>)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut commands: Commands,
) {
    let far = &mut *far;
    if far.world != voxels.id {
        // a new world: nothing of the old one's ground is any use (and
        // dropping the job cancels it)
        *far = Far { world: voxels.id, ..default() };
    }
    if let Some(task) = far.task.as_mut() {
        let Some(built) = block_on(poll_once(task)) else { return };
        far.task = None;
        far.spots = built.spots;
        if let (Ok(l), Ok(w)) = (land.single(), water.single()) {
            show(&mut commands, &mut meshes, l, built.land);
            show(&mut commands, &mut meshes, w, built.water);
        }
    }
    let before = far.drawn.len();
    far.drawn.retain(|k| chunks.shown.contains(k));
    let mut changed = far.drawn.len() != before;
    if far.drawn.len() < chunks.shown.len() {
        for k in &chunks.shown {
            if !far.drawn.contains(k) && (CY0..CY1).all(|y| !voxels.dirty.contains(&IVec3::new(k.x, y, k.y))) {
                far.drawn.insert(*k);
                changed = true;
            }
        }
    }
    if changed {
        far.drawn_rev += 1;
    }
    let tier = Tier::of(&settings.lighting);
    let at = (Vec2::new(focus.0.x, focus.0.z) / STEP as f32).floor().as_ivec2();
    let now = (voxels.id, far.drawn_rev, at, reach(tier).to_bits(), (cover.autumn.to_bits(), cover.winter.to_bits()));
    if far.looked == Some(now) {
        return;
    }
    far.looked = Some(now);
    let job = Job {
        seed: voxels.seed,
        at: (at * STEP).as_vec2() + STEP as f32 / 2.0,
        reach: reach(tier),
        // Low's fog has hidden everything long before
        sea: if tier == Tier::Low { 0.0 } else { OPEN_SEA },
        drawn: far.drawn.clone(),
        spots: std::mem::take(&mut far.spots),
        colours: Colours { autumn: cover.autumn, winter: cover.winter, ..Colours::from_palette(&map.palette) },
    };
    far.task = Some(AsyncComputeTaskPool::get().spawn(async move { job.build() }));
}
