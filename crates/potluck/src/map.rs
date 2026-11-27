//! The map: the world from above, drawn block by block from what's really
//! there and kept up to date as blocks change. It shows as a minimap in the
//! corner and as a full-screen map with waypoints, friends and places
//! shared in chat. Under a roof or underground both show the level you're
//! on, like a plan of the rooms and tunnels round you.
//!
//! Nothing here moves the camera: the map opens and closes at once, and
//! going somewhere fades the screen (`waypoints::Travel`).

use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::window::PrimaryWindow;

use crate::blocks::{self, BlockId, Shape, WATER};
use crate::chat::Chat;
use crate::creatures::Herd;
use crate::input::Controls;
use crate::net::Net;
use crate::player::Me;
use crate::settings::Settings;
use crate::ui::{Act, Acts, Fonts, Focus, Screen, Toasts, WorldInfo, DIM, INK};
use crate::waypoints::{self, Goal, Kind, Travel, Waypoints};
use crate::world::{Voxels, CHUNK, MIN_Y, SEA, WX, WY, WZ};

/// The sea past the island's edge.
pub const SEA_COLOR: [u8; 4] = [26, 58, 98, 255];
/// Rock round a tunnel, when the map shows the level you're on.
const ROCK: [u8; 3] = [34, 32, 36];

/// Each block's colour seen from above.
#[derive(Clone)]
pub struct Palette(pub Vec<[u8; 3]>);

impl Default for Palette {
    /// Colours for the main blocks, until the textures give their own (and
    /// on servers and in tests, which have none).
    fn default() -> Self {
        use crate::blocks::*;
        let mut p = vec![[128, 128, 128]; 256];
        for (b, c) in [
            (GRASS, [96, 142, 62]),
            (DIRT, [122, 88, 58]),
            (STONE, [124, 124, 126]),
            (SAND, [219, 204, 152]),
            (GRAVEL, [132, 126, 120]),
            (LOG, [112, 82, 52]),
            (LEAVES, [58, 108, 46]),
            (PLANKS, [176, 136, 86]),
            (MOSSY, [96, 116, 82]),
            (SNOW, [238, 242, 248]),
            (WOOL_RED, [190, 52, 52]),
            (WOOL_CREAM, [234, 224, 196]),
            (HAY, [212, 176, 72]),
            (PINE, [44, 84, 56]),
            (WOOL_BLUE, [56, 86, 170]),
            (WOOL_YELLOW, [226, 196, 62]),
            (WATER, [48, 98, 170]),
            (LANTERN, [255, 204, 112]),
            (BIRCH, [212, 206, 190]),
            (BIRCH_LEAVES, [112, 152, 72]),
            (COBBLE, [116, 116, 112]),
            (GLASS, [182, 212, 222]),
            (BEDROCK, [58, 58, 60]),
            (BARREL, [132, 96, 60]),
            (CRATE, [162, 122, 72]),
            (BOOKSHELF, [150, 110, 70]),
            (FARMLAND, [96, 66, 42]),
            (TABLE, [172, 132, 82]),
            (STOOL, [172, 132, 82]),
            (SANDSTONE, [216, 190, 136]),
            (SANDSTONE_BRICKS, [206, 180, 126]),
            (COLUMN, [222, 216, 202]),
            (BRAZIER, [92, 82, 72]),
            (FENCE, [160, 120, 76]),
        ] {
            p[b as usize] = c;
        }
        Palette(p)
    }
}

impl Palette {
    /// Each block's top, from its texture: the average of the parts that
    /// aren't see-through. Icons are only on this side until the first
    /// frame is drawn, so this runs at startup.
    pub fn from_icons(atlas: &crate::textures::BlockAtlas, images: &Assets<Image>) -> Palette {
        let mut p = Palette::default();
        for (i, b) in blocks::BLOCKS.iter().enumerate() {
            // water reads best as the map's own blue
            if b.shape == Shape::Water || b.shape == Shape::Air {
                continue;
            }
            let Some(data) = atlas.icons.get(b.faces[0]).and_then(|h| images.get(h)).and_then(|img| img.data.as_ref()) else { continue };
            let (mut sum, mut n) = ([0f32; 3], 0f32);
            for px in data.chunks_exact(4) {
                if px[3] > 127 {
                    for k in 0..3 {
                        sum[k] += px[k] as f32;
                    }
                    n += 1.0;
                }
            }
            if n > 0.0 {
                // leaves from above are mostly shade
                let dim = if b.shape == Shape::Cutout && b.name.contains("eaves") || b.name.contains("needles") { 0.82 } else { 1.0 };
                p.0[i] = sum.map(|s| (s / n * dim).round().clamp(0.0, 255.0) as u8);
            }
        }
        p
    }
}

/// What's seen of one column from above.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Column {
    pub block: BlockId,
    /// The height of the block seen (under any water).
    pub y: i32,
    /// How deep the water over it is.
    pub water: i32,
}

impl Column {
    /// The height you'd stand at: the water's top, or the block's.
    pub fn top(&self) -> i32 {
        self.y + self.water
    }
}

/// The first thing seen looking down a column from `from`: plants and air
/// are looked through, water is counted.
pub fn surface(v: &Voxels, x: i32, z: i32, from: i32) -> Column {
    let mut water = 0;
    let mut y = from.min(WY - 1);
    while y >= MIN_Y {
        let b = v.get(x, y, z);
        match blocks::shape(b) {
            Shape::Air | Shape::Cross => {}
            Shape::Water => water += 1,
            _ => return Column { block: b, y, water },
        }
        y -= 1;
    }
    Column { block: blocks::BEDROCK, y: MIN_Y, water }
}

fn mix(a: [u8; 3], b: [u8; 3], t: f32) -> [u8; 3] {
    [0, 1, 2].map(|i| (a[i] as f32 * (1.0 - t) + b[i] as f32 * t).round() as u8)
}

fn scale(c: [u8; 3], f: f32) -> [u8; 3] {
    c.map(|v| (v as f32 * f).round().clamp(0.0, 255.0) as u8)
}

/// A column's colour: its block, lit by the slope (lighter facing north,
/// as the sun is drawn on maps), with water over it bluer the deeper it is.
pub fn shade(p: &Palette, c: Column, north: i32) -> [u8; 4] {
    let base = p.0[c.block as usize];
    let rgb = if c.water > 0 {
        let deep = (c.water as f32 / 7.0).min(1.0);
        let water = p.0[WATER as usize];
        scale(mix(scale(base, 0.8), water, 0.55 + deep * 0.4), 1.0 - deep * 0.25)
    } else {
        let d = (c.top() - north).clamp(-3, 3) as f32;
        // higher ground a touch brighter, so hills read from above
        let height = 1.0 + (c.y - crate::world::SEA) as f32 / 240.0;
        scale(base, (1.0 + d * 0.085) * height)
    };
    [rgb[0], rgb[1], rgb[2], 255]
}

/// The map's window onto the world, in blocks each way: what the picture
/// holds, which the full map shows and the minimap draws from. It moves
/// with the player (or the full map's middle), and every column ever drawn
/// is kept, so coming back shows it again.
pub const SIZE: i32 = 768;
/// Land nobody has seen yet.
pub const UNSEEN: [u8; 4] = [20, 24, 32, 255];
/// The most columns kept (about 6 KB each); the furthest from the player
/// go past it.
const MAX_TILES: usize = 12_000;
/// How far round the player the level under a roof is drawn (in blocks);
/// further out it's the map from above, darkened.
const LEVEL_REACH: i32 = 192;

/// One column of the map, as it was last seen.
struct Tile {
    colors: Vec<[u8; 4]>,
    heights: Vec<i16>,
}

/// The map of one world: a colour for every column seen, from above or at
/// one level, and the picture of the part round the player the screen
/// draws.
#[derive(Resource)]
pub struct MapImage {
    pub palette: Palette,
    tiles: std::collections::HashMap<IVec2, Tile>,
    /// The window's north-west corner, in blocks (a column's corner).
    pub origin: IVec2,
    /// The window from above, row by row from its north-west corner.
    pub colors: Vec<[u8; 4]>,
    /// Each column's height from above, for slopes and the cursor.
    pub heights: Vec<i16>,
    /// The level the map shows instead, under a roof or underground: its
    /// height, where it was drawn round, and the window's colours there.
    pub level: Option<(i32, IVec2, Vec<[u8; 4]>)>,
    /// The picture the screen draws, when there's a screen.
    pub image: Option<Handle<Image>>,
    /// Which world it's of (`Voxels::id`).
    world: u64,
    /// The columns drawn so far (`Voxels::serial`), and those that came
    /// in and wait to be (a few each time, so streaming land never stalls).
    drawn: u64,
    todo: Vec<IVec2>,
    /// Changed since the picture was last written.
    pub rev: u32,
    written: u32,
    /// The colour past what's drawn.
    pub sea: [u8; 4],
}

impl Default for MapImage {
    fn default() -> Self {
        let n = (SIZE * SIZE) as usize;
        MapImage {
            palette: Palette::default(),
            tiles: Default::default(),
            origin: window_round(Vec2::new(WX as f32 / 2.0, WZ as f32 / 2.0)),
            colors: vec![UNSEEN; n],
            heights: vec![SEA as i16; n],
            level: None,
            image: None,
            world: 0,
            drawn: 0,
            todo: vec![],
            rev: 0,
            written: u32::MAX,
            sea: UNSEEN,
        }
    }
}

/// The window's corner for a window round `at`.
fn window_round(at: Vec2) -> IVec2 {
    let c = (at / CHUNK as f32).floor().as_ivec2() * CHUNK;
    c - IVec2::splat(SIZE / 2)
}

