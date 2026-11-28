//! Items made by creations (world 0.25): `[items.lamp_oil]` in a creation's
//! potluck.toml, a picture in letters like a block's, and what kind of thing
//! it is.
//!
//! A world gives each one an id from `items::FIRST_MADE_ITEM` up when its
//! creation starts (`register`, from `Scripts::start` on the host, before its
//! code runs, so the code can use it by name), and the host sends the list to
//! everyone who joins and whenever it changes (`table`, `adopt`). Pockets,
//! stores and creations all keep things by name, so a kept world needs no
//! list of its own: its creations make their items again before the player's
//! pockets come back. The game draws the pictures into the block atlas as
//! they come (`draw`), and an item's icon is its picture.
//!
//! What an item does is up to the creations that know its kind: Hunger eats
//! `food`, Armor wears `armor` by its `slot` and `guard`, wild creatures take
//! a `weapon`'s `power` as a sword's, the Workbench makes anything with a
//! `recipe`, and a `tool` digs as its `digs` says, which the engine gives
//! the making creation as its `world.digging`. Any creation reads the table
//! with `world.item(name)`.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{colors_of, intern, picture, title, Picture, LONGEST_NAME};
use crate::items::{self, Digging, Item, Thing, FIRST_MADE_ITEM, MADE_ITEMS};

/// How many items one creation can make.
pub const MOST_EACH: usize = 32;
/// What an item can be. Material is anything else: what's made into
/// something, traded or kept.
pub const KINDS: [&str; 5] = ["material", "food", "tool", "weapon", "armor"];
/// Where armor goes: `script::WEAR_SLOTS`.
pub const SLOTS: [&str; 5] = crate::script::WEAR_SLOTS;
/// The most a food fills, a piece of armor guards and a weapon bops.
pub const MOST_FOOD: f64 = 20.0;
pub const MOST_GUARD: f64 = 10.0;
pub const MOST_POWER: f64 = 20.0;

/// The most things a recipe takes, and of each.
pub const RECIPE_THINGS: usize = 6;
pub const RECIPE_MOST: f64 = 64.0;

const KEYS: &[&str] = &["name", "kind", "stack", "colors", "picture", "recipe", "makes", "food", "says", "slot", "guard", "warm", "digs", "power"];

/// One made item as a world keeps it and sends it: its id here, the creation
/// that made it, its name there, and its table as written.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct Entry {
    pub id: Thing,
    pub creation: String,
    pub key: String,
    pub spec: Value,
}

/// A made item, read from its table.
#[derive(Clone, Debug)]
pub struct Def {
    pub name: String,
    pub kind: &'static str,
    pub stack: u32,
    pub picture: Picture,
    /// A tool's digging (`digs`), as `world.digging` takes it.
    pub digging: Option<Digging>,
}

struct Slot {
    entry: Entry,
    def: Def,
    /// The atlas tile of its picture.
    tile: &'static str,
}

static SLOTS_MADE: Mutex<Vec<Option<Slot>>> = Mutex::new(Vec::new());
/// Counts changes to the list, so the atlas and the network know when to
/// look again.
static GENERATION: AtomicU64 = AtomicU64::new(0);

fn slots() -> std::sync::MutexGuard<'static, Vec<Option<Slot>>> {
    let mut s = SLOTS_MADE.lock().unwrap_or_else(|e| e.into_inner());
    if s.is_empty() {
        s.resize_with(MADE_ITEMS, || None);
    }
    s
}

/// Changes each time the list does.
pub fn generation() -> u64 {
    GENERATION.load(Ordering::Acquire)
}

fn number(what: &str, key: &str, v: &Value, lo: f64, hi: f64, says: &str) -> Result<f64, String> {
    v.as_f64().filter(|n| n.is_finite() && (lo..=hi).contains(n)).ok_or_else(|| format!("{what}: {key} is {says}, {lo} to {hi}"))
}

