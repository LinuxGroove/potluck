//! Blocks made by creations: `[blocks.marble]` in a creation's potluck.toml,
//! with its pictures drawn as rows of letters and a colour for each letter.
//!
//! A world gives each made block an id from `blocks::FIRST_MADE` up when its
//! creation first runs there (`register`, from `Scripts::start` on the host),
//! and keeps it for as long as the world lasts: a server saves the list with
//! its world, and the host sends it to everyone who joins (`table`, `adopt`).
//! The list carries each block's table as written, so a world still shows
//! its blocks when the creation that made them has gone. The game draws the
//! pictures into the block atlas as they come (`draw`).

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::blocks::{self, Block, BlockId, Cuboid, Shape, FIRST_MADE};

/// How many blocks one creation can make.
pub const MOST_EACH: usize = 16;
/// How many colours one block's pictures can use.
pub const MOST_COLORS: usize = 24;
/// How many boxes a block made of boxes can have.
pub const MOST_BOXES: usize = 16;
/// How many pixels across a picture can be.
pub const SIZES: [usize; 4] = [8, 16, 32, 64];
/// The longest name a made block can have.
const LONGEST_NAME: usize = 24;
/// The keys a block's table can have.
const KEYS: &[&str] = &["name", "shape", "kind", "hardness", "light", "glow", "colors", "picture", "top", "side", "bottom", "boxes", "solid"];

/// One made block as a world keeps it, saves it and sends it: its id here,
/// the creation that made it, its name there, and its table as written.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct Entry {
    pub id: BlockId,
    pub creation: String,
    pub key: String,
    pub spec: Value,
}

/// A picture `size` pixels square, row 0 at the top: sRGB colours with
/// alpha 0 where it's see-through, and which pixels glow.
#[derive(Clone, Debug, PartialEq)]
pub struct Picture {
    pub size: usize,
    pub rgba: Vec<[u8; 4]>,
    pub glow: Vec<bool>,
}

/// A made block, read from its table.
#[derive(Clone, Debug)]
pub struct Def {
    pub name: String,
    pub shape: Shape,
    pub hardness: f32,
    pub light: bool,
    /// What it sounds like and which tools dig it: one of `sound::DIG_KINDS`,
    /// or None to go by its name, as the game's own blocks do.
    pub kind: Option<&'static str>,
    /// Boxes in sixteenths of a block (from, to), for a block made of boxes.
    pub boxes: Vec<([u8; 3], [u8; 3])>,
    /// Whether bodies bump into a block made of boxes.
    pub solid: bool,
    /// Top, sides and bottom; `same[i]` is the first face with the same
    /// picture, which shares its tile.
    pub faces: [Picture; 3],
    pub same: [usize; 3],
}

struct Slot {
    entry: Entry,
    def: Def,
    /// The atlas tiles of its faces.
    tiles: [&'static str; 3],
}

static SLOTS: Mutex<Vec<Option<Slot>>> = Mutex::new(Vec::new());
/// Counts changes to the list, so the atlas and the network know when to
/// look again.
static GENERATION: AtomicU64 = AtomicU64::new(0);
/// Tile names, leaked once each.
static NAMES: Mutex<BTreeMap<String, &'static str>> = Mutex::new(BTreeMap::new());

fn intern(s: String) -> &'static str {
    let mut names = NAMES.lock().unwrap();
    if let Some(n) = names.get(&s) {
        return n;
    }
    let n: &'static str = Box::leak(s.clone().into_boxed_str());
    names.insert(s, n);
    n
}

fn slots() -> std::sync::MutexGuard<'static, Vec<Option<Slot>>> {
    let mut s = SLOTS.lock().unwrap_or_else(|e| e.into_inner());
    if s.is_empty() {
        s.resize_with(256 - FIRST_MADE as usize, || None);
    }
    s
}

/// Changes each time the list does.
pub fn generation() -> u64 {
    GENERATION.load(Ordering::Acquire)
}