impl MapImage {
    /// Where (x, z) is in the window, if it's in it.
    #[inline]
    fn idx(&self, x: i32, z: i32) -> Option<usize> {
        let (lx, lz) = (x - self.origin.x, z - self.origin.y);
        (lx >= 0 && lz >= 0 && lx < SIZE && lz < SIZE).then(|| (lz * SIZE + lx) as usize)
    }

    /// Draws everything loaded again.
    pub fn draw_all(&mut self, v: &Voxels) {
        self.world = v.id;
        self.tiles.clear();
        self.todo.clear();
        let mut keys: Vec<IVec2> = v.keys().collect();
        keys.sort_by_key(|k| (k.y, k.x));
        for k in keys {
            self.draw_tile(v, k);
        }
        self.drawn = v.serial;
        self.compose(v);
    }

    /// Draws one column from what's there, and the row just south of it,
    /// whose slopes it lights.
    fn draw_tile(&mut self, v: &Voxels, key: IVec2) {
        let (x0, z0) = (key.x * CHUNK, key.y * CHUNK);
        let n = (CHUNK * CHUNK) as usize;
        let mut tile = Tile { colors: vec![UNSEEN; n], heights: vec![0; n] };
        // looking down from the highest part with anything in it
        let from = v.column(key).map_or(WY - 1, |c| (0..crate::world::SECTIONS).rev().find(|&i| c.uniform(i) != Some(blocks::AIR)).map_or(MIN_Y, |i| MIN_Y + (i as i32 + 1) * CHUNK - 1));
        let cols: Vec<Column> = (0..CHUNK).flat_map(|z| (0..CHUNK).map(move |x| (x, z))).map(|(x, z)| surface(v, x0 + x, z0 + z, from)).collect();
        for (i, c) in cols.iter().enumerate() {
            tile.heights[i] = c.top() as i16;
        }
        for z in 0..CHUNK {
            for x in 0..CHUNK {
                let i = (z * CHUNK + x) as usize;
                let north = if z > 0 { tile.heights[i - CHUNK as usize] as i32 } else { self.height_of(x0 + x, z0 - 1).unwrap_or(cols[i].top()) };
                tile.colors[i] = shade(&self.palette, cols[i], north);
            }
        }
        self.tiles.insert(key, tile);
        self.blit(key);
        // the row south of it, lit by it
        let south = key + IVec2::Y;
        if self.tiles.contains_key(&south) && v.has(south) {
            for x in x0..x0 + CHUNK {
                self.draw_point(v, x, z0 + CHUNK);
            }
        }
    }

    /// The height kept for a spot, if it's been drawn.
    fn height_of(&self, x: i32, z: i32) -> Option<i32> {
        let k = IVec2::new(x.div_euclid(CHUNK), z.div_euclid(CHUNK));
        self.tiles.get(&k).map(|t| t.heights[(z.rem_euclid(CHUNK) * CHUNK + x.rem_euclid(CHUNK)) as usize] as i32)
    }

    /// Draws one spot again (its blocks changed).
    fn draw_point(&mut self, v: &Voxels, x: i32, z: i32) {
        let k = IVec2::new(x.div_euclid(CHUNK), z.div_euclid(CHUNK));
        if !v.has(k) || !self.tiles.contains_key(&k) {
            return;
        }
        let col = surface(v, x, z, WY - 1);
        let north = self.height_of(x, z - 1).unwrap_or(col.top());
        let color = shade(&self.palette, col, north);
        let i = (z.rem_euclid(CHUNK) * CHUNK + x.rem_euclid(CHUNK)) as usize;
        if let Some(t) = self.tiles.get_mut(&k) {
            t.heights[i] = col.top() as i16;
            t.colors[i] = color;
        }
        if let Some(w) = self.idx(x, z) {
            self.heights[w] = col.top() as i16;
            self.colors[w] = color;
        }
    }

    /// Copies a kept column into the window, if it's in it.
    fn blit(&mut self, key: IVec2) {
        let Some(t) = self.tiles.get(&key) else { return };
        let (x0, z0) = (key.x * CHUNK, key.y * CHUNK);
        for z in 0..CHUNK {
            let Some(w) = self.idx(x0, z0 + z) else { continue };
            if self.idx(x0 + CHUNK - 1, z0 + z).is_none() {
                continue;
            }
            let i = (z * CHUNK) as usize;
            self.colors[w..w + CHUNK as usize].copy_from_slice(&t.colors[i..i + CHUNK as usize]);
            self.heights[w..w + CHUNK as usize].copy_from_slice(&t.heights[i..i + CHUNK as usize]);
        }
    }

    /// Fills the window from the columns kept, and the level if one's shown.
    fn compose(&mut self, v: &Voxels) {
        self.colors.fill(UNSEEN);
        self.heights.fill(SEA as i16);
        let lo = self.origin.div_euclid(IVec2::splat(CHUNK));
        let n = SIZE / CHUNK;
        for kz in lo.y..lo.y + n {
            for kx in lo.x..lo.x + n {
                self.blit(IVec2::new(kx, kz));
            }
        }
        if let Some((y, at, _)) = self.level {
            self.level = Some((y, at, self.level_colors(v, y, at)));
        }
        self.rev += 1;
    }

    /// Moves the window to keep `at` well inside it.
    pub fn follow(&mut self, v: &Voxels, at: Vec2) {
        let want = window_round(at);
        if (want - self.origin).abs().max_element() >= SIZE / 6 {
            self.origin = want;
            self.compose(v);
        }
    }

    /// Lets go of the columns furthest from `at` when too many are kept.
    fn forget(&mut self, at: Vec2) {
        if self.tiles.len() <= MAX_TILES {
            return;
        }
        let c = (at / CHUNK as f32).floor().as_ivec2();
        let mut keys: Vec<IVec2> = self.tiles.keys().copied().collect();
        keys.sort_by_key(|k| -(*k - c).length_squared());
        for k in keys.into_iter().take(self.tiles.len() - MAX_TILES * 9 / 10) {
            self.tiles.remove(&k);
        }
    }

    /// Draws the columns that came in, and the spots whose blocks changed
    /// and those just south of them (whose slope they light).
    pub fn draw_changes(&mut self, v: &mut Voxels, at: Vec2) {
        // a new world, or more changes than are worth going through one by one
        if v.id != self.world || v.map_dirty.len() >= 65536 {
            v.map_dirty.clear();
            self.draw_all(v);
            return;
        }
        let new: Vec<IVec2> = v.since(self.drawn).collect();
        self.drawn = v.serial;
        self.todo.extend(new);
        if self.todo.is_empty() && v.map_dirty.is_empty() {
            return;
        }
        // nearest first, north before south among those drawn together
        let c = (at / CHUNK as f32).floor().as_ivec2();
        self.todo.sort_by_key(|k| std::cmp::Reverse((*k - c).length_squared()));
        let mut now: Vec<IVec2> = (0..32).map_while(|_| self.todo.pop()).collect();
        now.sort_by_key(|k| (k.y, k.x));
        for k in now {
            if v.has(k) {
                self.draw_tile(v, k);
            }
        }
        self.forget(at);
        let mut spots: Vec<IVec2> = v.map_dirty.drain().collect();
        spots.extend(spots.clone().into_iter().map(|c| c + IVec2::Y));
        spots.sort_by_key(|c| (c.y, c.x));
        spots.dedup();
        for c in &spots {
            self.draw_point(v, c.x, c.y);
        }
        if let Some((y, round, _)) = &self.level {
            let (y, round) = (*y, *round);
            for c in &spots {
                if let Some(i) = self.idx(c.x, c.y) {
                    let color = self.level_color(v, c.x, c.y, y, round);
                    if let Some((_, _, level)) = &mut self.level {
                        level[i] = color;
                    }
                }
            }
        }
        self.rev += 1;
    }

    /// Shows the level at height `y` (under a roof) round `at`, or the world
    /// from above (None).
    pub fn show_level(&mut self, v: &Voxels, y: Option<i32>, at: Vec2) {
        let round = (at / CHUNK as f32).floor().as_ivec2() * CHUNK;
        let now = self.level.as_ref().map(|l| (l.0, l.1));
        // the level follows the player a column at a time
        if now == y.map(|y| (y, round)) {
            return;
        }
        self.level = y.map(|y| (y, round, self.level_colors(v, y, round)));
        self.rev += 1;
    }

    /// The window's colours at level `y`, drawn round `round`.
    fn level_colors(&self, v: &Voxels, y: i32, round: IVec2) -> Vec<[u8; 4]> {
        let mut out = Vec::with_capacity((SIZE * SIZE) as usize);
        for z in self.origin.y..self.origin.y + SIZE {
            for x in self.origin.x..self.origin.x + SIZE {
                out.push(self.level_color(v, x, z, y, round));
            }
        }
        out
    }

    fn level_color(&self, v: &Voxels, x: i32, z: i32, y: i32, round: IVec2) -> [u8; 4] {
        let near = (x - round.x).abs().max((z - round.y).abs()) <= LEVEL_REACH;
        if near && v.loaded(x, z) {
            return level_color(&self.palette, v, x, z, y);
        }
        let c = self.idx(x, z).map_or(UNSEEN, |i| self.colors[i]);
        let rgb = mix(ROCK, [c[0], c[1], c[2]], 0.35);
        [rgb[0], rgb[1], rgb[2], 255]
    }

    /// The colours being shown.
    pub fn shown(&self) -> &[[u8; 4]] {
        match &self.level {
            Some((_, _, l)) => l,
            None => &self.colors,
        }
    }

