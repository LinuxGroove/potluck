//! The automatic checks every version of every creation goes through before
//! anyone else can load it. A failed check holds that version back (only its
//! maker sees it, with the reason); a warning lets it through but puts it in
//! front of a person.

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};

/// What's in a creation's potluck.toml (the game's `creations::Manifest`).
#[derive(Deserialize, Serialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Toml {
    pub name: String,
    pub by: String,
    pub kind: String,
    pub about: String,
    pub main: String,
    pub version: u32,
    pub remix_of: Option<String>,
    pub licence: String,
    pub needs: BTreeMap<String, String>,
    /// Its buttons (`@potluck/input`): `[actions.buck]`.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub actions: BTreeMap<String, Action>,
    /// Its client half's file (`@potluck/net`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client: Option<String>,
    /// Sounds of its own (`@potluck/sound` 0.2): `[sounds.moo]`.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub sounds: BTreeMap<String, toml::Value>,
    /// Blocks of its own (world 0.12): `[blocks.marble]`.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub blocks: BTreeMap<String, toml::Value>,
}

#[derive(Deserialize, Serialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Action {
    pub name: String,
    pub pad: String,
    pub keys: String,
}

/// The pad buttons and keys an action may ask for. Keep in step with
/// `PAD_BUTTONS` and the keys in the game's input.rs.
pub const PAD_BUTTONS: &[&str] = &["A", "X", "Y", "LB", "RB", "LT", "RT", "Up", "Down", "Left", "Right"];
pub const MAX_ACTIONS: usize = 8;

fn key_ok(k: &str) -> bool {
    k == "Space" || (k.len() == 1 && k.chars().all(|c| c.is_ascii_uppercase()) && !"WASD".contains(k))
}

/// What's wrong with a creation's actions, and the ones with no pad button.
pub fn action_problems(actions: &BTreeMap<String, Action>) -> (Vec<String>, Vec<String>) {
    let mut problems = vec![];
    if actions.len() > MAX_ACTIONS {
        problems.push(format!("it has {} actions, and the most is {MAX_ACTIONS}", actions.len()));
    }
    for (id, a) in actions {
        if id.is_empty() || id.len() > 24 || !id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_') {
            problems.push(format!("an action's id is 1 to 24 small letters, numbers or _ (not \"{id}\")"));
        } else if a.name.trim().is_empty() || a.name.chars().count() > 24 {
            problems.push(format!("action {id} needs a name of 1 to 24 letters"));
        } else if a.pad.is_empty() && a.keys.is_empty() {
            problems.push(format!("action {id} needs a pad button or a key"));
        } else if !a.pad.is_empty() && !PAD_BUTTONS.contains(&a.pad.as_str()) {
            problems.push(format!("action {id} can't have the pad's {} (it can have: {})", a.pad, PAD_BUTTONS.join(", ")));
        } else if !a.keys.is_empty() && !key_ok(&a.keys) {
            problems.push(format!("action {id} can't have the {} key (it can have Space or a letter but W, A, S and D)", a.keys));
        }
    }
    let no_pad = actions.iter().filter(|(_, a)| a.pad.is_empty()).map(|(id, _)| id.clone()).collect();
    (problems, no_pad)
}

/// What a creation's own sounds may say. Keep in step with `RECIPE_KEYS`,
/// `LAYER_KEYS`, `WAVES` and the limits in the game's sound.rs.
pub const SOUND_KEYS: &[&str] = &["caption", "wave", "pitch", "notes", "each", "length", "attack", "decay", "wobble", "pulse", "lowpass", "highpass", "layers"];
pub const LAYER_KEYS: &[&str] = &["wave", "pitch", "notes", "each", "length", "attack", "decay", "wobble", "pulse", "lowpass", "highpass", "at", "gain"];
pub const WAVES: &[&str] = &["sine", "triangle", "saw", "square", "noise"];
pub const MAX_SOUNDS: usize = 16;

fn note_ok(n: &toml::Value) -> bool {
    match n {
        toml::Value::Integer(_) | toml::Value::Float(_) => true,
        toml::Value::String(s) => {
            let s = s.trim();
            let mut c = s.chars();
            s == "-"
                || (c.next().is_some_and(|l| "CDEFGABcdefgab".contains(l)) && {
                    let rest: String = c.collect();
                    let rest = rest.strip_prefix(['#', 'b']).unwrap_or(&rest);
                    rest.is_empty() || rest.parse::<i32>().is_ok_and(|o| (0..=9).contains(&o))
                })
        }
        _ => false,
    }
}

fn layer_problems(what: &str, t: &toml::Table, keys: &[&str], out: &mut Vec<String>) {
    for k in t.keys() {
        if !keys.contains(&k.as_str()) {
            out.push(format!("{what} has \"{k}\", which sounds don't (they have {})", keys.join(", ")));
        }
    }
    if let Some(w) = t.get("wave") {
        if !w.as_str().is_some_and(|w| WAVES.contains(&w)) {
            out.push(format!("{what}'s wave is one of {}", WAVES.join(", ")));
        }
    }
    if let Some(n) = t.get("notes") {
        match n.as_array() {
            Some(a) if a.len() > 32 => out.push(format!("{what} has more than 32 notes")),
            Some(a) if a.iter().all(note_ok) => {}
            _ => out.push(format!("{what}'s notes are a list of names like \"C5\" or \"F#4\", \"-\" for a rest, or numbers")),
        }
    }
}