fn title(key: &str) -> String {
    let words = key.replace('_', " ");
    let mut c = words.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

fn hex(what: &str, letter: &str, v: &Value) -> Result<[u8; 3], String> {
    let s = v.as_str().unwrap_or("");
    let h = s.strip_prefix('#').unwrap_or("");
    let digits: Vec<u8> = h.chars().filter_map(|c| c.to_digit(16).map(|d| d as u8)).collect();
    match (h.len(), digits.len()) {
        (6, 6) => Ok([digits[0] * 16 + digits[1], digits[2] * 16 + digits[3], digits[4] * 16 + digits[5]]),
        (3, 3) => Ok([digits[0] * 17, digits[1] * 17, digits[2] * 17]),
        _ => Err(format!("{what}: colors.{letter} is a colour like \"#c86432\"")),
    }
}

/// The rows of a picture, written as one string of lines or a list of them.
fn rows(what: &str, v: &Value) -> Result<Vec<Vec<char>>, String> {
    let lines: Vec<String> = match v {
        Value::String(s) => s.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect(),
        Value::Array(a) => a.iter().map(|l| l.as_str().map(|l| l.trim().to_string())).collect::<Option<Vec<_>>>().ok_or_else(|| format!("{what} is rows of letters, one string each"))?,
        _ => return Err(format!("{what} is rows of letters, one for each pixel: a string of lines, or a list of strings")),
    };
    Ok(lines.into_iter().map(|l| l.chars().collect()).collect())
}

fn picture(what: &str, v: &Value, colors: &BTreeMap<char, [u8; 3]>, glow: &[char], glow_bright: bool, clear_ok: bool) -> Result<Picture, String> {
    let rows = rows(what, v)?;
    let n = rows.len();
    if !SIZES.contains(&n) {
        return Err(format!("{what} has {n} rows; a picture is 8, 16, 32 or 64 pixels square"));
    }
    let mut rgba = Vec::with_capacity(n * n);
    let mut glows = Vec::with_capacity(n * n);
    for (y, row) in rows.iter().enumerate() {
        if row.len() != n {
            return Err(format!("{what}: row {} has {} pixels, and a picture {n} rows tall is {n} across", y + 1, row.len()));
        }
        for &c in row {
            if c == '.' {
                if !clear_ok {
                    return Err(format!("{what}: a solid block can't have see-through pixels (.); give it shape = \"see-through\""));
                }
                rgba.push([0, 0, 0, 0]);
                glows.push(false);
                continue;
            }
            let Some(&[r, g, b]) = colors.get(&c) else {
                return Err(format!("{what} uses \"{c}\", which colors doesn't give a colour"));
            };
            rgba.push([r, g, b, 255]);
            glows.push(glow.contains(&c) || (glow_bright && r as u32 + g as u32 > 300));
        }
    }
    Ok(Picture { size: n, rgba, glow: glows })
}

/// Reads one of a creation's blocks, saying plainly what's wrong.
pub fn read(key: &str, spec: &Value) -> Result<Def, String> {
    let what = format!("blocks.{key}");
    if !crate::sound::own_name_ok(key) {
        return Err(format!("{what}: a block's name in potluck.toml is lowercase letters, digits and _"));
    }
    let Value::Object(t) = spec else {
        return Err(format!("{what}: a block is a table, [blocks.{key}], with its colors and a picture"));
    };
    if let Some(k) = t.keys().find(|k| !KEYS.contains(&k.as_str())) {
        return Err(format!("{what}: there's no {k} for a block (there are: {})", KEYS.join(", ")));
    }
    let name = match t.get("name") {
        None => title(key),
        Some(Value::String(n)) => n.split_whitespace().collect::<Vec<_>>().join(" "),
        Some(_) => return Err(format!("{what}: name is what players see it called")),
    };
    if name.is_empty() || name.chars().count() > LONGEST_NAME || !name.chars().all(|c| c.is_alphanumeric() || c == ' ' || c == '-' || c == '\'') {
        return Err(format!("{what}: a block's name is 1 to {LONGEST_NAME} letters, digits and spaces"));
    }
    if blocks::BLOCKS.iter().any(|b| blocks::squash(b.name) == blocks::squash(&name)) {
        return Err(format!("{what}: the game has a block called {name} already; give yours another name"));
    }
    let boxes = match t.get("boxes") {
        None => vec![],
        Some(Value::Array(a)) => {
            if a.is_empty() || a.len() > MOST_BOXES {
                return Err(format!("{what}: boxes is 1 to {MOST_BOXES} boxes"));
            }
            let mut out = vec![];
            for (i, b) in a.iter().enumerate() {
                let n: Option<Vec<u8>> = b.as_array().map(|b| b.iter().filter_map(|v| v.as_u64().filter(|v| *v <= 16).map(|v| v as u8)).collect());
                match n {
                    Some(n) if n.len() == 6 && n[0] < n[3] && n[1] < n[4] && n[2] < n[5] => out.push(([n[0], n[1], n[2]], [n[3], n[4], n[5]])),
                    _ => return Err(format!("{what}: box {} is [x, y, z, x, y, z] in sixteenths of a block, from 0 to 16, the second corner past the first", i + 1)),
                }
            }
            out
        }
        Some(_) => return Err(format!("{what}: boxes is a list of boxes, each [x, y, z, x, y, z] in sixteenths of a block")),
    };
    let shape = match (t.get("shape").map(|s| s.as_str()), boxes.is_empty()) {
        (None | Some(Some("solid")), true) => Shape::Solid,
        (Some(Some("see-through")), true) => Shape::Cutout,
        (Some(Some("plant")), true) => Shape::Cross,
        (None | Some(Some("boxes")), false) => Shape::Model,
        (Some(Some(s)), false) if s != "boxes" => return Err(format!("{what}: a block made of boxes has no shape but its boxes")),
        _ => return Err(format!("{what}: shape is \"solid\", \"see-through\" or \"plant\" (or give it boxes)")),
    };
    let solid = match t.get("solid") {
        None => shape == Shape::Model,
        Some(Value::Bool(b)) if shape == Shape::Model => *b,
        Some(Value::Bool(_)) => return Err(format!("{what}: solid is for blocks made of boxes; the others go by their shape")),
        Some(_) => return Err(format!("{what}: solid is true or false")),
    };
    let kind = match t.get("kind") {
        None => None,
        Some(Value::String(k)) => Some(*crate::sound::DIG_KINDS.iter().find(|d| **d == k.as_str()).ok_or_else(|| format!("{what}: kind is one of {}", crate::sound::DIG_KINDS.join(", ")))?),
        Some(_) => return Err(format!("{what}: kind is one of {}", crate::sound::DIG_KINDS.join(", "))),
    };
    let hardness = match t.get("hardness") {
        None => 1.0,
        Some(v) => v.as_f64().filter(|h| (0.0..=20.0).contains(h)).ok_or_else(|| format!("{what}: hardness is seconds to break by hand, 0 to 20"))? as f32,
    };
    let light = match t.get("light") {
        None => false,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err(format!("{what}: light is true or false")),
    };
    let mut colors = BTreeMap::new();
    match t.get("colors") {
        Some(Value::Object(c)) => {
            if c.len() > MOST_COLORS {
                return Err(format!("{what}: colors gives {MOST_COLORS} colours at most"));
            }
            for (letter, v) in c {
                let mut ch = letter.chars();
                let (Some(l), None) = (ch.next(), ch.next()) else {
                    return Err(format!("{what}: each of colors is one letter, like w = \"#f0ece4\""));
                };
                if l == '.' || l.is_whitespace() {
                    return Err(format!("{what}: . is see-through, so colors can't give it a colour"));
                }
                colors.insert(l, hex(&what, letter, v)?);
            }
        }
        None => return Err(format!("{what}: a block needs colors, a colour for each letter in its pictures")),
        Some(_) => return Err(format!("{what}: colors is a table of letters and colours, like {{ w = \"#f0ece4\" }}")),
    }
    let glow: Vec<char> = match t.get("glow") {
        None => vec![],
        Some(Value::String(g)) => g.chars().filter(|c| !c.is_whitespace()).collect(),
        Some(_) => return Err(format!("{what}: glow is the letters whose pixels glow, like \"yo\"")),
    };
    if let Some(c) = glow.iter().find(|c| !colors.contains_key(c)) {
        return Err(format!("{what}: glow names \"{c}\", which colors doesn't give a colour"));
    }
    let clear_ok = shape != Shape::Solid;
    let glow_bright = light && glow.is_empty();
    let all = t.get("picture");
    let mut faces = Vec::with_capacity(3);
    let mut texts = Vec::with_capacity(3);
    for face in ["top", "side", "bottom"] {
        let Some(v) = t.get(face).or(all) else {
            return Err(format!("{what}: a block needs a picture (or a top, a side and a bottom)"));
        };
        let label = if t.contains_key(face) { format!("{what}.{face}") } else { format!("{what}.picture") };
        faces.push(picture(&label, v, &colors, &glow, glow_bright, clear_ok)?);
        texts.push(v.clone());
    }
    let same = [0, if texts[1] == texts[0] { 0 } else { 1 }, if texts[2] == texts[0] { 0 } else if texts[2] == texts[1] { 1 } else { 2 }];
    let faces: [Picture; 3] = faces.try_into().unwrap();
    Ok(Def { name, shape, hardness, light, kind, boxes, solid, faces, same })
}

/// Reads all of a creation's blocks.
pub fn read_all(made: &BTreeMap<String, Value>) -> Result<Vec<(String, Def)>, String> {
    if made.len() > MOST_EACH {
        return Err(format!("a creation makes {MOST_EACH} blocks at most, and this one makes {}", made.len()));
    }
    made.iter().map(|(k, v)| Ok((k.clone(), read(k, v)?))).collect()
}

/// Puts a block in its slot and the block table.
fn install(s: &mut [Option<Slot>], id: BlockId, entry: Entry, def: Def) {
    let base = format!("{}/{}", entry.creation, entry.key);
    let names = [intern(format!("{base}/top")), intern(format!("{base}/side")), intern(format!("{base}/bottom"))];
    let tiles = [names[def.same[0]], names[def.same[1]], names[def.same[2]]];
    let model: &'static [Cuboid] = Box::leak(def.boxes.iter().map(|(from, to)| Cuboid::new(*from, *to)).collect::<Vec<_>>().into_boxed_slice());
    let block: &'static Block = Box::leak(Box::new(Block {
        name: intern(def.name.clone()),
        faces: tiles,
        shape: def.shape,
        light: def.light,
        hardness: def.hardness,
        model,
        solid: def.solid,
    }));
    blocks::set_made(id, Some(block));
    s[(id - FIRST_MADE) as usize] = Some(Slot { entry: Entry { id, ..entry }, def, tiles });
}