    /// The colour at a spot in the world: unseen where nothing's drawn.
    pub fn at(&self, x: f32, z: f32) -> [u8; 4] {
        let (x, z) = (x.floor() as i32, z.floor() as i32);
        if let Some(i) = self.idx(x, z) {
            return self.shown()[i];
        }
        let k = IVec2::new(x.div_euclid(CHUNK), z.div_euclid(CHUNK));
        self.tiles.get(&k).map_or(UNSEEN, |t| t.colors[(z.rem_euclid(CHUNK) * CHUNK + x.rem_euclid(CHUNK)) as usize])
    }

    pub fn height(&self, x: f32, z: f32) -> i32 {
        let (x, z) = (x.floor() as i32, z.floor() as i32);
        if let Some(i) = self.idx(x, z) {
            return self.heights[i] as i32;
        }
        self.height_of(x, z).unwrap_or(SEA)
    }

    /// The part of the world drawn so far: (north-west, south-east corner).
    pub fn bounds(&self) -> (Vec2, Vec2) {
        let mut lo = IVec2::new(0, 0);
        let mut hi = IVec2::new(crate::world::CX, crate::world::CZ);
        for k in self.tiles.keys() {
            lo = lo.min(*k);
            hi = hi.max(*k + IVec2::ONE);
        }
        ((lo * CHUNK).as_vec2(), (hi * CHUNK).as_vec2())
    }
}

/// A column at one level: rock where the level is inside something solid,
/// otherwise the floor below it, lit by how near it is.
fn level_color(p: &Palette, v: &Voxels, x: i32, z: i32, y: i32) -> [u8; 4] {
    let here = v.get(x, y, z);
    if blocks::is_opaque(here) {
        return [ROCK[0], ROCK[1], ROCK[2], 255];
    }
    let c = surface(v, x, z, y);
    // floors far below fade into the dark
    let drop = (y - c.top()).max(0) as f32;
    let lit = (1.0 - drop / 14.0).clamp(0.25, 1.0);
    let rgb = shade(p, c, c.top());
    let rgb = mix(ROCK, [rgb[0], rgb[1], rgb[2]], lit);
    [rgb[0], rgb[1], rgb[2], 255]
}

/// The level the map should show for someone with their eyes at `eye`:
/// a level when there's a solid roof over them and round them (a cave, a
/// house), none out under the sky or a tree.
pub fn roof_level(v: &Voxels, eye: Vec3) -> Option<i32> {
    let head = eye.y.floor() as i32;
    let roofed = |x: i32, z: i32| ((head + 1)..(head + 24).min(WY)).any(|y| blocks::is_opaque(v.get(x, y, z)));
    let (x, z) = (eye.x.floor() as i32, eye.z.floor() as i32);
    if !roofed(x, z) {
        return None;
    }
    let round = [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().filter(|(dx, dz)| roofed(x + dx, z + dz)).count();
    (round >= 2).then_some(head)
}

fn image(size: u32, fill: [u8; 4]) -> Image {
    let mut img = Image::new_fill(Extent3d { width: size, height: size, depth_or_array_layers: 1 }, TextureDimension::D2, &fill, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::default());
    img.sampler = ImageSampler::nearest();
    img
}

// ---------- the minimap ----------

/// How far the minimap sees: blocks per pixel, by the setting's name.
pub const ZOOMS: &[(&str, f32)] = &[("off", 0.0), ("near", 0.5), ("middle", 1.0), ("far", 2.0)];

pub fn zoom_of(setting: &str) -> f32 {
    ZOOMS.iter().find(|z| z.0 == setting).map(|z| z.1).unwrap_or(1.0)
}

pub fn next_zoom(setting: &str) -> &'static str {
    let i = ZOOMS.iter().position(|z| z.0 == setting).unwrap_or(2);
    ZOOMS[(i + 1) % ZOOMS.len()].0
}

/// The setting's name as the menus say it.
/// A small shadow under labels drawn over the world or the map, so they
/// read on snow and sand alike (Bevy's own is 4 px off, which doubles them).
pub fn shadow() -> TextShadow {
    TextShadow { offset: Vec2::new(1.0, 1.5), color: Color::srgba(0.0, 0.0, 0.0, 0.85) }
}

pub fn zoom_name(setting: &str) -> &'static str {
    match setting {
        "off" => "Off",
        "near" => "Near",
        "far" => "Far",
        _ => "Middle",
    }
}

/// The minimap's size on the screen.
pub const MINI: f32 = 196.0;

/// A dot on the minimap: where, its colour and how big.
pub struct Dot {
    pub at: Vec2,
    pub rgb: [u8; 3],
    pub r: f32,
}

/// Draws the minimap into `out` (`size` square, RGBA): a round window on
/// the map centred on `centre`, turned so `facing` is up when `turn` is
/// set (otherwise north is up), with dots and an arrow for the player.
pub fn draw_minimap(out: &mut [u8], size: usize, map: &MapImage, centre: Vec2, facing: Vec2, turn: bool, per_px: f32, dots: &[Dot]) {
    let c = size as f32 / 2.0;
    let r = c - 1.0;
    let f = if facing.length() > 1e-4 { facing.normalize() } else { Vec2::NEG_Y };
    // screen to world: up on the screen is `f` when turning
    let (ax, ay) = if turn { (Vec2::new(-f.y, f.x), Vec2::new(-f.x, -f.y)) } else { (Vec2::X, Vec2::Y) };
    let to_world = |sx: f32, sy: f32| centre + (ax * sx + ay * sy) * per_px;
    // world to screen, for the dots
    let to_screen = |w: Vec2| {
        let d = (w - centre) / per_px;
        if turn { Vec2::new(d.dot(ax), d.dot(ay)) } else { d }
    };
    for py in 0..size {
        for px in 0..size {
            let (sx, sy) = (px as f32 + 0.5 - c, py as f32 + 0.5 - c);
            let d = (sx * sx + sy * sy).sqrt();
            let i = (py * size + px) * 4;
            if d > r + 0.5 {
                out[i..i + 4].copy_from_slice(&[0, 0, 0, 0]);
                continue;
            }
            let w = to_world(sx, sy);
            let mut col = map.at(w.x, w.y);
            // a dark rim with a light edge
            if d > r - 3.0 {
                col = if d > r - 1.0 { [230, 232, 236, 255] } else { [16, 18, 24, 255] };
            }
            // soft outside edge
            let a = ((r + 0.5 - d).clamp(0.0, 1.0) * 255.0) as u8;
            out[i..i + 4].copy_from_slice(&[col[0], col[1], col[2], a.min(col[3])]);
        }
    }
    let mut plot = |x: f32, y: f32, rgb: [u8; 3], cover: f32| {
        let (px, py) = (x.floor() as i32, y.floor() as i32);
        if px < 0 || py < 0 || px >= size as i32 || py >= size as i32 {
            return;
        }
        let (sx, sy) = (px as f32 + 0.5 - c, py as f32 + 0.5 - c);
        if (sx * sx + sy * sy).sqrt() > r - 3.0 {
            return;
        }
        let i = (py as usize * size + px as usize) * 4;
        for k in 0..3 {
            out[i + k] = (out[i + k] as f32 * (1.0 - cover) + rgb[k] as f32 * cover).round() as u8;
        }
    };
    for dot in dots {
        let s = to_screen(dot.at) + Vec2::splat(c);
        let rr = dot.r + 1.2;
        for y in (s.y - rr).floor() as i32..=(s.y + rr).ceil() as i32 {
            for x in (s.x - rr).floor() as i32..=(s.x + rr).ceil() as i32 {
                let d = Vec2::new(x as f32 + 0.5, y as f32 + 0.5).distance(s);
                if d <= rr {
                    // a dark ring round a coloured middle
                    let edge = (rr - d).clamp(0.0, 1.0);
                    let rgb = if d > dot.r { [20, 22, 28] } else { dot.rgb };
                    plot(x as f32, y as f32, rgb, edge);
                }
            }
        }
    }
    // the player: an arrow in the middle, pointing the way they face
    let up = if turn { Vec2::NEG_Y } else { f };
    let side = Vec2::new(-up.y, up.x);
    let tip = Vec2::splat(c) + up * 8.0;
    let back = Vec2::splat(c) - up * 5.5;
    let tri = [tip, back + side * 6.0, Vec2::splat(c) - up * 2.5, back - side * 6.0];
    arrow(&mut plot, &tri);
}

/// Fills an arrow head (tip, right corner, notch, left corner) with a dark
/// outline, four samples a pixel.
fn arrow(plot: &mut impl FnMut(f32, f32, [u8; 3], f32), q: &[Vec2; 4]) {
    let inside = |p: Vec2, grow: f32| -> bool {
        // two triangles: tip, right, notch and tip, notch, left
        let tri = |a: Vec2, b: Vec2, c: Vec2| {
            let s = |p1: Vec2, p2: Vec2| (p2 - p1).perp_dot(p - p1) / (p2 - p1).length();
            let (d1, d2, d3) = (s(a, b), s(b, c), s(c, a));
            (d1 >= -grow && d2 >= -grow && d3 >= -grow) || (d1 <= grow && d2 <= grow && d3 <= grow)
        };
        tri(q[0], q[1], q[2]) || tri(q[0], q[2], q[3])
    };
    let lo = q.iter().fold(Vec2::splat(f32::MAX), |a, b| a.min(*b)) - Vec2::splat(3.0);
    let hi = q.iter().fold(Vec2::splat(f32::MIN), |a, b| a.max(*b)) + Vec2::splat(3.0);
    for y in lo.y.floor() as i32..=hi.y.ceil() as i32 {
        for x in lo.x.floor() as i32..=hi.x.ceil() as i32 {
            let (mut fill, mut edge) = (0.0, 0.0);
            for (ox, oy) in [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)] {
                let p = Vec2::new(x as f32 + ox, y as f32 + oy);
                if inside(p, 0.0) {
                    fill += 0.25;
                } else if inside(p, 1.6) {
                    edge += 0.25;
                }
            }
            if edge > 0.0 {
                plot(x as f32, y as f32, [16, 18, 24], edge);
            }
            if fill > 0.0 {
                plot(x as f32, y as f32, [255, 255, 255], fill);
            }
        }
    }
}