/// A tool's `digs`: speeds by kind of block and a level, as `world.digging`
/// takes them (`{ hard = 4, level = 3 }`).
fn digs(what: &str, v: &Value) -> Result<Digging, String> {
    let Value::Object(t) = v else {
        return Err(format!("{what}: digs is speeds by kind of block and a level, like {{ hard = 4, level = 3 }}"));
    };
    let mut d = Digging::default();
    for (kind, n) in t {
        if kind == "level" {
            let n = number(what, "digs.level", n, 0.0, items::MAX_LEVEL as f64, "a whole number")?;
            if n.fract() != 0.0 {
                return Err(format!("{what}: digs.level is a whole number, 0 to {}", items::MAX_LEVEL));
            }
            d.level = n as u8;
            continue;
        }
        let Some(k) = crate::sound::DIG_KINDS.iter().position(|d| *d == kind) else {
            return Err(format!("{what}: digs knows {} and level, not \"{kind}\"", crate::sound::DIG_KINDS.join(", ")));
        };
        d.speeds[k] = number(what, &format!("digs.{kind}"), n, 0.0, crate::script::MAX_DIGGING, "how fast it digs (1 is as a hand does)")? as f32;
    }
    Ok(d)
}

/// Reads one of a creation's items, saying plainly what's wrong.
pub fn read(key: &str, spec: &Value) -> Result<Def, String> {
    let what = format!("items.{key}");
    if !crate::sound::own_name_ok(key) {
        return Err(format!("{what}: an item's name in potluck.toml is lowercase letters, digits and _"));
    }
    let Value::Object(t) = spec else {
        return Err(format!("{what}: an item is a table, [items.{key}], with its colors and a picture"));
    };
    if let Some(k) = t.keys().find(|k| !KEYS.contains(&k.as_str())) {
        return Err(format!("{what}: there's no {k} for an item (there are: {})", KEYS.join(", ")));
    }
    let name = match t.get("name") {
        None => title(key),
        Some(Value::String(n)) => n.split_whitespace().collect::<Vec<_>>().join(" "),
        Some(_) => return Err(format!("{what}: name is what players see it called")),
    };
    if name.is_empty() || name.chars().count() > LONGEST_NAME || !name.chars().all(|c| c.is_alphanumeric() || c == ' ' || c == '-' || c == '\'') {
        return Err(format!("{what}: an item's name is 1 to {LONGEST_NAME} letters, digits and spaces"));
    }
    let squashed = crate::blocks::squash(&name);
    if crate::blocks::BLOCKS.iter().any(|b| crate::blocks::squash(b.name) == squashed) || items::ITEMS.iter().any(|i| crate::blocks::squash(i.name) == squashed) {
        return Err(format!("{what}: the game has something called {name} already; give yours another name"));
    }
    let kind = match t.get("kind") {
        None => "material",
        Some(Value::String(k)) => KINDS.iter().find(|x| **x == k.as_str()).copied().ok_or_else(|| format!("{what}: kind is one of {}", KINDS.join(", ")))?,
        Some(_) => return Err(format!("{what}: kind is one of {}", KINDS.join(", "))),
    };
    // what belongs to which kind
    for (field, of) in [("food", "food"), ("says", "food"), ("slot", "armor"), ("guard", "armor"), ("warm", "armor"), ("digs", "tool"), ("power", "weapon")] {
        if t.contains_key(field) && kind != of {
            return Err(format!("{what}: {field} is for an item of kind = \"{of}\""));
        }
    }
    let stack = match t.get("stack") {
        None if matches!(kind, "material" | "food") => items::BLOCK_STACK,
        None => 1,
        Some(v) => {
            let n = number(&what, "stack", v, 1.0, items::BLOCK_STACK as f64, "how many go in one slot")?;
            if n.fract() != 0.0 {
                return Err(format!("{what}: stack is a whole number, 1 to {}", items::BLOCK_STACK));
            }
            n as u32
        }
    };
    if let Some(v) = t.get("food") {
        number(&what, "food", v, 1.0, MOST_FOOD, "how much it fills")?;
    }
    match t.get("says") {
        Some(Value::String(s)) if !s.trim().is_empty() && s.chars().count() <= 80 => {}
        None => {}
        Some(_) => return Err(format!("{what}: says is what a player is told when they eat it, 80 letters at most")),
    }
    // what a workbench makes it from (the Workbench creation reads it)
    match t.get("recipe") {
        None => {
            if t.contains_key("makes") {
                return Err(format!("{what}: makes is how many a recipe makes; give it a recipe too"));
            }
        }
        Some(Value::Object(r)) if !r.is_empty() && r.len() <= RECIPE_THINGS => {
            for (thing, n) in r {
                if thing.trim().is_empty() || thing.chars().count() > 40 {
                    return Err(format!("{what}: a recipe takes things by name, like {{ Planks = 2 }}"));
                }
                let n = number(&what, &format!("recipe's {thing}"), n, 1.0, RECIPE_MOST, "how many it takes")?;
                if n.fract() != 0.0 {
                    return Err(format!("{what}: a recipe takes whole things, 1 to {RECIPE_MOST}"));
                }
            }
        }
        Some(_) => return Err(format!("{what}: recipe is what it's made from, 1 to {RECIPE_THINGS} things by name and how many, like {{ Planks = 2, Coal = 1 }}")),
    }
    if let Some(v) = t.get("makes") {
        let n = number(&what, "makes", v, 1.0, stack as f64, "how many a recipe makes, up to a stack")?;
        if n.fract() != 0.0 {
            return Err(format!("{what}: makes is a whole number"));
        }
    }
    if kind == "armor" {
        match t.get("slot") {
            Some(Value::String(s)) if SLOTS.contains(&s.as_str()) => {}
            _ => return Err(format!("{what}: armor needs a slot, where it's worn: {}", SLOTS.join(", "))),
        }
    }
    if let Some(v) = t.get("guard") {
        number(&what, "guard", v, 0.0, MOST_GUARD, "how much it guards")?;
    }
    if t.get("warm").is_some_and(|v| !v.is_boolean()) {
        return Err(format!("{what}: warm is true or false"));
    }
    if let Some(v) = t.get("power") {
        number(&what, "power", v, 0.0, MOST_POWER, "how hard it bops (a hand bops 1)")?;
    }
    let digging = t.get("digs").map(|v| digs(&what, v)).transpose()?;
    let colors = colors_of(&what, t.get("colors"), "an item")?;
    let Some(p) = t.get("picture") else {
        return Err(format!("{what}: an item needs a picture, its icon"));
    };
    let picture = picture(&format!("{what}.picture"), p, &colors, &[], false, true)?;
    Ok(Def { name, kind, stack, picture, digging })
}