/// Gives a creation's blocks ids in this world (keeping the ids they have
/// already) and their new looks. Nothing changes if any is wrong.
pub fn register(creation: &str, made: &BTreeMap<String, Value>) -> Result<Vec<BlockId>, String> {
    if made.is_empty() {
        return Ok(vec![]);
    }
    let defs = read_all(made)?;
    let mut s = slots();
    let mut ids = vec![];
    let mut taken: Vec<usize> = vec![];
    for (key, _) in &defs {
        let at = s.iter().position(|x| x.as_ref().is_some_and(|x| x.entry.creation == creation && x.entry.key == *key));
        let at = match at {
            Some(i) => i,
            None => (0..s.len()).find(|i| s[*i].is_none() && !taken.contains(i)).ok_or_else(|| format!("this world has {} blocks from creations already, and that's as many as it holds", s.len()))?,
        };
        taken.push(at);
        ids.push(FIRST_MADE + at as BlockId);
    }
    let mut changed = false;
    for ((key, def), &id) in defs.into_iter().zip(&ids) {
        let entry = Entry { id, creation: creation.to_string(), key: key.clone(), spec: made[&key].clone() };
        if s[(id - FIRST_MADE) as usize].as_ref().is_some_and(|x| x.entry == entry) {
            continue;
        }
        install(&mut s, id, entry, def);
        changed = true;
    }
    if changed {
        GENERATION.fetch_add(1, Ordering::AcqRel);
    }
    Ok(ids)
}