/// Which way a player faces on the map (x east, y south).
pub fn facing(yaw: f32) -> Vec2 {
    Vec2::new(-yaw.sin(), -yaw.cos())
}

// ---------- the HUD's minimap ----------

#[derive(Component)]
pub struct Minimap;
#[derive(Component)]
pub struct MinimapPicture;
#[derive(Component)]
pub struct MinimapLine;
#[derive(Component)]
pub struct MinimapNorth;
/// A waypoint's pin on the minimap.
#[derive(Component)]
pub struct MiniPin(usize);
/// A waypoint's label over the world.
#[derive(Component)]
pub struct WorldPin(usize);

const PINS: usize = 12;

fn pin(letter_font: &TextFont) -> impl Bundle {
    (
        Node { position_type: PositionType::Absolute, width: px(16), height: px(16), justify_content: JustifyContent::Center, align_items: AlignItems::Center, border: UiRect::all(px(1.5)), border_radius: BorderRadius::all(px(8)), ..default() },
        BorderColor::all(Color::srgba(0.06, 0.07, 0.1, 0.9)),
        BackgroundColor(Color::WHITE),
        children![(Text::new(""), letter_font.clone(), TextColor(Color::srgb(0.06, 0.07, 0.1)))],
    )
}

/// The minimap in the top right corner, the coordinates under it, and the
/// labels for waypoints over the world.
pub fn spawn_hud(mut commands: Commands, fonts: Res<Fonts>, mut images: ResMut<Assets<Image>>) {
    let ui = fonts.ui.clone();
    let small = TextFont { font: ui.clone().into(), font_size: FontSize::Px(10.0), ..default() };
    let mini = images.add(image(MINI as u32, [0, 0, 0, 0]));
    commands
        .spawn((Minimap, Node { position_type: PositionType::Absolute, right: px(16), top: px(12), width: px(MINI), flex_direction: FlexDirection::Column, align_items: AlignItems::Center, row_gap: px(4), ..default() }, Pickable::IGNORE, Visibility::Hidden))
        .with_children(|m| {
            m.spawn((MinimapPicture, ImageNode::new(mini), Node { width: px(MINI), height: px(MINI), ..default() })).with_children(|p| {
                p.spawn((
                    MinimapNorth,
                    Text::new("N"),
                    TextFont { font: ui.clone().into(), font_size: FontSize::Px(13.0), ..default() },
                    TextColor(INK),
                    shadow(),
                    Node { position_type: PositionType::Absolute, ..default() },
                ));
                for i in 0..PINS {
                    p.spawn((MiniPin(i), pin(&small), Visibility::Hidden));
                }
            });
            m.spawn((MinimapLine, Text::new(""), TextFont { font: ui.clone().into(), font_size: FontSize::Px(13.0), ..default() }, TextColor(INK), shadow(), TextLayout::justify(Justify::Center)));
        });
    for i in 0..PINS {
        commands.spawn((
            WorldPin(i),
            Node { position_type: PositionType::Absolute, flex_direction: FlexDirection::Column, align_items: AlignItems::Center, row_gap: px(2), ..default() },
            Pickable::IGNORE,
            Visibility::Hidden,
            children![
                (Node { width: px(10), height: px(10), border: UiRect::all(px(1.5)), border_radius: BorderRadius::all(px(2)), ..default() }, UiTransform::from_rotation(Rot2::degrees(45.0)), BorderColor::all(Color::srgba(0.06, 0.07, 0.1, 0.9)), BackgroundColor(Color::WHITE)),
                (Text::new(""), TextFont { font: ui.clone().into(), font_size: FontSize::Px(13.0), ..default() }, TextColor(INK), shadow(), TextLayout::justify(Justify::Center)),
            ],
        ));
    }
}

/// The map's colours from the blocks' own icons: after startup, while the
/// icons are still on this side (they move to the graphics card after).
pub fn palette(mut map: ResMut<MapImage>, atlas: Option<Res<crate::textures::BlockAtlas>>, images: Res<Assets<Image>>) {
    if let Some(atlas) = atlas {
        map.palette = Palette::from_icons(&atlas, &images);
        // drawn again in the new colours
        map.world = 0;
    }
}

/// Keeps the map up to date: blocks that changed, and the level shown
/// under a roof (switching only after a moment, so a doorway doesn't flicker).
#[allow(clippy::too_many_arguments)]
pub fn refresh(
    time: Res<Time>,
    mut map: ResMut<MapImage>,
    mut voxels: ResMut<Voxels>,
    me: Res<Me>,
    info: Res<WorldInfo>,
    focus: Res<Focus>,
    mut view: ResMut<MapView>,
    mut images: ResMut<Assets<Image>>,
    mut layer: Query<&mut BackgroundColor, With<MapLayer>>,
    mut roof: Local<(Option<i32>, f32)>,
    mut since: Local<f32>,
) {
    if !info.in_world && map.world != 0 {
        return;
    }
    let mine = Vec2::new(me.pos.x, me.pos.z);
    *since += time.delta_secs();
    // a new world is drawn at once; changes a few times a second
    if voxels.id != map.world || *since > 0.25 {
        *since = 0.0;
        map.draw_changes(&mut voxels, mine);
    }
    // the picture holds the part round the player, or round where the full
    // map looks
    map.follow(&voxels, if on_map(&focus.screen) { view.centre } else { mine });
    view.bounds = map.bounds();
    let want = roof_level(&voxels, me.eye());
    if want.is_some() != roof.0.is_some() {
        roof.1 += time.delta_secs();
        if roof.1 > if want.is_some() { 0.4 } else { 0.8 } {
            roof.0 = want;
            roof.1 = 0.0;
        }
    } else {
        roof.0 = want;
        roof.1 = 0.0;
    }
    // a level shown follows the player up and down
    map.show_level(&voxels, roof.0, mine);
    if map.written != map.rev {
        map.written = map.rev;
        for mut bg in &mut layer {
            bg.0 = Color::srgb_u8(map.sea[0], map.sea[1], map.sea[2]);
        }
        let Some(h) = map.image.clone() else { return };
        if let Some(mut img) = images.get_mut(&h) {
            img.data = Some(map.shown().iter().flatten().copied().collect());
        }
    }
}

/// Where a waypoint pin goes on the minimap, from its middle: inside the
/// ring, or held on its edge when it's further away.
fn mini_offset(d: Vec2, per_px: f32, facing: Vec2, turn: bool) -> (Vec2, bool) {
    let f = facing.normalize_or(Vec2::NEG_Y);
    let (ax, ay) = if turn { (Vec2::new(-f.y, f.x), Vec2::new(-f.x, -f.y)) } else { (Vec2::X, Vec2::Y) };
    let s = Vec2::new(d.dot(ax), d.dot(ay)) / per_px;
    let edge = MINI / 2.0 - 12.0;
    if s.length() > edge { (s.normalize() * edge, true) } else { (s, false) }
}