/// The most cubes of a model a body uses (the game's `model::MAX_PARTS`).
pub const MAX_MODEL_CUBES: usize = 64;

/// How many cubes a Blockbench model has, as the game's `model::read` counts
/// them (visible cubes with a `from` and a `to`), or why it isn't one.
pub fn model_cubes(text: &str) -> Result<usize, String> {
    let v: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("isn't a Blockbench model: {e}"))?;
    let elements = v.get("elements").and_then(|e| e.as_array()).ok_or("has no cubes in it (is it a Blockbench .bbmodel?)")?;
    Ok(elements
        .iter()
        .filter(|e| e.get("type").and_then(|t| t.as_str()).unwrap_or("cube") == "cube")
        .filter(|e| e.get("visibility").and_then(|v| v.as_bool()) != Some(false))
        .filter(|e| e.get("from").and_then(|f| f.as_array()).is_some_and(|f| f.len() == 3) && e.get("to").and_then(|t| t.as_array()).is_some_and(|t| t.len() == 3))
        .count())
}

/// The names in a model (its own, its cubes', groups' and textures'), which
/// are all of it a person reads: the rest is numbers and pictures.
fn model_names(text: &str) -> String {
    fn names(v: &serde_json::Value, out: &mut Vec<String>) {
        match v {
            serde_json::Value::Object(o) => {
                if let Some(n) = o.get("name").and_then(|n| n.as_str()) {
                    out.push(n.to_string());
                }
                for (k, c) in o {
                    if k != "source" {
                        names(c, out);
                    }
                }
            }
            serde_json::Value::Array(a) => a.iter().for_each(|c| names(c, out)),
            _ => {}
        }
    }
    let mut out = vec![];
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(text) {
        names(&v, &mut out);
    }
    out.join(" ")
}

/// What's wrong with a creation's own sounds.
pub fn sound_problems(sounds: &BTreeMap<String, toml::Value>) -> Vec<String> {
    let mut out = vec![];
    if sounds.len() > MAX_SOUNDS {
        out.push(format!("it has {} sounds of its own, and the most is {MAX_SOUNDS}", sounds.len()));
    }
    for (name, v) in sounds {
        if name.is_empty() || name.len() > 24 || !name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_') {
            out.push(format!("a sound's name is 1 to 24 small letters, numbers or _ (not \"{name}\")"));
            continue;
        }
        let what = format!("sound {name}");
        let Some(t) = v.as_table() else {
            out.push(format!("{what} is a table, [sounds.{name}]"));
            continue;
        };
        layer_problems(&what, t, SOUND_KEYS, &mut out);
        if let Some(l) = t.get("layers") {
            match l.as_array() {
                Some(a) if a.len() > 3 => out.push(format!("{what} has more than 3 layers")),
                Some(a) => {
                    for (i, l) in a.iter().enumerate() {
                        match l.as_table() {
                            Some(l) => layer_problems(&format!("{what}'s layer {}", i + 1), l, LAYER_KEYS, &mut out),
                            None => out.push(format!("{what}'s layers are tables, [[sounds.{name}.layers]]")),
                        }
                    }
                }
                None => out.push(format!("{what}'s layers are a list, [[sounds.{name}.layers]]")),
            }
        }
    }
    out
}

/// The most blocks a creation can make, and the most boxes in one (the
/// game's `made`).
pub const MAX_BLOCKS: usize = 16;
const MAX_BLOCK_BOXES: usize = 16;
/// A moving block's pictures, its own included (the game's `made::MOST_FRAMES`).
const MAX_BLOCK_FRAMES: usize = 8;

/// A block's picture: rows of letters, as one string of lines or a list.
fn picture_rows(v: &toml::Value) -> Option<Vec<String>> {
    match v {
        toml::Value::String(s) => Some(s.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect()),
        toml::Value::Array(a) => a.iter().map(|l| l.as_str().map(|l| l.trim().to_string())).collect(),
        _ => None,
    }
}