/// Every made block in this world, for saving and sending.
pub fn table() -> Vec<Entry> {
    slots().iter().flatten().map(|x| x.entry.clone()).collect()
}

/// Takes a world's made blocks as its host gave them, at exactly their ids
/// (a guest's world is the host's). Entries that don't read are left out.
pub fn adopt(entries: &[Entry]) {
    let mut s = slots();
    let mut changed = false;
    for e in entries.iter().filter(|e| e.id >= FIRST_MADE) {
        if s[(e.id - FIRST_MADE) as usize].as_ref().is_some_and(|x| x.entry == *e) {
            continue;
        }
        let Ok(def) = read(&e.key, &e.spec) else { continue };
        // the same block somewhere else is the old world's
        for (i, x) in s.iter_mut().enumerate() {
            if x.as_ref().is_some_and(|x| x.entry.creation == e.creation && x.entry.key == e.key) && i != (e.id - FIRST_MADE) as usize {
                *x = None;
                blocks::set_made(FIRST_MADE + i as BlockId, None);
            }
        }
        install(&mut s, e.id, e.clone(), def);
        changed = true;
    }
    if changed {
        GENERATION.fetch_add(1, Ordering::AcqRel);
    }
}

/// Takes a saved world's made blocks, keeping their ids where they're free
/// (or already theirs). Returns new ids for the saved world's blocks where
/// another block had theirs, for `Voxels::renumber`.
pub fn restore(entries: &[Entry]) -> Option<[BlockId; 256]> {
    let mut to: [BlockId; 256] = std::array::from_fn(|i| i as BlockId);
    let mut moved = false;
    let mut s = slots();
    for e in entries.iter().filter(|e| e.id >= FIRST_MADE) {
        let Ok(def) = read(&e.key, &e.spec) else { continue };
        let mine = |x: &Option<Slot>| x.as_ref().is_some_and(|x| x.entry.creation == e.creation && x.entry.key == e.key);
        let at = s.iter().position(mine).or_else(|| s[(e.id - FIRST_MADE) as usize].is_none().then_some((e.id - FIRST_MADE) as usize)).or_else(|| s.iter().position(|x| x.is_none()));
        let Some(at) = at else { continue };
        let id = FIRST_MADE + at as BlockId;
        if id != e.id {
            to[e.id as usize] = id;
            moved = true;
        }
        if s[at].is_none() {
            install(&mut s, id, Entry { id, ..e.clone() }, def);
        }
    }
    drop(s);
    GENERATION.fetch_add(1, Ordering::AcqRel);
    moved.then_some(to)
}