/// The minimap, the coordinates and the waypoints over the world, each frame.
#[allow(clippy::too_many_arguments)]
pub fn draw_hud(
    state: (Res<MapImage>, Res<Me>, Res<Settings>, Res<Focus>, Res<WorldInfo>, Res<Herd>, Res<Net>, Res<Waypoints>, Res<Time>),
    mut images: ResMut<Assets<Image>>,
    mut root: Query<&mut Visibility, (With<Minimap>, Without<MiniPin>, Without<WorldPin>)>,
    picture: Query<&ImageNode, With<MinimapPicture>>,
    mut texts: ParamSet<(Query<&mut Text, With<MinimapLine>>, Query<&mut Node, With<MinimapNorth>>)>,
    mut pins: Query<(&MiniPin, &mut Node, &mut Visibility, &mut BackgroundColor, &Children), (Without<MinimapNorth>, Without<WorldPin>)>,
    mut world_pins: Query<(&WorldPin, &mut Node, &mut Visibility, &Children), (Without<MinimapNorth>, Without<MiniPin>)>,
    mut letters: Query<&mut Text, (Without<MinimapLine>, Without<MinimapNorth>)>,
    mut diamonds: Query<&mut BackgroundColor, Without<MiniPin>>,
    cam: Query<(&Camera, &GlobalTransform), With<crate::player::MainCamera>>,
    mut last: Local<(Vec3, f32, u32, f32, usize)>,
) {
    let (map, me, settings, focus, info, herd, net, waypoints, time) = state;
    let per_px = zoom_of(&settings.minimap);
    let show = info.in_world && per_px > 0.0 && matches!(focus.screen, Screen::Playing | Screen::Panel | Screen::Chat);
    for mut v in &mut root {
        *v = if show { Visibility::Inherited } else { Visibility::Hidden };
    }
    let f = facing(me.yaw);
    let turn = settings.minimap_turns;
    if show {
        // drawn again when something on it moved, and now and then for others
        last.3 += time.delta_secs();
        let moved = me.pos.distance(last.0) > 0.05 || (me.yaw - last.1).abs() > 0.004 || map.rev != last.2 || last.3 > 0.2;
        if moved {
            *last = (me.pos, me.yaw, map.rev, 0.0, last.4);
            let mut dots: Vec<Dot> = herd.bodies.values().map(|b| Dot { at: Vec2::new(b.pos.x, b.pos.z), rgb: [236, 222, 190], r: 1.8 }).collect();
            for p in net.remote_players() {
                let c = Color::hsl((p.id as f32 * 67.0) % 360.0, 0.7, 0.62).to_srgba();
                dots.push(Dot { at: Vec2::new(p.pos.x, p.pos.z), rgb: [(c.red * 255.0) as u8, (c.green * 255.0) as u8, (c.blue * 255.0) as u8], r: 3.2 });
            }
            if let Some(mut img) = picture.single().ok().and_then(|p| images.get_mut(&p.image)) {
                let size = MINI as usize;
                let data = img.data.get_or_insert_with(|| vec![0; size * size * 4]);
                draw_minimap(data, size, &map, Vec2::new(me.pos.x, me.pos.z), f, turn, per_px, &dots);
            }
        }
        if let Ok(mut t) = texts.p0().single_mut() {
            let p = me.pos.floor().as_ivec3();
            let line = format!("{}, {}, {}{}", p.x, p.y, p.z, if map.level.is_some() { "  ·  this level" } else { "" });
            if t.0 != line {
                t.0 = line;
            }
        }
        // north on the ring
        if let Ok(mut n) = texts.p1().single_mut() {
            let (o, _) = mini_offset(Vec2::new(0.0, -10000.0), 1.0, f, turn);
            let at = o.normalize_or(Vec2::NEG_Y) * (MINI / 2.0 - 9.0) + Vec2::splat(MINI / 2.0);
            n.left = px(at.x - 5.0);
            n.top = px(at.y - 9.0);
        }
    }
    // waypoints: on the minimap, and over the world
    let mut near: Vec<&waypoints::Waypoint> = waypoints.list().iter().filter(|w| w.shown).collect();
    let flat = |w: &waypoints::Waypoint| Vec2::new(w.pos().x - me.pos.x, w.pos().z - me.pos.z).length();
    near.sort_by(|a, b| flat(a).total_cmp(&flat(b)));
    for (pin, mut node, mut vis, mut bg, kids) in &mut pins {
        let Some(w) = near.get(pin.0).filter(|_| show) else {
            *vis = Visibility::Hidden;
            continue;
        };
        let d = Vec2::new(w.pos().x - me.pos.x, w.pos().z - me.pos.z);
        let (o, _) = mini_offset(d, per_px, f, turn);
        node.left = px(MINI / 2.0 + o.x - 8.0);
        node.top = px(MINI / 2.0 + o.y - 8.0);
        let c = w.rgb();
        bg.0 = Color::srgb_u8(c[0], c[1], c[2]);
        *vis = Visibility::Inherited;
        if let Some(mut t) = kids.first().and_then(|k| letters.get_mut(*k).ok()) {
            let l = w.letter();
            if t.0 != l {
                t.0 = l;
            }
        }
    }
    let labels = settings.waypoint_labels && info.in_world && matches!(focus.screen, Screen::Playing | Screen::Panel | Screen::Chat);
    // with chat open, names that would sit behind it are left out
    let clear_of = if focus.screen == Screen::Chat { crate::chat::WIDTH + 60.0 } else { 0.0 };
    let view = cam.single().ok();
    // marked on the map, a waypoint stands on the ground there
    let spot_of = |w: &waypoints::Waypoint| if w.at[1] > 0 { w.pos() } else { w.pos().with_y(map.height(w.pos().x, w.pos().z) as f32 + 1.0) };
    for (pin, mut node, mut vis, kids) in &mut world_pins {
        let spot = near.get(pin.0).filter(|w| labels && spot_of(w).distance(me.pos) > 4.0).and_then(|w| {
            let (camera, ct) = view?;
            let at = spot_of(w) + Vec3::Y * 1.2;
            // only what's in front
            (ct.forward().dot((at - ct.translation()).normalize()) > 0.0).then_some(())?;
            camera.world_to_viewport(ct, at).ok().filter(|v| v.x > clear_of).map(|v| (v, w))
        });
        let Some((v, w)) = spot else {
            *vis = Visibility::Hidden;
            continue;
        };
        let text = format!("{}\n{}", w.name, distance(spot_of(w).distance(me.pos)).trim_start());
        let wide = w.name.chars().count().max(5) as f32 * 3.6;
        node.left = px(v.x - wide);
        node.top = px(v.y - 8.0);
        node.width = px(wide * 2.0);
        *vis = Visibility::Inherited;
        if let Some(k) = kids.first() {
            if let Ok(mut d) = diamonds.get_mut(*k) {
                let c = w.rgb();
                d.0 = Color::srgb_u8(c[0], c[1], c[2]);
            }
        }
        if let Some(mut t) = kids.get(1).and_then(|k| letters.get_mut(*k).ok()) {
            if t.0 != text {
                t.0 = text;
            }
        }
    }
}

/// Creations' lines in the top right go under the minimap while it shows.
pub fn make_room(settings: Res<Settings>, mut pieces: Query<(&crate::ui::PieceText, &mut Node)>) {
    let top = if zoom_of(&settings.minimap) > 0.0 { MINI + 40.0 } else { 14.0 };
    for (p, mut n) in &mut pieces {
        if p.0 == 0 && n.top != px(top) {
            n.top = px(top);
        }
    }
}

// ---------- the full map ----------

/// What the full map looks at: its middle, how many pixels a block takes,
/// and the spot picked (the middle with a pad; a click with a mouse).
#[derive(Resource)]
pub struct MapView {
    pub centre: Vec2,
    pub zoom: f32,
    pub cursor: Vec2,
    /// The menu item the map last moved to, so picking one shows it once.
    shown_sel: Option<usize>,
    drag: Option<Vec2>,
    dragged: f32,
    /// The page of the waypoint list showing.
    pub page: usize,
    /// The part of the world drawn so far, which the middle stays in.
    pub bounds: (Vec2, Vec2),
}

impl Default for MapView {
    fn default() -> Self {
        let island = (Vec2::ZERO, Vec2::new(WX as f32, WZ as f32));
        MapView { centre: island.1 / 2.0, zoom: 3.0, cursor: Vec2::ZERO, shown_sel: None, drag: None, dragged: 0.0, page: 0, bounds: island }
    }
}

pub const MIN_ZOOM: f32 = 1.0;
pub const MAX_ZOOM: f32 = 16.0;
/// The menu's width on the right of the full map.
pub const PANEL: f32 = 420.0;

impl MapView {
    /// The middle of the part of the screen the map has (left of the menu).
    pub fn middle(window: Vec2) -> Vec2 {
        Vec2::new((window.x - PANEL).max(200.0) / 2.0, window.y / 2.0)
    }

    pub fn to_screen(&self, window: Vec2, w: Vec2) -> Vec2 {
        MapView::middle(window) + (w - self.centre) * self.zoom
    }

    pub fn to_world(&self, window: Vec2, s: Vec2) -> Vec2 {
        self.centre + (s - MapView::middle(window)) / self.zoom
    }

    /// Zooms by a factor, keeping the world under `at` (on the screen) still.
    pub fn zoom_by(&mut self, window: Vec2, at: Vec2, factor: f32) {
        let before = self.to_world(window, at);
        self.zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        let after = self.to_world(window, at);
        self.centre += before - after;
        self.keep_in();
    }

    fn keep_in(&mut self) {
        self.centre = self.centre.clamp(self.bounds.0, self.bounds.1);
    }
}

/// The full map's layer under the menu.
#[derive(Component)]
pub struct MapLayer;
#[derive(Component)]
pub struct MapPicture;
#[derive(Component)]
pub struct MapMarker(usize);
#[derive(Component)]
pub struct MapMe;
#[derive(Component)]
pub struct MapCursor;
#[derive(Component)]
pub struct MapReadout;
#[derive(Component)]
pub struct MapHint;

const MARKERS: usize = 48;

/// The arrow for the player on the full map, pointing up.
fn arrow_image() -> Image {
    let size = 32usize;
    let mut data = vec![0u8; size * size * 4];
    let mut cover = vec![(0.0f32, 0.0f32); size * size];
    let c = Vec2::splat(16.0);
    let up = Vec2::NEG_Y;
    let side = Vec2::X;
    let q = [c + up * 13.0, c - up * 9.0 + side * 10.0, c - up * 4.0, c - up * 9.0 - side * 10.0];
    let mut plot = |x: f32, y: f32, rgb: [u8; 3], a: f32| {
        let (x, y) = (x as usize, y as usize);
        if x < size && y < size {
            let k = y * size + x;
            if rgb[0] > 128 { cover[k].0 = cover[k].0.max(a) } else { cover[k].1 = cover[k].1.max(a) }
        }
    };
    arrow(&mut plot, &q);
    for (k, (fill, edge)) in cover.iter().enumerate() {
        let a = (fill + edge).min(1.0);
        let v = if a > 0.0 { (fill / a * 255.0) as u8 } else { 0 };
        let i = k * 4;
        data[i..i + 4].copy_from_slice(&[v.max(16), v.max(18), v.max(24), (a * 255.0) as u8]);
    }
    let mut img = Image::new(Extent3d { width: size as u32, height: size as u32, depth_or_array_layers: 1 }, TextureDimension::D2, data, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::RENDER_WORLD);
    img.sampler = ImageSampler::linear();
    img
}