/// What players see a block called: its `name`, or its key as the game
/// reads it (`red_bricks` is "Red bricks").
pub fn block_name(key: &str, b: &toml::Value) -> String {
    if let Some(n) = b.get("name").and_then(|n| n.as_str()) {
        return n.to_string();
    }
    let words = key.replace('_', " ");
    let mut c = words.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

/// What's wrong with a creation's own blocks, read as the game reads them
/// (the game says more when it starts one).
pub fn block_problems(blocks: &BTreeMap<String, toml::Value>) -> Vec<String> {
    let mut out = vec![];
    if blocks.len() > MAX_BLOCKS {
        out.push(format!("it makes {} blocks, and the most is {MAX_BLOCKS}", blocks.len()));
    }
    for (key, v) in blocks {
        if key.is_empty() || key.len() > 24 || !key.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_') {
            out.push(format!("a block's name in potluck.toml is 1 to 24 small letters, numbers or _ (not \"{key}\")"));
            continue;
        }
        let what = format!("block {key}");
        let Some(t) = v.as_table() else {
            out.push(format!("{what} is a table, [blocks.{key}]"));
            continue;
        };
        let colors: Vec<char> = match t.get("colors").and_then(|c| c.as_table()) {
            Some(c) => {
                for (letter, hex) in c {
                    let h = hex.as_str().and_then(|h| h.strip_prefix('#')).unwrap_or("");
                    if letter.chars().count() != 1 || letter == "." || !(h.len() == 3 || h.len() == 6) || !h.chars().all(|c| c.is_ascii_hexdigit()) {
                        out.push(format!("{what}'s colors are one letter each, and a colour like \"#c86432\""));
                        break;
                    }
                }
                c.keys().filter_map(|k| k.chars().next()).collect()
            }
            None => {
                out.push(format!("{what} needs colors, a colour for each letter in its pictures"));
                continue;
            }
        };
        let faces: Vec<&str> = ["picture", "top", "side", "bottom"].into_iter().filter(|f| t.contains_key(*f)).collect();
        if !faces.contains(&"picture") && faces.len() < 3 {
            out.push(format!("{what} needs a picture (or a top, a side and a bottom)"));
        }
        let check_face = |out: &mut Vec<String>, face: &str, v: &toml::Value| {
            let Some(rows) = picture_rows(v) else {
                out.push(format!("{what}'s {face} is rows of letters"));
                return;
            };
            let n = rows.len();
            if ![8, 16, 32, 64].contains(&n) || rows.iter().any(|r| r.chars().count() != n) {
                out.push(format!("{what}'s {face} isn't square, 8, 16, 32 or 64 pixels across"));
            } else if let Some(c) = rows.iter().flat_map(|r| r.chars()).find(|c| *c != '.' && !colors.contains(c)) {
                out.push(format!("{what}'s {face} uses \"{c}\", which colors doesn't give a colour"));
            }
        };
        for face in faces {
            check_face(&mut out, face, &t[face]);
        }
        // a moving block's other pictures (world 0.14)
        if let Some(f) = t.get("frames") {
            match f.as_array() {
                Some(a) if !a.is_empty() && a.len() < MAX_BLOCK_FRAMES => {
                    for (i, frame) in a.iter().enumerate() {
                        let given: Vec<_> = ["picture", "top", "side", "bottom"].into_iter().filter_map(|k| frame.get(k).map(|v| (k, v))).collect();
                        if given.is_empty() {
                            out.push(format!("{what}'s frame {} needs a picture (or a top, a side or a bottom)", i + 1));
                        }
                        for (face, v) in given {
                            check_face(&mut out, &format!("frame {} {face}", i + 1), v);
                        }
                    }
                }
                _ => out.push(format!("{what}'s frames are 1 to {} more pictures to show in turn", MAX_BLOCK_FRAMES - 1)),
            }
        }
        if let Some(speed) = t.get("speed") {
            if !t.contains_key("frames") {
                out.push(format!("{what} has a speed but no frames to show"));
            } else if !speed.as_float().or(speed.as_integer().map(|i| i as f64)).is_some_and(|s| (0.5..=8.0).contains(&s)) {
                out.push(format!("{what}'s speed is how many pictures a second it shows, 0.5 to 8"));
            }
        }
        if let Some(b) = t.get("boxes") {
            // a box, or { box = [...], joins = "east" } (world 0.15)
            let corners = |b: &toml::Value| -> bool {
                let six = |b: Option<&toml::Value>| b.and_then(|b| b.as_array()).is_some_and(|n| n.len() == 6 && n.iter().all(|v| v.as_integer().is_some_and(|v| (0..=16).contains(&v))));
                match b.as_table() {
                    Some(t) => six(t.get("box")) && t.keys().all(|k| k == "box" || k == "joins") && t.get("joins").is_none_or(|j| j.as_str().is_some_and(|j| SIDE_NAMES.contains(&j))),
                    None => six(Some(b)),
                }
            };
            let ok = b.as_array().is_some_and(|a| !a.is_empty() && a.len() <= MAX_BLOCK_BOXES && a.iter().all(corners));
            if !ok {
                out.push(format!("{what}'s boxes are 1 to {MAX_BLOCK_BOXES} lists of six sixteenths, [x, y, z, x, y, z], or {{ box = [x, y, z, x, y, z], joins = \"east\" }}"));
            }
        }
        if t.get("turns").is_some_and(|v| v.as_bool().is_none()) {
            out.push(format!("{what}'s turns is true or false"));
        }
    }
    out
}

/// The sides a box can join toward (the game's `made::SIDE_NAMES`).
const SIDE_NAMES: [&str; 6] = ["east", "west", "up", "down", "south", "north"];

/// The API modules this service knows and the versions games have. Keep in
/// step with the modules in the game's script.rs.
pub const MODULES: &[(&str, &str)] = &[("world", "0.15"), ("tweak", "0.1"), ("game", "0.1"), ("store", "0.1"), ("input", "0.1"), ("ui", "0.2"), ("sound", "0.2"), ("net", "0.2"), ("test", "0.5")];
pub const KINDS: &[&str] = &["creature", "block", "tool", "game", "mod", "world", "thing"];
pub const LICENCES: &[&str] = &["CC-BY-4.0", "CC-BY-SA-4.0", "CC0-1.0"];

pub const MAX_FILES: usize = 64;
pub const MAX_FILE: usize = 128 * 1024;
pub const MAX_SIZE: usize = 256 * 1024;
const TEXT: &[&str] = &["luau", "lua", "toml", "md", "txt", "json", "bbmodel"];

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Check {
    pub name: String,
    /// "pass", "warn" (a person looks) or "fail" (held back)
    pub result: String,
    pub note: String,
}

impl Check {
    fn new(name: &str, result: &str, note: impl Into<String>) -> Check {
        Check { name: name.into(), result: result.into(), note: note.into() }
    }
}