/// A saved world: `POTLUCKW3`, the seed, the blocks creations made there
/// (`made::Entry`s as JSON, after its length) and the columns anyone
/// changed (`Voxels::encode`; the rest come from the seed). Worlds saved
/// before the world went on past the island start `POTLUCKW2` and hold the
/// whole island; before creations made blocks, `POTLUCKW1` with no list.
pub fn world_bytes(voxels: &crate::world::Voxels) -> Vec<u8> {
    let made = serde_json::to_vec(&table()).unwrap_or_else(|_| b"[]".to_vec());
    let mut bytes = b"POTLUCKW3".to_vec();
    bytes.extend_from_slice(&voxels.seed.to_le_bytes());
    bytes.extend_from_slice(&(made.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&made);
    bytes.extend_from_slice(&voxels.encode());
    bytes
}

/// Reads a saved world, taking back the blocks creations made in it (with
/// new ids where theirs are taken).
pub fn read_world(bytes: &[u8]) -> Option<crate::world::Voxels> {
    if bytes.len() <= 13 || !bytes.starts_with(b"POTLUCKW") {
        return None;
    }
    let seed = u32::from_le_bytes([bytes[9], bytes[10], bytes[11], bytes[12]]);
    match bytes[8] {
        b'1' => crate::world::Voxels::decode_island(seed, &bytes[13..]),
        b'2' | b'3' => {
            let n = u32::from_le_bytes(bytes.get(13..17)?.try_into().ok()?) as usize;
            let made: Vec<Entry> = serde_json::from_slice(bytes.get(17..17 + n)?).ok()?;
            let rest = bytes.get(17 + n..)?;
            let mut v = if bytes[8] == b'2' { crate::world::Voxels::decode_island(seed, rest)? } else { crate::world::Voxels::decode(seed, rest)? };
            if let Some(to) = restore(&made) {
                v.renumber(&to);
            }
            Some(v)
        }
        _ => None,
    }
}

/// What a made block sounds like and which tools dig it, if its table says.
pub fn kind(id: BlockId) -> Option<&'static str> {
    if id < FIRST_MADE {
        return None;
    }
    slots()[(id - FIRST_MADE) as usize].as_ref().and_then(|x| x.def.kind)
}

/// A made block's colour from above, for the map: its top's pixels that
/// aren't see-through, averaged.
pub fn color(id: BlockId) -> Option<[u8; 3]> {
    if id < FIRST_MADE {
        return None;
    }
    let s = slots();
    let p = &s[(id - FIRST_MADE) as usize].as_ref()?.def.faces[0];
    let (mut sum, mut n) = ([0u32; 3], 0u32);
    for c in p.rgba.iter().filter(|c| c[3] > 0) {
        for k in 0..3 {
            sum[k] += c[k] as u32;
        }
        n += 1;
    }
    (n > 0).then(|| sum.map(|v| (v / n) as u8))
}

/// Draws made blocks into the atlas (`draw`) and sends the tiles it draws
/// to the graphics card: the game's, not the server's.
pub struct MadePlugin;