pub fn spawn_map(mut commands: Commands, fonts: Res<Fonts>, mut map: ResMut<MapImage>, mut images: ResMut<Assets<Image>>) {
    let ui = fonts.ui.clone();
    let font = |s: f32| TextFont { font: ui.clone().into(), font_size: FontSize::Px(s), ..default() };
    let arrow = images.add(arrow_image());
    let picture = images.add(image(SIZE as u32, UNSEEN));
    map.image = Some(picture.clone());
    commands
        .spawn((
            MapLayer,
            Node { position_type: PositionType::Absolute, width: percent(100), height: percent(100), overflow: Overflow::clip(), ..default() },
            BackgroundColor(Color::srgb_u8(UNSEEN[0], UNSEEN[1], UNSEEN[2])),
            GlobalZIndex(9),
            Visibility::Hidden,
        ))
        .with_children(|m| {
            m.spawn((MapPicture, ImageNode::new(picture), Node { position_type: PositionType::Absolute, ..default() }, Pickable::IGNORE));
            for i in 0..MARKERS {
                m.spawn((
                    MapMarker(i),
                    Node { position_type: PositionType::Absolute, align_items: AlignItems::Center, column_gap: px(6), ..default() },
                    Pickable::IGNORE,
                    Visibility::Hidden,
                    children![
                        (
                            Node { width: px(20), height: px(20), justify_content: JustifyContent::Center, align_items: AlignItems::Center, border: UiRect::all(px(2)), border_radius: BorderRadius::all(px(10)), ..default() },
                            BorderColor::all(Color::srgba(0.06, 0.07, 0.1, 0.9)),
                            BackgroundColor(Color::WHITE),
                            children![(Text::new(""), font(11.0), TextColor(Color::srgb(0.06, 0.07, 0.1)))],
                        ),
                        (Text::new(""), font(14.0), TextColor(INK), shadow()),
                    ],
                ));
            }
            m.spawn((MapMe, ImageNode::new(arrow), Node { position_type: PositionType::Absolute, width: px(28), height: px(28), ..default() }, UiTransform::default(), Pickable::IGNORE));
            m.spawn((
                MapCursor,
                Node { position_type: PositionType::Absolute, width: px(26), height: px(26), border: UiRect::all(px(2)), border_radius: BorderRadius::all(px(13)), ..default() },
                BorderColor::all(Color::WHITE),
                Pickable::IGNORE,
            ))
            .with_child((Node { position_type: PositionType::Absolute, left: px(10), top: px(10), width: px(2), height: px(2), ..default() }, BackgroundColor(Color::WHITE)));
            m.spawn((
                MapReadout,
                Text::new(""),
                font(15.0),
                TextColor(INK),
                Node { position_type: PositionType::Absolute, left: px(18), bottom: px(42), padding: UiRect::axes(px(12), px(6)), border_radius: BorderRadius::all(px(10)), ..default() },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
            ));
            m.spawn((MapHint, Text::new(""), font(13.0), TextColor(DIM), shadow(), Node { position_type: PositionType::Absolute, left: px(18), bottom: px(14), ..default() }));
        });
}

/// Whether a screen is one of the map's.
pub fn on_map(s: &Screen) -> bool {
    matches!(s, Screen::Map | Screen::Waypoint(_) | Screen::Shared(_))
}

/// One marker on the full map.
struct Mark {
    at: Vec2,
    rgb: [u8; 3],
    letter: String,
    label: String,
}

/// The full map, each frame it's open.
#[allow(clippy::too_many_arguments)]
pub fn draw_map(
    view: Res<MapView>,
    focus: Res<Focus>,
    me: Res<Me>,
    map: Res<MapImage>,
    net: Res<Net>,
    chat: Res<Chat>,
    waypoints: Res<Waypoints>,
    c: Res<Controls>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut layer: Query<&mut Visibility, (With<MapLayer>, Without<MapMarker>)>,
    mut picture: Query<(&mut Node, &mut ImageNode), (With<MapPicture>, Without<MapMe>, Without<MapMarker>, Without<MapCursor>)>,
    mut arrow: Query<(&mut Node, &mut UiTransform), (With<MapMe>, Without<MapPicture>, Without<MapMarker>, Without<MapCursor>)>,
    mut cursor: Query<&mut Node, (With<MapCursor>, Without<MapPicture>, Without<MapMe>, Without<MapMarker>)>,
    mut markers: Query<(&MapMarker, &mut Node, &mut Visibility, &Children), (Without<MapPicture>, Without<MapMe>, Without<MapCursor>, Without<MapLayer>)>,
    mut parts: Query<(Option<&mut BackgroundColor>, Option<&mut Text>, Option<&Children>), (Without<MapMarker>, Without<MapReadout>, Without<MapHint>)>,
    mut texts: ParamSet<(Query<&mut Text, With<MapReadout>>, Query<&mut Text, With<MapHint>>)>,
) {
    let open = on_map(&focus.screen);
    for mut v in &mut layer {
        *v = if open { Visibility::Inherited } else { Visibility::Hidden };
    }
    if !open {
        return;
    }
    let Ok(window) = windows.single() else { return };
    let win = Vec2::new(window.width(), window.height());
    // the picture of the part round the middle, cut to what's on the screen
    if let Ok((mut node, mut img)) = picture.single_mut() {
        let o = map.origin.as_vec2();
        let lo = view.to_screen(win, o);
        let hi = view.to_screen(win, o + Vec2::splat(SIZE as f32));
        let (slo, shi) = (lo.max(Vec2::ZERO), hi.min(win));
        if shi.x > slo.x && shi.y > slo.y {
            let tlo = view.to_world(win, slo) - o;
            let thi = view.to_world(win, shi) - o;
            node.left = px(slo.x);
            node.top = px(slo.y);
            node.width = px(shi.x - slo.x);
            node.height = px(shi.y - slo.y);
            img.rect = Some(Rect::from_corners(tlo, thi));
            node.display = Display::Flex;
        } else {
            node.display = Display::None;
        }
    }
    // you
    if let Ok((mut node, mut t)) = arrow.single_mut() {
        let s = view.to_screen(win, Vec2::new(me.pos.x, me.pos.z));
        node.left = px(s.x - 14.0);
        node.top = px(s.y - 14.0);
        let f = facing(me.yaw);
        t.rotation = Rot2::radians(f.x.atan2(-f.y));
    }
    let picked = view.cursor;
    if let Ok(mut node) = cursor.single_mut() {
        let s = view.to_screen(win, picked);
        node.left = px(s.x - 13.0);
        node.top = px(s.y - 13.0);
    }
    // waypoints, friends, places shared in chat and the Green
    let mut marks: Vec<Mark> = vec![];
    let g = crate::world::GREEN;
    marks.push(Mark { at: Vec2::new(g.x, g.z), rgb: [120, 220, 140], letter: "G".into(), label: "The Green".into() });
    for (_, who, p) in chat.places(net.my_id) {
        marks.push(Mark { at: Vec2::new(p.at[0] as f32 + 0.5, p.at[2] as f32 + 0.5), rgb: [200, 200, 210], letter: "?".into(), label: format!("{} (from {who})", p.name) });
    }
    for w in waypoints.list() {
        marks.push(Mark { at: Vec2::new(w.pos().x, w.pos().z), rgb: w.rgb(), letter: w.letter(), label: w.name.clone() });
    }
    for p in net.remote_players() {
        let c = Color::hsl((p.id as f32 * 67.0) % 360.0, 0.7, 0.62).to_srgba();
        marks.push(Mark { at: Vec2::new(p.pos.x, p.pos.z), rgb: [(c.red * 255.0) as u8, (c.green * 255.0) as u8, (c.blue * 255.0) as u8], letter: String::new(), label: p.name.clone() });
    }
    for (m, mut node, mut vis, kids) in &mut markers {
        let Some(mark) = marks.get(m.0) else {
            *vis = Visibility::Hidden;
            continue;
        };
        let s = view.to_screen(win, mark.at);
        if s.x < -40.0 || s.y < -40.0 || s.x > win.x + 40.0 || s.y > win.y + 40.0 {
            *vis = Visibility::Hidden;
            continue;
        }
        node.left = px(s.x - 10.0);
        node.top = px(s.y - 10.0);
        *vis = Visibility::Inherited;
        let (Some(dot), Some(label)) = (kids.first().copied(), kids.get(1).copied()) else { continue };
        let letter = parts.get(dot).ok().and_then(|(_, _, k)| k.and_then(|k| k.first().copied()));
        if let Ok((Some(mut bg), _, _)) = parts.get_mut(dot) {
            bg.0 = Color::srgb_u8(mark.rgb[0], mark.rgb[1], mark.rgb[2]);
        }
        if let Some(l) = letter {
            if let Ok((_, Some(mut t), _)) = parts.get_mut(l) {
                if t.0 != mark.letter {
                    t.0 = mark.letter.clone();
                }
            }
        }
        if let Ok((_, Some(mut t), _)) = parts.get_mut(label) {
            if t.0 != mark.label {
                t.0 = mark.label.clone();
            }
        }
    }
    if let Ok(mut t) = texts.p0().single_mut() {
        let y = map.height(picked.x, picked.y);
        let dist = Vec2::new(me.pos.x, me.pos.z).distance(picked);
        let line = format!("{}, {}, {}  ·  {:.0} m from you{}", picked.x.floor(), y, picked.y.floor(), dist, if map.level.is_some() { "  ·  showing the level you're on" } else { "" });
        if t.0 != line {
            t.0 = line;
        }
    }
    if let Ok(mut t) = texts.p1().single_mut() {
        let hint = if c.pad {
            "Left stick moves · LT and RT zoom · LB and RB go between waypoints · X marks the spot · Right stick press: back to you"
        } else {
            "Drag to move · Wheel zooms · Click picks a spot · Right click marks it · WASD moves · Space: back to you"
        };
        if t.0 != hint {
            t.0 = hint.to_string();
        }
    }
}