pub fn read_all(made: &BTreeMap<String, Value>) -> Result<Vec<(String, Def)>, String> {
    if made.len() > MOST_EACH {
        return Err(format!("a creation makes {MOST_EACH} items at most, and this one makes {}", made.len()));
    }
    made.iter().map(|(k, v)| Ok((k.clone(), read(k, v)?))).collect()
}

fn install(s: &mut [Option<Slot>], id: Thing, entry: Entry, def: Def) {
    let tile = intern(format!("{}/{}/item", entry.creation, entry.key));
    let item: &'static Item = Box::leak(Box::new(Item { name: intern(def.name.clone()), icon: tile, stack: def.stack }));
    items::set_made_item(id, Some(item));
    s[(id - FIRST_MADE_ITEM) as usize] = Some(Slot { entry: Entry { id, ..entry }, def, tile });
}

/// Gives a creation's items ids in this world (keeping the ids they have
/// already) and their new looks. Nothing changes if any is wrong.
pub fn register(creation: &str, made: &BTreeMap<String, Value>) -> Result<Vec<Thing>, String> {
    if made.is_empty() {
        return Ok(vec![]);
    }
    let defs = read_all(made)?;
    let mut s = slots();
    let mut ids = vec![];
    for (key, _) in &defs {
        let at = s.iter().position(|x| x.as_ref().is_some_and(|x| x.entry.creation == creation && x.entry.key == *key));
        let at = match at {
            Some(i) => i,
            None => (0..s.len())
                .find(|i| s[*i].is_none() && !ids.contains(&(FIRST_MADE_ITEM + *i as Thing)))
                .ok_or_else(|| format!("this world has {} items from creations already, and that's as many as it holds", s.len()))?,
        };
        ids.push(FIRST_MADE_ITEM + at as Thing);
    }
    let mut changed = false;
    for ((key, def), &id) in defs.into_iter().zip(&ids) {
        let entry = Entry { id, creation: creation.to_string(), spec: made[&key].clone(), key };
        if s[(id - FIRST_MADE_ITEM) as usize].as_ref().is_some_and(|x| x.entry == entry) {
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

/// Every made item in this world, for sending.
pub fn table() -> Vec<Entry> {
    slots().iter().flatten().map(|x| x.entry.clone()).collect()
}

/// Takes a world's made items as its host gave them, at exactly their ids
/// (a guest's world is the host's). Entries that don't read are left out.
pub fn adopt(entries: &[Entry]) {
    let mut s = slots();
    let mut changed = false;
    for e in entries.iter().filter(|e| e.id >= FIRST_MADE_ITEM && ((e.id - FIRST_MADE_ITEM) as usize) < MADE_ITEMS) {
        if s[(e.id - FIRST_MADE_ITEM) as usize].as_ref().is_some_and(|x| x.entry == *e) {
            continue;
        }
        let Ok(def) = read(&e.key, &e.spec) else { continue };
        // the same item somewhere else is the old world's
        for (i, x) in s.iter_mut().enumerate() {
            if x.as_ref().is_some_and(|x| x.entry.creation == e.creation && x.entry.key == e.key) && i != (e.id - FIRST_MADE_ITEM) as usize {
                *x = None;
                items::set_made_item(FIRST_MADE_ITEM + i as Thing, None);
            }
        }
        install(&mut s, e.id, e.clone(), def);
        changed = true;
    }
    if changed {
        GENERATION.fetch_add(1, Ordering::AcqRel);
    }
}

/// A made item's table for creations (`world.item`): what was written, with
/// its name, kind and stack filled in and the creation that made it as `by`.
pub fn info(t: Thing) -> Option<(Value, &'static str)> {
    let s = slots();
    let x = s.get(t.checked_sub(FIRST_MADE_ITEM)? as usize)?.as_ref()?;
    let mut v = match &x.entry.spec {
        Value::Object(o) => o.clone(),
        _ => Default::default(),
    };
    v.remove("colors");
    v.remove("picture");
    v.insert("name".into(), Value::String(x.def.name.clone()));
    v.insert("kind".into(), Value::String(x.def.kind.into()));
    v.insert("stack".into(), Value::from(x.def.stack));
    v.insert("by".into(), Value::String(x.entry.creation.clone()));
    Some((Value::Object(v), x.def.kind))
}

/// The names of the items creations made in this world.
pub fn names() -> Vec<&'static str> {
    slots().iter().flatten().filter_map(|x| items::made_item(x.entry.id)).map(|i| i.name).collect()
}

/// A creation's tools, with how each digs: the engine gives these to the
/// making creation as its `world.digging`.
pub fn diggers(creation: &str) -> Vec<(Thing, Digging)> {
    slots().iter().flatten().filter(|x| x.entry.creation == creation).filter_map(|x| Some((x.entry.id, x.def.digging?))).collect()
}

/// Draws made items' pictures into the atlas as their icons: the game's,
/// not the server's (`super::MadePlugin`).
pub fn draw(atlas: Option<ResMut<crate::textures::BlockAtlas>>, mut images: ResMut<Assets<Image>>, mut seen: Local<u64>) {
    let g = generation();
    let Some(mut atlas) = atlas.filter(|_| *seen != g) else { return };
    *seen = g;
    let mut full = false;
    for x in slots().iter().flatten() {
        let p = &x.def.picture;
        if atlas.put(&mut images, x.tile, &p.rgba, p.size, &p.glow).is_none() {
            full = true;
        }
    }
    if full {
        warn!("The block atlas is full; some items creations made show as missing");
    }
}