impl Plugin for MadePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, draw);
        if let Some(render) = app.get_sub_app_mut(bevy::render::RenderApp) {
            render
                .init_resource::<Writes>()
                .add_systems(bevy::render::ExtractSchedule, take_writes)
                .add_systems(bevy::render::Render, send_writes.in_set(bevy::render::RenderSystems::PrepareResources));
        }
    }
}

/// Tiles on their way to the graphics card.
#[derive(Resource, Default)]
struct Writes(Vec<crate::textures::TileWrite>);

fn take_writes(mut main: ResMut<bevy::render::MainWorld>, mut writes: ResMut<Writes>) {
    if main.get_resource::<crate::textures::BlockAtlas>().is_some_and(|a| !a.writes.is_empty()) {
        writes.0.append(&mut main.resource_mut::<crate::textures::BlockAtlas>().writes);
    }
}

/// Draws each tile into its atlas image once the image is on the card.
fn send_writes(mut writes: ResMut<Writes>, gpu: Res<bevy::render::render_asset::RenderAssets<bevy::render::texture::GpuImage>>, queue: Res<bevy::render::renderer::RenderQueue>) {
    use bevy::render::render_resource::{Extent3d, Origin3d, TexelCopyBufferLayout, TexelCopyTextureInfo, TextureAspect};
    writes.0.retain(|w| {
        let Some(img) = gpu.get(w.image) else { return true };
        queue.write_texture(
            TexelCopyTextureInfo { texture: &img.texture, mip_level: w.level, origin: Origin3d { x: w.x, y: w.y, z: 0 }, aspect: TextureAspect::All },
            &w.data,
            TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w.size * 4), rows_per_image: None },
            Extent3d { width: w.size, height: w.size, depth_or_array_layers: 1 },
        );
        false
    });
}

/// Draws made blocks' pictures into the block atlas when the list changes,
/// gives the map their colours, and has the chunks with them drawn again
/// (and their lights found).
pub fn draw(
    atlas: Option<ResMut<crate::textures::BlockAtlas>>,
    mut images: ResMut<Assets<Image>>,
    mut voxels: ResMut<crate::world::Voxels>,
    mut map: Option<ResMut<crate::map::MapImage>>,
    mut seen: Local<(u64, u64)>,
) {
    let (g, world) = (generation(), voxels.id);
    let Some(mut atlas) = atlas.filter(|_| *seen != (g, world)) else { return };
    let fresh = seen.0 != g;
    *seen = (g, world);
    let mut full = false;
    let mut made = vec![];
    {
        let s = slots();
        for x in s.iter().flatten() {
            made.push((x.entry.id, x.def.light));
            if !fresh {
                continue;
            }
            for (i, face) in x.def.faces.iter().enumerate() {
                if x.def.same[i] == i && atlas.put(&mut images, x.tiles[i], &face.rgba, face.size, &face.glow).is_none() {
                    full = true;
                }
            }
        }
    }
    if full {
        warn!("The block atlas is full; some blocks creations made show as missing");
    }
    if let Some(map) = map.as_mut() {
        for (id, _) in &made {
            if let Some(c) = color(*id) {
                map.palette.0[*id as usize] = c;
            }
        }
    }
    // the chunks and columns with made blocks in them, and their lights
    let mut chunks = std::collections::HashSet::new();
    let mut columns = std::collections::HashSet::new();
    let mut lights = vec![];
    let size = IVec3::splat(crate::world::CHUNK);
    voxels.each_from(FIRST_MADE, |p, id| {
        let c = p.div_euclid(size);
        chunks.insert(c);
        // and the chunk next door, for its faces against this block
        for d in blocks::SIDES {
            let n = (p + IVec3::from_array(d)).div_euclid(size);
            if n != c && (crate::world::CY0..crate::world::CY1).contains(&n.y) {
                chunks.insert(n);
            }
        }
        columns.insert(IVec2::new(p.x, p.z));
        if blocks::block(id).light {
            lights.push(p);
        }
    });
    let v = &mut *voxels;
    let dark: Vec<IVec3> = v.lanterns.iter().filter(|p| !blocks::block(v.at(**p)).light).copied().collect();
    for p in dark {
        v.lanterns.remove(&p);
    }
    v.lanterns.extend(lights);
    if fresh {
        v.dirty.extend(chunks);
        v.map_dirty.extend(columns);
    }
}