/// The full map's own controls: moving it, zooming, picking a spot and
/// marking it. The list on the right is a menu, worked by `ui::menu_input`.
#[allow(clippy::too_many_arguments)]
pub fn map_input(
    time: Res<Time>,
    mut view: ResMut<MapView>,
    focus: Res<Focus>,
    me: Res<Me>,
    waypoints: Res<Waypoints>,
    c: Res<Controls>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    pads: Query<&bevy::input::gamepad::Gamepad>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut acts: ResMut<Acts>,
    mut was_open: Local<bool>,
) {
    let open = focus.screen == Screen::Map;
    if !open {
        *was_open = on_map(&focus.screen);
        return;
    }
    // the map's own button closes it again
    if c.map {
        acts.0.push(Act::Resume);
        return;
    }
    let Ok(window) = windows.single() else { return };
    let win = Vec2::new(window.width(), window.height());
    let me2 = Vec2::new(me.pos.x, me.pos.z);
    if !*was_open {
        // opening it: centred on you, the spot picked where you stand
        *was_open = true;
        view.centre = me2;
        view.cursor = me2;
        view.shown_sel = None;
    }
    let dt = time.delta_secs().min(0.05);
    // a waypoint chosen in the list: the map shows it
    if view.shown_sel != Some(focus.sel) {
        view.shown_sel = Some(focus.sel);
        if let Some(crate::ui::Item { kind: crate::ui::Kind::Button(Act::OpenWaypoint(id)), .. }) = focus.items.get(focus.sel) {
            if let Some(w) = waypoints.get(*id) {
                view.centre = Vec2::new(w.pos().x, w.pos().z);
                view.cursor = view.centre;
            }
        }
    }
    // the pad: the left stick moves, the triggers zoom about the middle
    let mut pan = Vec2::ZERO;
    let mut zoom = 0.0;
    for pad in &pads {
        let ls = pad.left_stick();
        if ls.length() > 0.18 {
            pan += Vec2::new(ls.x, -ls.y) * ls.length();
        }
        zoom += pad.get(GamepadButton::RightTrigger2).unwrap_or(0.0) - pad.get(GamepadButton::LeftTrigger2).unwrap_or(0.0);
        if pad.just_pressed(GamepadButton::RightThumb) {
            view.centre = me2;
        }
        if pad.just_pressed(GamepadButton::West) {
            acts.0.push(Act::MarkCursor);
        }
        let step = pad.just_pressed(GamepadButton::RightTrigger) as i32 - pad.just_pressed(GamepadButton::LeftTrigger) as i32;
        if step != 0 && !waypoints.list().is_empty() {
            // the next waypoint along, by distance from the middle
            let mut list: Vec<Vec2> = waypoints.list().iter().map(|w| Vec2::new(w.pos().x, w.pos().z)).collect();
            list.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
            let i = list.iter().position(|p| p.distance(view.centre) < 0.5).map(|i| i as i32).unwrap_or(-1);
            let n = list.len() as i32;
            let next = if i < 0 { if step > 0 { 0 } else { n - 1 } } else { (i + step).rem_euclid(n) };
            view.centre = list[next as usize];
        }
    }
    // the keyboard: WASD moves, - and = zoom, Space comes back to you
    let k = |code| keys.pressed(code);
    pan += Vec2::new(k(KeyCode::KeyD) as i32 as f32 - k(KeyCode::KeyA) as i32 as f32, k(KeyCode::KeyS) as i32 as f32 - k(KeyCode::KeyW) as i32 as f32);
    zoom += k(KeyCode::Equal) as i32 as f32 - k(KeyCode::Minus) as i32 as f32;
    if keys.just_pressed(KeyCode::Space) {
        view.centre = me2;
    }
    if pan != Vec2::ZERO {
        // about half the screen a second at full tilt, whatever the zoom
        let speed = win.y * 0.6 / view.zoom;
        view.centre += pan.clamp_length_max(1.0) * speed * dt;
        view.keep_in();
    }
    let middle = MapView::middle(win);
    if zoom != 0.0 {
        view.zoom_by(win, middle, (1.0 + 1.6 * dt * zoom.abs().min(1.0)).powf(zoom.signum()));
    }
    if c.pad {
        view.cursor = view.centre;
    }
    // the mouse, over the map (not the menu on the right)
    let pointer = window.cursor_position().filter(|p| p.x < win.x - PANEL);
    if let Some(p) = pointer {
        if scroll.delta.y != 0.0 {
            view.zoom_by(win, p, if scroll.delta.y > 0.0 { 1.25 } else { 0.8 });
        }
        if mouse.just_pressed(MouseButton::Left) {
            view.drag = Some(p);
            view.dragged = 0.0;
        }
        if mouse.just_pressed(MouseButton::Right) {
            view.cursor = view.to_world(win, p);
            acts.0.push(Act::MarkCursor);
        }
    }
    if view.drag.is_some() {
        if mouse.pressed(MouseButton::Left) {
            let d = motion.delta;
            view.dragged += d.length();
            let zoom = view.zoom;
            view.centre -= d / zoom;
            view.keep_in();
        } else {
            // a click that didn't drag picks the spot
            if view.dragged < 4.0 {
                if let Some(p) = pointer {
                    view.cursor = view.to_world(win, p);
                }
            }
            view.drag = None;
        }
    }
}

/// What the map's buttons do: going places, marking them, the minimap.
#[allow(clippy::too_many_arguments)]
pub fn map_acts(
    mut acts: ResMut<Acts>,
    mut focus: ResMut<Focus>,
    mut settings: ResMut<Settings>,
    me: Res<Me>,
    mut view: ResMut<MapView>,
    mut waypoints: ResMut<Waypoints>,
    mut travel: ResMut<Travel>,
    mut chat: ResMut<Chat>,
    net: Res<Net>,
    info: Res<WorldInfo>,
    mut toasts: ResMut<Toasts>,
) {
    let mut rest = vec![];
    let world_allows = net.teleport_allowed();
    for act in std::mem::take(&mut acts.0) {
        let mut go = |to: Vec3, exact: bool, name: &str, goal: Goal, focus: &mut Focus, me: &Me, travel: &mut Travel| match waypoints::may_go(&goal, settings.survival, world_allows, me.riding.is_some(), travel.rest) {
            Ok(wait) => {
                travel.start(me, to, exact, name, wait);
                focus.go(Screen::Playing);
            }
            Err(why) => {
                focus.status = why.clone();
                toasts.push(why);
            }
        };
        match act {
            Act::OpenMap if info.in_world => {
                focus.status.clear();
                focus.go(Screen::Map);
            }
            Act::MapGo => {
                let at = view.cursor;
                go(Vec3::new(at.x, 0.0, at.y), false, "the spot you picked", Goal::Spot, &mut focus, &me, &mut travel);
            }
            Act::MapHome => {
                view.centre = Vec2::new(me.pos.x, me.pos.z);
                view.cursor = view.centre;
            }
            Act::MarkHere | Act::MarkCursor => {
                let here = matches!(act, Act::MarkHere);
                let at = if here { me.pos.floor().as_ivec3() } else {
                    let (x, z) = (view.cursor.x.floor() as i32, view.cursor.y.floor() as i32);
                    IVec3::new(x, 0, z)
                };
                let name = waypoints.next_name();
                // marked on the map: its height is the ground's there
                match waypoints.add(&name, at, here, Kind::Mine) {
                    Some(_) if !here && settings.survival => focus.status = format!("Marked {name}. Go there on foot once to come back to it."),
                    Some(_) if !focus.playing() => focus.status = format!("Marked {name}"),
                    Some(_) => toasts.push(format!("Marked {name}")),
                    None => toasts.push(format!("A world keeps {} waypoints at most.", waypoints::MAX)),
                }
            }
            Act::OpenWaypoint(id) => {
                if let Some(w) = waypoints.get(id) {
                    focus.fields.insert("waypoint".into(), w.name.clone());
                    focus.status.clear();
                    view.centre = Vec2::new(w.pos().x, w.pos().z);
                    view.cursor = view.centre;
                    focus.go(Screen::Waypoint(id));
                }
            }
            Act::GoWaypoint(id) => {
                if let Some(w) = waypoints.get(id).cloned() {
                    // marked on the map, it has no height: land on the ground
                    let exact = w.at[1] > 0;
                    go(w.pos(), exact, &w.name, Goal::Waypoint { stood: w.stood }, &mut focus, &me, &mut travel);
                }
            }
            Act::RenameWaypoint(id, name) => {
                let name: String = name.trim().chars().take(32).collect();
                if !name.is_empty() {
                    waypoints.edit(id, |w| w.name = name.clone());
                    focus.status = format!("Renamed {name}");
                }
            }
            Act::RecolourWaypoint(id) => waypoints.edit(id, |w| w.color = (w.color + 1) % waypoints::COLORS.len()),
            Act::ShowWaypoint(id) => waypoints.edit(id, |w| w.shown = !w.shown),
            Act::ShareWaypoint(id) => {
                if let Some(w) = waypoints.get(id) {
                    let at = if w.at[1] > 0 { w.at } else { [w.at[0], view_height(&w.at), w.at[2]] };
                    chat.out.push(crate::chat::Say { text: String::new(), place: Some(crate::chat::Place { name: w.name.clone(), at }) });
                    focus.status = "Shared in chat: it shows on everyone's map.".into();
                }
            }
            Act::DeleteWaypoint(id) => {
                if focus.status == "Choose Delete again to delete it." {
                    waypoints.remove(id);
                    focus.status.clear();
                    focus.go(Screen::Map);
                } else {
                    focus.status = "Choose Delete again to delete it.".into();
                }
            }
            Act::OpenShared(line) => {
                if let Some((_, _, p)) = chat.places(net.my_id).into_iter().find(|(l, _, _)| *l == line) {
                    view.centre = Vec2::new(p.at[0] as f32 + 0.5, p.at[2] as f32 + 0.5);
                    view.cursor = view.centre;
                    focus.status.clear();
                    focus.go(Screen::Shared(line));
                }
            }
            Act::KeepShared(line) => {
                if let Some((_, who, p)) = chat.places(net.my_id).into_iter().find(|(l, _, _)| *l == line) {
                    if let Some(id) = waypoints.add(&p.name, IVec3::from_array(p.at), false, Kind::Shared(who)) {
                        focus.go(Screen::Waypoint(id));
                        focus.fields.insert("waypoint".into(), p.name.clone());
                    }
                }
            }
            Act::GoShared(line) => {
                if let Some((_, _, p)) = chat.places(net.my_id).into_iter().find(|(l, _, _)| *l == line) {
                    go(Vec3::new(p.at[0] as f32 + 0.5, p.at[1] as f32, p.at[2] as f32 + 0.5), true, &p.name, Goal::Spot, &mut focus, &me, &mut travel);
                }
            }
            Act::GoPlayer(pid) => {
                if let Some(p) = net.remote_players().into_iter().find(|p| p.id == pid) {
                    let (at, name) = (p.pos, p.name.clone());
                    go(at, true, &name, Goal::Player, &mut focus, &me, &mut travel);
                }
            }
            Act::MinimapZoom => {
                settings.minimap = next_zoom(&settings.minimap).into();
                settings.save();
                if focus.playing() {
                    toasts.push(format!("Minimap: {}", zoom_name(&settings.minimap)));
                }
            }
            Act::MapPage => view.page += 1,
            other => rest.push(other),
        }
    }
    acts.0 = rest;
}