pub fn failed(checks: &[Check]) -> bool {
    checks.iter().any(|c| c.result == "fail")
}

pub fn warned(checks: &[Check]) -> bool {
    checks.iter().any(|c| c.result == "warn")
}

pub fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() < 80
        && !name.starts_with('/')
        && !name.contains("..")
        && !name.contains('\\')
        && name.chars().all(|c| c.is_alphanumeric() || "._-/".contains(c))
}

/// Words to look for in names, descriptions and the strings in code. A
/// line `word*` matches any word starting with it; `#` starts a comment.
#[derive(Default)]
pub struct Words {
    whole: HashSet<String>,
    starts: Vec<String>,
}

impl Words {
    pub fn parse(text: &str) -> Words {
        let mut w = Words::default();
        for line in text.lines() {
            let l = line.split('#').next().unwrap_or("").trim().to_lowercase();
            if l.is_empty() {
                continue;
            }
            match l.strip_suffix('*') {
                Some(p) => w.starts.push(fold(p)),
                None => {
                    w.whole.insert(fold(&l));
                }
            }
        }
        w
    }

    pub fn is_empty(&self) -> bool {
        self.whole.is_empty() && self.starts.is_empty()
    }

    /// The listed words found in some text, folding look-alike spellings.
    pub fn find(&self, text: &str) -> Vec<String> {
        let mut found = Vec::new();
        for word in text.split(|c: char| !(c.is_alphanumeric() || "@$".contains(c))) {
            if word.is_empty() {
                continue;
            }
            let f = fold(word);
            // the listed word itself, so a moderator sees which one matched
            let hit = if self.whole.contains(&f) { Some(f.clone()) } else { self.starts.iter().find(|s| f.starts_with(s.as_str())).cloned() };
            if let Some(hit) = hit.filter(|h| !found.contains(h)) {
                found.push(hit);
            }
        }
        found
    }
}

/// Lower case with number and symbol stand-ins turned back into letters and
/// runs of a letter squashed: "B4DDD w0rd" reads as "bad word".
fn fold(s: &str) -> String {
    let mut out = String::new();
    for c in s.to_lowercase().chars() {
        let c = match c {
            '0' => 'o',
            '1' | '!' => 'i',
            '3' => 'e',
            '4' | '@' => 'a',
            '5' | '$' => 's',
            '7' => 't',
            c => c,
        };
        if out.chars().last() != Some(c) {
            out.push(c);
        }
    }
    out
}

/// The strings and comments in Luau code: where words players see live.
pub fn strings_in(code: &str) -> String {
    let mut out = String::new();
    for t in crate::highlight::tokens(code) {
        if matches!(t.kind, crate::highlight::Kind::Str | crate::highlight::Kind::Comment) {
            out.push_str(t.text);
            out.push(' ');
        }
    }
    out
}

/// Whether Luau can compile it, without running anything.
fn compiles(name: &str, code: &str) -> Result<(), String> {
    let lua = mlua::Lua::new();
    let _ = lua.sandbox(true);
    match lua.load(code).set_name(format!("={name}")).into_function() {
        Ok(_) => Ok(()),
        Err(e) => Err(e.to_string().lines().next().unwrap_or("").replace("syntax error: ", "").trim().to_string()),
    }
}

fn version_ok(want: &str, have: &str) -> bool {
    let p = |s: &str| -> Option<(u32, u32)> {
        let mut it = s.trim().split('.');
        Some((it.next()?.parse().ok()?, it.next().unwrap_or("0").parse().ok()?))
    };
    match (p(want), p(have)) {
        // the same major, and any minor up to ours: minors only add, in 0.x
        // as from 1.0 (docs/api.md), as the game's require() checks it
        (Some((wm, wn)), Some((hm, hn))) => wm == hm && wn <= hn,
        _ => false,
    }
}