/// A waypoint marked on the map has no height of its own; it's shared at
/// the ground's (the receiving game lands on the ground either way).
fn view_height(at: &[i32; 3]) -> i32 {
    at[1].max(1)
}

/// Opening the map, marking where you are and the minimap's zoom, while playing.
pub fn keys(c: Res<Controls>, mut focus: ResMut<Focus>, info: Res<WorldInfo>, mut acts: ResMut<Acts>) {
    if !focus.playing() || !info.in_world {
        return;
    }
    if c.map {
        focus.status.clear();
        focus.go(Screen::Map);
    } else if c.mark {
        acts.0.push(Act::MarkHere);
    } else if c.minimap {
        acts.0.push(Act::MinimapZoom);
    }
}

/// How far, in words (with no-break spaces, so a long row wraps before
/// the distance rather than inside it).
fn distance(d: f32) -> String {
    if d < 1000.0 {
        format!("\u{a0}{d:.0}\u{a0}m")
    } else {
        format!("\u{a0}{:.1}\u{a0}km", d / 1000.0)
    }
}

/// Waypoints a page of the map's list shows.
const PAGE: usize = 8;

/// The map's screens: what their lists say, worked out each frame they're open.
#[allow(clippy::too_many_arguments)]
pub fn fill(
    mut focus: ResMut<Focus>,
    waypoints: Res<Waypoints>,
    me: Res<Me>,
    net: Res<Net>,
    chat: Res<Chat>,
    settings: Res<Settings>,
    travel: Res<Travel>,
    mut view: ResMut<MapView>,
    c: Res<Controls>,
) {
    use crate::ui::{button, field, note, toggle};
    if !on_map(&focus.screen) {
        return;
    }
    let survival = settings.survival;
    let allowed = net.teleport_allowed();
    let can_say = net.chat_allowed() && settings.chat;
    let far = |at: Vec3| distance(Vec2::new(at.x - me.pos.x, at.z - me.pos.z).length());
    let mut v = vec![];
    let title = match focus.screen.clone() {
        Screen::Map => {
            if !focus.status.is_empty() {
                v.push(note(focus.status.clone()));
            }
            if !allowed {
                v.push(note("This world doesn't let players teleport."));
            } else if survival && travel.rest > 0.0 {
                v.push(note(format!("In survival you can go again in {} s.", travel.rest.ceil() as i32)));
            }
            if allowed && !survival {
                v.push(button(if c.pad { "Go to the middle of the map" } else { "Go to the spot you picked" }, Act::MapGo));
            }
            v.push(button(if c.pad { "Mark the middle of the map" } else { "Mark the spot you picked" }, Act::MarkCursor));
            v.push(button("Mark where I am", Act::MarkHere));
            v.push(button("Back to where I am", Act::MapHome));
            let list = waypoints.list();
            if !list.is_empty() {
                let pages = list.len().div_ceil(PAGE);
                view.page %= pages;
                v.push(note(if pages > 1 { format!("Waypoints, page {} of {pages}", view.page + 1) } else { "Waypoints".into() }));
                let mut sorted: Vec<&waypoints::Waypoint> = list.iter().collect();
                let flat = |w: &waypoints::Waypoint| Vec2::new(w.pos().x - me.pos.x, w.pos().z - me.pos.z).length();
                sorted.sort_by(|a, b| flat(a).total_cmp(&flat(b)));
                for w in sorted.iter().skip(view.page * PAGE).take(PAGE) {
                    let what = match &w.kind {
                        Kind::Shared(who) => format!(" (from {who})"),
                        _ => String::new(),
                    };
                    let shut = if survival && allowed && !w.stood { "  ·  not stood at yet" } else { "" };
                    v.push(button(format!("{}{what}  ·{}{shut}", w.name, far(w.pos())), Act::OpenWaypoint(w.id)));
                }
                if pages > 1 {
                    v.push(button("More waypoints", Act::MapPage));
                }
            }
            let shared = chat.places(net.my_id);
            if !shared.is_empty() {
                v.push(note("Shared in chat"));
                for (line, who, p) in shared {
                    let at = Vec3::new(p.at[0] as f32, p.at[1] as f32, p.at[2] as f32);
                    v.push(button(format!("{} from {who}  ·{}", p.name, far(at)), Act::OpenShared(line)));
                }
            }
            let players = net.remote_players();
            if allowed && !survival && !players.is_empty() {
                v.push(note("Friends here"));
                for p in players.iter().take(PAGE) {
                    v.push(button(format!("Go to {}  ·{}", p.name, far(p.pos)), Act::GoPlayer(p.id)));
                }
            }
            v.push(button(format!("Minimap: {}", zoom_name(&settings.minimap)), Act::MinimapZoom));
            v.push(toggle("Minimap turns with you", "minimap_turns", settings.minimap_turns));
            v.push(toggle("Waypoint names over the world", "waypoint_labels", settings.waypoint_labels));
            v.push(button("Close", Act::Resume));
            "Map".to_string()
        }
        Screen::Waypoint(id) => {
            let Some(w) = waypoints.get(id).cloned() else {
                focus.made = ("Gone".into(), vec![button("Back to the map", Act::OpenMap)]);
                return;
            };
            let what = match &w.kind {
                Kind::Mine => String::new(),
                Kind::Fell => "Where you fell  ·  ".into(),
                Kind::Shared(who) => format!("Shared by {who}  ·  "),
            };
            let height = if w.at[1] > 0 { w.at[1].to_string() } else { "on the ground".into() };
            v.push(note(format!("{what}{}, {height}, {}  ·{} from you", w.at[0], w.at[2], far(w.pos()))));
            if survival && allowed && !w.stood {
                v.push(note("In survival you can go back to places you've stood at. Walk here once, and then you can come back."));
            }
            let typed = focus.fields.get("waypoint").cloned().unwrap_or_else(|| w.name.clone());
            v.push(field("Name", "waypoint", typed.clone()));
            v.push(button("Rename it", Act::RenameWaypoint(id, typed)));
            v.push(button(format!("Colour: {}", waypoints::COLORS[w.color % waypoints::COLORS.len()].0), Act::RecolourWaypoint(id)));
            v.push(button(format!("On the minimap and over the world: {}", if w.shown { "On" } else { "Off" }), Act::ShowWaypoint(id)));
            if allowed {
                v.push(button("Go there", Act::GoWaypoint(id)));
            }
            if can_say {
                v.push(button("Share it in chat", Act::ShareWaypoint(id)));
            }
            v.push(button("Delete it", Act::DeleteWaypoint(id)));
            if !focus.status.is_empty() {
                v.push(note(focus.status.clone()));
            }
            v.push(button("Back to the map", Act::OpenMap));
            w.name
        }
        Screen::Shared(line) => {
            let Some((_, who, p)) = chat.places(net.my_id).into_iter().find(|(l, _, _)| *l == line) else {
                focus.made = ("Gone".into(), vec![button("Back to the map", Act::OpenMap)]);
                return;
            };
            let at = Vec3::new(p.at[0] as f32, p.at[1] as f32, p.at[2] as f32);
            v.push(note(format!("Shared by {who} in chat  ·  {}, {}, {}  ·{} from you", p.at[0], p.at[1], p.at[2], far(at))));
            if allowed && !survival {
                v.push(button("Go there", Act::GoShared(line)));
            }
            v.push(button("Keep it as a waypoint", Act::KeepShared(line)));
            if !focus.status.is_empty() {
                v.push(note(focus.status.clone()));
            }
            v.push(button("Back to the map", Act::OpenMap));
            p.name
        }
        _ => return,
    };
    focus.made = (title, v);
}