/// Runs every check on a creation's files.
pub fn run(files: &BTreeMap<String, String>, words: &Words, banned: &dyn Fn(&str) -> bool) -> (Option<Toml>, Vec<Check>) {
    let mut out = Vec::new();

    // Size and file names
    let total: usize = files.iter().map(|(k, v)| k.len() + v.len()).sum();
    let mut size_problems = Vec::new();
    if files.len() > MAX_FILES {
        size_problems.push(format!("it has {} files, and the most is {MAX_FILES}", files.len()));
    }
    if total > MAX_SIZE {
        size_problems.push(format!("it's {} KB, and the most is {} KB", total / 1024, MAX_SIZE / 1024));
    }
    for (name, text) in files {
        let ext = name.rsplit('.').next().unwrap_or("");
        if !safe_name(name) {
            size_problems.push(format!("{name} has a name that isn't allowed"));
        } else if !TEXT.contains(&ext) {
            size_problems.push(format!("{name} isn't a kind of file creations can have yet"));
        } else if text.len() > MAX_FILE {
            size_problems.push(format!("{name} is over {} KB", MAX_FILE / 1024));
        }
    }
    out.push(match size_problems.is_empty() {
        true => Check::new("Size", "pass", format!("{} files, {:.1} KB", files.len(), total as f64 / 1024.0)),
        false => Check::new("Size", "fail", size_problems.join("; ")),
    });

    // potluck.toml
    let toml = files.get("potluck.toml").map(|t| toml::from_str::<Toml>(t));
    let manifest = match toml {
        None => {
            out.push(Check::new("Made for Potluck", "fail", "there's no potluck.toml"));
            None
        }
        Some(Err(e)) => {
            out.push(Check::new("Made for Potluck", "fail", format!("potluck.toml has a mistake: {}", e.message())));
            None
        }
        Some(Ok(mut m)) => {
            if m.main.is_empty() {
                m.main = "main.luau".into();
            }
            if m.kind.is_empty() {
                m.kind = "creature".into();
            }
            if m.licence.is_empty() {
                m.licence = "CC-BY-4.0".into();
            }
            let mut problems = Vec::new();
            if m.name.trim().is_empty() || m.name.chars().count() > 48 {
                problems.push("its name must be 1 to 48 letters".to_string());
            }
            if m.about.chars().count() > 500 {
                problems.push("its about is over 500 letters".into());
            }
            if !files.contains_key(&m.main) {
                problems.push(format!("there's no {} to run", m.main));
            }
            if let Some(c) = m.client.as_ref().filter(|c| !files.contains_key(*c)) {
                problems.push(format!("there's no {c} for its client half"));
            }
            if !KINDS.contains(&m.kind.as_str()) {
                problems.push(format!("kind \"{}\" isn't one of {}", m.kind, KINDS.join(", ")));
            }
            if !LICENCES.contains(&m.licence.as_str()) {
                problems.push(format!("licence \"{}\" isn't one of {}", m.licence, LICENCES.join(", ")));
            }
            out.push(match problems.is_empty() {
                true => Check::new("Made for Potluck", "pass", format!("a {} called {}", m.kind, m.name)),
                false => Check::new("Made for Potluck", "fail", problems.join("; ")),
            });
            Some(m)
        }
    };

    // The API it needs, and what the code asks for
    let mut api = Vec::new();
    if let Some(m) = &manifest {
        for (module, want) in &m.needs {
            match MODULES.iter().find(|(n, _)| n == module) {
                None => api.push(format!("there's no module called @potluck/{module}")),
                Some((_, have)) if !version_ok(want, have) => api.push(format!("@potluck/{module} {want} isn't in this version of Potluck ({have})")),
                _ => {}
            }
        }
    }
    for (name, text) in files.iter().filter(|(n, _)| n.ends_with(".luau") || n.ends_with(".lua")) {
        let mut rest = text.as_str();
        while let Some(i) = rest.find("@potluck/") {
            rest = &rest[i + 9..];
            let module: String = rest.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
            if !module.is_empty() && !MODULES.iter().any(|(n, _)| *n == module) {
                let note = format!("{name} asks for @potluck/{module}, which doesn't exist");
                if !api.contains(&note) {
                    api.push(note);
                }
            }
        }
    }
    let needs: Vec<String> = manifest.iter().flat_map(|m| m.needs.iter().map(|(k, v)| format!("{k} {v}"))).collect();
    out.push(match api.is_empty() {
        true => Check::new("API", "pass", if needs.is_empty() { "needs nothing special".to_string() } else { format!("needs {}", needs.join(", ")) }),
        false => Check::new("API", "fail", api.join("; ")),
    });

    // Its buttons
    if let Some(m) = manifest.as_ref().filter(|m| !m.actions.is_empty()) {
        let (problems, no_pad) = action_problems(&m.actions);
        out.push(if !problems.is_empty() {
            Check::new("Buttons", "fail", problems.join("; "))
        } else if !no_pad.is_empty() {
            Check::new("Buttons", "pass", format!("not pad friendly: {} has no pad button", no_pad.join(", ")))
        } else {
            Check::new("Buttons", "pass", "pad friendly: every action has a pad button")
        });
    }

    // Its own sounds
    if let Some(m) = manifest.as_ref().filter(|m| !m.sounds.is_empty()) {
        let problems = sound_problems(&m.sounds);
        out.push(match problems.is_empty() {
            true => Check::new("Sounds", "pass", format!("makes {} sound{} of its own", m.sounds.len(), if m.sounds.len() == 1 { "" } else { "s" })),
            false => Check::new("Sounds", "fail", problems.join("; ")),
        });
    }

    // Its own blocks (world 0.12)
    if let Some(m) = manifest.as_ref().filter(|m| !m.blocks.is_empty()) {
        let problems = block_problems(&m.blocks);
        out.push(match problems.is_empty() {
            true => Check::new("Blocks", "pass", format!("makes {} block{} of its own", m.blocks.len(), if m.blocks.len() == 1 { "" } else { "s" })),
            false => Check::new("Blocks", "fail", problems.join("; ")),
        });
    }

    // Its Blockbench models (world 0.11), read as the game reads them
    let models: Vec<_> = files.iter().filter(|(n, _)| n.ends_with(".bbmodel")).collect();
    if !models.is_empty() {
        let mut problems = vec![];
        let mut notes = vec![];
        for (name, text) in &models {
            match model_cubes(text) {
                Err(e) => problems.push(format!("{name} {e}")),
                Ok(0) => problems.push(format!("{name} has no cubes in it to make a body from")),
                Ok(n) if n > MAX_MODEL_CUBES => notes.push(format!("{name} has {n} cubes, and a body uses the first {MAX_MODEL_CUBES}")),
                Ok(_) => {}
            }
        }
        out.push(if !problems.is_empty() {
            Check::new("Models", "fail", problems.join("; "))
        } else if !notes.is_empty() {
            Check::new("Models", "warn", notes.join("; "))
        } else {
            Check::new("Models", "pass", format!("{} Blockbench model{}", models.len(), if models.len() == 1 { "" } else { "s" }))
        });
    }

    // The code compiles
    let mut code_problems = Vec::new();
    for (name, text) in files.iter().filter(|(n, _)| n.ends_with(".luau") || n.ends_with(".lua")) {
        if name.starts_with("types/") {
            continue;
        }
        if let Err(e) = compiles(name, text) {
            code_problems.push(e);
        }
    }
    out.push(match code_problems.is_empty() {
        true => Check::new("Code", "pass", "every file compiles"),
        false => Check::new("Code", "fail", code_problems.join("; ")),
    });

    // Words: in its name and description they hold it back; in its code a person looks
    if words.is_empty() {
        out.push(Check::new("Words", "warn", "no word list is set up on this server, so a person checks"));
    } else {
        let mut shown = Vec::new();
        if let Some(m) = &manifest {
            shown.extend(words.find(&m.name));
            shown.extend(words.find(&m.about));
            shown.extend(words.find(&m.by));
        }
        let mut in_code = Vec::new();
        // what players see its blocks called
        if let Some(m) = &manifest {
            for (key, b) in &m.blocks {
                in_code.extend(words.find(key));
                if let Some(n) = b.get("name").and_then(|n| n.as_str()) {
                    in_code.extend(words.find(n));
                }
            }
        }
        for (name, text) in files {
            let t = if name.ends_with(".luau") || name.ends_with(".lua") {
                strings_in(text)
            } else if name.ends_with(".bbmodel") {
                model_names(text)
            } else if name == "potluck.toml" || name == "AGENTS.md" || name.starts_with("types/") {
                String::new()
            } else {
                text.clone()
            };
            in_code.extend(words.find(&t));
        }
        out.push(if !shown.is_empty() {
            Check::new("Words", "fail", "its name or description has words the market doesn't allow")
        } else if !in_code.is_empty() {
            Check::new("Words", "warn", format!("its text has words a person should look at: {}", in_code.join(", ")))
        } else {
            Check::new("Words", "pass", "nothing found")
        });
    }

    // Files taken down before
    let bad: Vec<&String> = files.iter().filter(|(_, t)| banned(&crate::codes::sha256_hex(t.as_bytes()))).map(|(n, _)| n).collect();
    out.push(match bad.is_empty() {
        true => Check::new("Known files", "pass", "none taken down before"),
        false => Check::new("Known files", "fail", format!("{} was taken down before", bad.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "))),
    });

    (manifest, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn creation(main: &str) -> BTreeMap<String, String> {
        let mut f = BTreeMap::new();
        f.insert("potluck.toml".into(), "name = \"Hopper\"\nby = \"jo\"\nkind = \"creature\"\nmain = \"main.luau\"\n[needs]\nworld = \"0.1\"\n".into());
        f.insert("main.luau".into(), main.into());
        f
    }

    #[test]
    fn a_good_creation_passes() {
        let words = Words::parse("grumble\nsnark*\n");
        let (m, checks) = run(&creation("local world = require(\"@potluck/world\")\nprint(\"hi\")\n"), &words, &|_| false);
        assert_eq!(m.unwrap().name, "Hopper");
        assert!(checks.iter().all(|c| c.result == "pass"), "{checks:?}");
    }

    #[test]
    fn every_base_creation_passes() {
        let words = Words::parse("grumbleweed\nsnark*\n");
        for (id, list) in crate::market::BASE {
            let files: BTreeMap<String, String> = list.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
            let (_, checks) = run(&files, &words, &|_| false);
            assert!(!failed(&checks) && !warned(&checks), "{id}: {checks:?}");
        }
    }

    #[test]
    fn a_creations_own_sounds_are_checked() {
        let words = Words::parse("grumble\n");
        let with = |sounds: &str| {
            let mut f = creation("local sound = require(\"@potluck/sound@0.2\")\n");
            let toml = f["potluck.toml"].replace("[needs]", &format!("{sounds}\n[needs]\nsound = \"0.2\""));
            f.insert("potluck.toml".into(), toml);
            let (_, checks) = run(&f, &words, &|_| false);
            checks.into_iter().find(|c| c.name == "Sounds").expect("a Sounds check")
        };
        let good = with("[sounds.moo]\nwave = \"saw\"\npitch = [220, 160]\nlength = 0.8\n[[sounds.moo.layers]]\nwave = \"noise\"\npitch = 900\ngain = 0.3\n[sounds.tune]\nnotes = [\"C5\", \"E5\", \"-\", \"G5\", 1046.5]\n");
        assert_eq!((good.result.as_str(), good.note.as_str()), ("pass", "makes 2 sounds of its own"), "{good:?}");
        for bad in [
            "[sounds.moo]\npiitch = 200\n",
            "[sounds.moo]\nwave = \"hum\"\n",
            "[sounds.Moo]\npitch = 200\n",
            "[sounds.moo]\nnotes = [\"H2\"]\n",
            "[sounds.moo]\n[[sounds.moo.layers]]\ncaption = \"no\"\n",
        ] {
            let c = with(bad);
            assert_eq!(c.result, "fail", "{bad}: {c:?}");
        }
    }

    #[test]
    fn blockbench_models_are_checked_and_only_their_names_are_read() {
        let words = Words::parse("grumble\n");
        let with = |model: &str| {
            let mut f = creation("local world = require(\"@potluck/world@0.11\")\n");
            f.insert("models/hopper.bbmodel".into(), model.into());
            let (_, checks) = run(&f, &words, &|_| false);
            checks
        };
        let find = |checks: &[Check], name: &str| checks.iter().find(|c| c.name == name).cloned().unwrap_or_else(|| panic!("a {name} check: {checks:?}"));
        let cube = r#"{"name": "leg", "from": [0, 0, 0], "to": [2, 4, 2]}"#;
        // a picture whose letters happen to spell a word is still only a picture
        let good = with(&format!(r#"{{"elements": [{cube}, {{"type": "mesh", "name": "tail"}}], "textures": [{{"name": "skin", "source": "data:image/png;base64,GRUMBLE"}}]}}"#));
        let models = find(&good, "Models");
        assert_eq!((models.result.as_str(), models.note.as_str()), ("pass", "1 Blockbench model"), "{models:?}");
        assert_eq!(find(&good, "Size").result, "pass");
        assert_eq!(find(&good, "Words").result, "pass", "{good:?}");
        let named = with(&format!(r#"{{"elements": [{cube}, {{"name": "grumble", "from": [0, 0, 0], "to": [1, 1, 1]}}]}}"#));
        assert_eq!(find(&named, "Words").result, "warn", "a cube's name is read");
        for bad in ["{ not json", r#"{"cubes": []}"#, r#"{"elements": [{"type": "mesh"}]}"#] {
            assert_eq!(find(&with(bad), "Models").result, "fail", "{bad}");
        }
        let many: Vec<String> = (0..70).map(|_| cube.to_string()).collect();
        assert_eq!(find(&with(&format!(r#"{{"elements": [{}]}}"#, many.join(","))), "Models").result, "warn");
        assert_eq!(model_cubes(&format!(r#"{{"elements": [{cube}, {{"name": "x", "from": [0, 0, 0], "to": [1, 1, 1], "visibility": false}}]}}"#)), Ok(1));
    }

    #[test]
    fn a_creations_own_blocks_are_checked_and_their_names_read() {
        let words = Words::parse("grumble\n");
        let with = |blocks: &str| {
            let mut f = creation("local world = require(\"@potluck/world@0.12\")\n");
            let toml = f["potluck.toml"].replace("[needs]", &format!("{blocks}\n[needs]"));
            f.insert("potluck.toml".into(), toml);
            let (_, checks) = run(&f, &words, &|_| false);
            checks
        };
        let find = |checks: &[Check], name: &str| checks.iter().find(|c| c.name == name).cloned().unwrap_or_else(|| panic!("a {name} check: {checks:?}"));
        let eight = format!("picture = [{}]", vec!["\"aaaaaaaa\""; 8].join(", "));
        let good = with(&format!("[blocks.marble]\ncolors = {{ a = \"#f0ece4\" }}\n{eight}\n[blocks.slab]\nboxes = [[0, 0, 0, 16, 8, 16]]\ncolors = {{ a = \"#fff\" }}\n{eight}\n"));
        let b = find(&good, "Blocks");
        assert_eq!((b.result.as_str(), b.note.as_str()), ("pass", "makes 2 blocks of its own"), "{b:?}");
        assert_eq!(find(&good, "Words").result, "pass");
        assert_eq!(find(&with(&format!("[blocks.marble]\nname = \"Grumble stone\"\ncolors = {{ a = \"#fff\" }}\n{eight}\n")), "Words").result, "warn", "what players see it called is read");
        for bad in [
            format!("[blocks.Marble]\ncolors = {{ a = \"#fff\" }}\n{eight}"),
            format!("[blocks.marble]\n{eight}"),
            format!("[blocks.marble]\ncolors = {{ a = \"white\" }}\n{eight}"),
            format!("[blocks.marble]\ncolors = {{ b = \"#fff\" }}\n{eight}"),
            "[blocks.marble]\ncolors = { a = \"#fff\" }\npicture = [\"aaa\"]".to_string(),
            format!("[blocks.marble]\ncolors = {{ a = \"#fff\" }}\ntop = [\"aaaaaaaa\"]"),
            format!("[blocks.marble]\nboxes = [[0, 0, 0, 16, 8]]\ncolors = {{ a = \"#fff\" }}\n{eight}"),
            // moving pictures (world 0.14)
            format!("[blocks.marble]\ncolors = {{ a = \"#fff\" }}\nspeed = 2\n{eight}"),
            format!("[blocks.marble]\ncolors = {{ a = \"#fff\" }}\n{eight}\nframes = []"),
            format!("[blocks.marble]\ncolors = {{ a = \"#fff\" }}\n{eight}\n[[blocks.marble.frames]]\npicture = [\"bbbbbbbb\"]"),
            format!("[blocks.marble]\ncolors = {{ a = \"#fff\" }}\n{eight}\n[[blocks.marble.frames]]\nshade = 1"),
            format!("[blocks.marble]\ncolors = {{ a = \"#fff\" }}\nspeed = 20\n{eight}\n[[blocks.marble.frames]]\n{eight}"),
            format!("[blocks.marble]\ncolors = {{ a = \"#fff\" }}\n{eight}\n{}", (0..8).map(|_| format!("[[blocks.marble.frames]]\n{eight}\n")).collect::<String>()),
        ] {
            assert_eq!(find(&with(&bad), "Blocks").result, "fail", "{bad}");
        }
        let joined = with(&format!("[blocks.marble]\ncolors = {{ a = \"#fff\" }}\nturns = true\nboxes = [[6, 0, 6, 10, 1, 10], {{ box = [10, 0, 7, 16, 1, 9], joins = \"east\" }}]\n{eight}\n"));
        assert_eq!(find(&joined, "Blocks").result, "pass", "{:?}", find(&joined, "Blocks"));
        for bad in [
            format!("[blocks.marble]\ncolors = {{ a = \"#fff\" }}\nturns = \"yes\"\n{eight}"),
            format!("[blocks.marble]\ncolors = {{ a = \"#fff\" }}\nboxes = [{{ box = [0, 0, 0, 16, 8, 16], joins = \"upward\" }}]\n{eight}"),
            format!("[blocks.marble]\ncolors = {{ a = \"#fff\" }}\nboxes = [{{ at = [0, 0, 0, 16, 8, 16] }}]\n{eight}"),
        ] {
            assert_eq!(find(&with(&bad), "Blocks").result, "fail", "{bad}");
        }
        let moving = with(&format!("[blocks.marble]\ncolors = {{ a = \"#fff\", b = \"#f80\" }}\nspeed = 2\n{eight}\n[[blocks.marble.frames]]\nside = [{}]\n", vec!["\"bbbbbbbb\""; 8].join(", ")));
        assert_eq!(find(&moving, "Blocks").result, "pass", "{:?}", find(&moving, "Blocks"));
    }

    #[test]
    fn buttons_are_checked_and_pad_friendly_is_noted() {
        let words = Words::parse("grumble\n");
        let with = |actions: &str| {
            let mut f = creation("local input = require(\"@potluck/input@0.1\")\n");
            let toml = f["potluck.toml"].replace("[needs]", &format!("{actions}\n[needs]\ninput = \"0.1\""));
            f.insert("potluck.toml".into(), toml);
            let (_, checks) = run(&f, &words, &|_| false);
            checks.into_iter().find(|c| c.name == "Buttons").expect("a Buttons check")
        };
        let good = with("[actions.buck]\nname = \"Buck\"\npad = \"X\"\nkeys = \"F\"\n");
        assert_eq!((good.result.as_str(), good.note.contains("pad friendly")), ("pass", true), "{good:?}");
        let keys_only = with("[actions.buck]\nname = \"Buck\"\nkeys = \"F\"\n");
        assert!(keys_only.result == "pass" && keys_only.note.contains("not pad friendly: buck"), "{keys_only:?}");
        for bad in ["[actions.buck]\nname = \"Buck\"\npad = \"B\"\n", "[actions.buck]\nname = \"Buck\"\nkeys = \"W\"\n", "[actions.Buck]\nname = \"Buck\"\npad = \"X\"\n", "[actions.buck]\nname = \"\"\npad = \"X\"\n", "[actions.buck]\nname = \"Buck\"\n"] {
            assert_eq!(with(bad).result, "fail", "{bad}");
        }
    }

    #[test]
    fn broken_code_and_unknown_modules_fail() {
        let (_, checks) = run(&creation("local x = = 2\nrequire(\"@potluck/files\")"), &Words::parse("x"), &|_| false);
        let code = checks.iter().find(|c| c.name == "Code").unwrap();
        assert_eq!(code.result, "fail");
        assert!(code.note.contains("main.luau"), "{}", code.note);
        assert_eq!(checks.iter().find(|c| c.name == "API").unwrap().result, "fail");
    }

    #[test]
    fn words_fold_look_alikes() {
        let w = Words::parse("grumble\nsnark*\n");
        assert_eq!(w.find("what a GRUMMBL3 day"), vec!["grumble"]);
        assert_eq!(w.find("snarky"), vec!["snark"]);
        assert!(w.find("grumbles").is_empty());
        let (_, checks) = run(&creation("print(\"so snarky\") -- fine"), &w, &|_| false);
        assert_eq!(checks.iter().find(|c| c.name == "Words").unwrap().result, "warn");
        let mut f = creation("print(1)");
        f.insert("potluck.toml".into(), "name = \"Grumble Bot\"\n".into());
        let (_, checks) = run(&f, &w, &|_| false);
        assert_eq!(checks.iter().find(|c| c.name == "Words").unwrap().result, "fail");
    }

    #[test]
    fn sizes_names_and_banned_files() {
        let mut f = creation("print(1)");
        f.insert("../escape.luau".into(), "x".into());
        f.insert("big.txt".into(), "a".repeat(MAX_FILE + 1));
        f.insert("picture.png".into(), "x".into());
        let (_, checks) = run(&f, &Words::parse("x"), &|_| false);
        let size = checks.iter().find(|c| c.name == "Size").unwrap();
        assert_eq!(size.result, "fail");
        assert!(size.note.contains("escape") && size.note.contains("big.txt") && size.note.contains("picture.png"), "{}", size.note);
        let bad = crate::codes::sha256_hex(b"print(1)");
        let (_, checks) = run(&creation("print(1)"), &Words::parse("x"), &|h| h == bad);
        assert_eq!(checks.iter().find(|c| c.name == "Known files").unwrap().result, "fail");
    }

    #[test]
    fn module_versions() {
        assert!(version_ok("0.1", "0.1"));
        assert!(version_ok("0.1", "0.2"), "made for 0.1, runs on 0.2");
        assert!(!version_ok("0.2", "0.1"));
        assert!(!version_ok("1.0", "0.2"));
        assert!(version_ok("1.2", "1.4"));
        assert!(!version_ok("1.5", "1.4"));
        assert!(!version_ok("2.0", "1.4"));
    }
}
