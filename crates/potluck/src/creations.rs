//! Creations: folders of plain text (potluck.toml, Luau, notes) that make
//! everything in the world. The base game's own come built in; players'
//! live in their data folder, where any editor can change them.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::script::KnobValue;
use crate::settings::data_dir;

const MAX_FILE: u64 = 128 * 1024;
const MAX_FILES: usize = 64;
pub const MAX_SIZE: usize = 256 * 1024;

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct Manifest {
    pub name: String,
    pub by: String,
    pub kind: String,
    pub about: String,
    pub main: String,
    pub version: u32,
    pub remix_of: Option<String>,
    pub licence: String,
    /// The API modules it needs, by version: world = "0.1".
    pub needs: BTreeMap<String, String>,
    /// What it's a remix of on the market, so the market can draw its family
    /// tree and carry suggestions back.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<FromRef>,
    /// Buttons it asks for with `@potluck/input`, by id: `[actions.buck]`
    /// with a name, a pad button and a key. Players can change them.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub actions: BTreeMap<String, ActionDef>,
    /// Its client half: a file that runs on each player's own game, for
    /// what only that player sees (`@potluck/net`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client: Option<String>,
}

/// One of a creation's actions: what players see it called, and the pad
/// button and key it starts on (`crate::input::PAD_BUTTONS`, `key_ok`).
#[derive(Clone, Serialize, Deserialize, Debug, Default, PartialEq)]
pub struct ActionDef {
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub pad: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub keys: String,
}

/// The market's name for what a creation was remixed from: a code and its
/// version, or one of the base game's creations by its id.
#[derive(Clone, Serialize, Deserialize, Debug, Default, PartialEq)]
pub struct FromRef {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
    #[serde(default)]
    pub version: u32,
}

/// What the market knows one of the player's creations by: its code there,
/// the newest version, and whether it's shared to Everyone. Kept by creation
/// id in market.json, so sharing again makes the next version.
#[derive(Clone, Serialize, Deserialize, Debug, Default, PartialEq)]
pub struct Mark {
    pub code: String,
    pub version: u32,
    #[serde(default)]
    pub everyone: bool,
}

impl Default for Manifest {
    fn default() -> Self {
        Self {
            name: "Untitled".into(),
            by: "someone".into(),
            kind: "creature".into(),
            about: String::new(),
            main: "main.luau".into(),
            version: 1,
            remix_of: None,
            licence: "CC-BY-4.0".into(),
            needs: BTreeMap::new(),
            from: None,
            actions: BTreeMap::new(),
            client: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Origin {
    /// Made by Potluck and built into the game.
    Base,
    /// In the player's own folder.
    Mine,
    /// Came from a friend's world.
    Friend(String),
    /// Loaded from a share code.
    Code(String),
}

#[derive(Clone, Debug)]
pub struct Creation {
    pub id: String,
    pub manifest: Manifest,
    pub files: BTreeMap<String, String>,
    pub dir: Option<PathBuf>,
    pub origin: Origin,
    pub stamp: u64,
    pub knobs: BTreeMap<String, KnobValue>,
    /// The share code of its last share.
    pub code: Option<String>,
}

/// A creation as sent between games and to the share server.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Payload {
    pub format: u32,
    pub id: String,
    pub manifest: Manifest,
    pub files: BTreeMap<String, String>,
    #[serde(default)]
    pub knobs: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    pub code: Option<String>,
}

pub fn knob_to_json(v: &KnobValue) -> serde_json::Value {
    match v {
        KnobValue::Number(n) => serde_json::json!(n),
        KnobValue::Toggle(b) => serde_json::json!(b),
        KnobValue::Color(c) => serde_json::json!(c),
    }
}

pub fn knob_from_json(v: &serde_json::Value) -> Option<KnobValue> {
    match v {
        serde_json::Value::Number(n) => Some(KnobValue::Number(n.as_f64()?)),
        serde_json::Value::Bool(b) => Some(KnobValue::Toggle(*b)),
        serde_json::Value::String(s) => Some(KnobValue::Color(s.clone())),
        _ => None,
    }
}

impl Creation {
    pub fn payload(&self) -> Payload {
        Payload {
            format: 1,
            id: self.id.clone(),
            manifest: self.manifest.clone(),
            files: self.files.clone(),
            knobs: self.knobs.iter().map(|(k, v)| (k.clone(), knob_to_json(v))).collect(),
            code: self.code.clone(),
        }
    }

    pub fn from_payload(p: Payload, origin: Origin) -> Result<Creation, String> {
        let size: usize = p.files.iter().map(|(k, v)| k.len() + v.len()).sum();
        if size > MAX_SIZE || p.files.len() > MAX_FILES {
            return Err("that creation is too big".into());
        }
        if !p.files.contains_key(&p.manifest.main) {
            return Err(format!("that creation has no {}", p.manifest.main));
        }
        for name in p.files.keys() {
            if !safe_name(name) {
                return Err(format!("that creation has a file with a strange name: {name}"));
            }
        }
        Ok(Creation {
            id: slug(&p.id),
            manifest: p.manifest,
            files: p.files,
            dir: None,
            origin,
            stamp: 0,
            knobs: p.knobs.iter().filter_map(|(k, v)| Some((k.clone(), knob_from_json(v)?))).collect(),
            code: p.code,
        })
    }

    pub fn size(&self) -> usize {
        self.files.values().map(|v| v.len()).sum()
    }

    pub fn editable(&self) -> bool {
        self.origin == Origin::Mine
    }
}

fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() < 80
        && !name.starts_with('/')
        && !name.contains("..")
        && !name.contains('\\')
        && name.chars().all(|c| c.is_alphanumeric() || "._-/".contains(c))
}

pub fn slug(s: &str) -> String {
    let mut out: String = s.to_lowercase().chars().map(|c| if c.is_alphanumeric() { c } else { '-' }).collect();
    while out.contains("--") {
        out = out.replace("--", "-");
    }
    let out = out.trim_matches('-').chars().take(40).collect::<String>();
    if out.is_empty() { "creation".into() } else { out }
}

/// The base game's creations, built into the binary: each folder in
/// `creations/` with its manifest and code, and the API's notes and types.
/// The base game's creations, built in: each one's potluck.toml and
/// main.luau, the files after a `+`, and the API's notes and types.
macro_rules! base {
    ($($id:literal $(+ $file:literal)*),* $(,)?) => {
        &[$(($id, &[
            ("potluck.toml", include_str!(concat!("../../../creations/", $id, "/potluck.toml"))),
            ("main.luau", include_str!(concat!("../../../creations/", $id, "/main.luau"))),
            $(($file, include_str!(concat!("../../../creations/", $id, "/", $file))),)*
            ("AGENTS.md", AGENTS_MD),
            ("types/potluck.d.luau", TYPES),
        ])),*]
    };
}

const BASE: &[(&str, &[(&str, &str)])] = base!["mossback", "woolly", "pebble-crab", "glowbug", "gnome", "duck", "frog", "mole", "pup", "rodeo-ram" + "client.luau", "jelly", "tag-bot", "villagers", "gloomlings", "workbench", "farming", "hunger", "tools"];

pub const AGENTS_MD: &str = include_str!("../../../creations/AGENTS.md");
pub const TYPES: &str = include_str!("../../../creations/types/potluck.d.luau");

#[derive(Resource, Default)]
pub struct Library {
    pub list: BTreeMap<String, Creation>,
    /// The creations running in this world.
    pub running: Vec<String>,
    since_scan: f32,
    /// Creations whose files changed on disk since the last frame.
    pub changed: Vec<String>,
    /// The market's codes for creations, by id (market.json).
    pub marks: BTreeMap<String, Mark>,
    /// Counts the worlds started or joined, so client halves start afresh
    /// in each.
    pub world: u32,
}

fn mine_dir() -> PathBuf {
    data_dir().join("creations")
}

fn tweaks_path() -> PathBuf {
    data_dir().join("tweaks.json")
}

fn marks_path() -> PathBuf {
    data_dir().join("market.json")
}

fn read_dir_files(dir: &Path) -> (BTreeMap<String, String>, u64) {
    let mut files = BTreeMap::new();
    let mut stamp = 0u64;
    let mut stack = vec![(dir.to_path_buf(), String::new())];
    while let Some((d, prefix)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            let name = format!("{prefix}{}", e.file_name().to_string_lossy());
            if name.starts_with('.') {
                continue;
            }
            let Ok(meta) = e.metadata() else { continue };
            if meta.is_dir() {
                if prefix.is_empty() && files.len() < MAX_FILES {
                    stack.push((p, format!("{name}/")));
                }
                continue;
            }
            let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
            if !matches!(ext, "luau" | "lua" | "toml" | "md" | "txt" | "json") || meta.len() > MAX_FILE || !safe_name(&name) {
                continue;
            }
            let m = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_millis() as u64).unwrap_or(0);
            stamp = stamp.wrapping_add(m.wrapping_mul(31)).wrapping_add(meta.len());
            if let Ok(t) = std::fs::read_to_string(&p) {
                files.insert(name, t);
            }
            if files.len() >= MAX_FILES {
                break;
            }
        }
    }
    (files, stamp)
}

fn load_dir(id: &str, dir: &Path) -> Option<Creation> {
    let (files, stamp) = read_dir_files(dir);
    let manifest: Manifest = toml::from_str(files.get("potluck.toml")?).unwrap_or_default();
    Some(Creation { id: id.to_string(), manifest, files, dir: Some(dir.to_path_buf()), origin: Origin::Mine, stamp, knobs: BTreeMap::new(), code: None })
}

impl Library {
    pub fn load() -> Self {
        let mut lib = Library::default();
        for (id, files) in BASE {
            let files: BTreeMap<String, String> = files.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
            let manifest: Manifest = toml::from_str(&files["potluck.toml"]).expect("base creation manifest");
            lib.list.insert(id.to_string(), Creation { id: id.to_string(), manifest, files, dir: None, origin: Origin::Base, stamp: 0, knobs: BTreeMap::new(), code: None });
        }
        if let Ok(rd) = std::fs::read_dir(mine_dir()) {
            for e in rd.flatten() {
                if e.path().is_dir() {
                    let id = e.file_name().to_string_lossy().to_string();
                    if let Some(c) = load_dir(&id, &e.path()) {
                        lib.list.insert(id, c);
                    }
                }
            }
        }
        lib.marks = std::fs::read_to_string(marks_path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        for (id, m) in &lib.marks {
            if let Some(c) = lib.list.get_mut(id).filter(|c| c.origin == Origin::Mine) {
                c.code = Some(m.code.clone());
            }
        }
        // knob settings for every creation
        if let Ok(t) = std::fs::read_to_string(tweaks_path()) {
            if let Ok(m) = serde_json::from_str::<BTreeMap<String, BTreeMap<String, serde_json::Value>>>(&t) {
                for (id, knobs) in m {
                    if let Some(c) = lib.list.get_mut(&id) {
                        c.knobs = knobs.iter().filter_map(|(k, v)| Some((k.clone(), knob_from_json(v)?))).collect();
                    }
                }
            }
        }
        lib
    }

    pub fn save_knobs(&self) {
        let m: BTreeMap<&String, BTreeMap<&String, serde_json::Value>> = self
            .list
            .iter()
            .filter(|(_, c)| !c.knobs.is_empty() && matches!(c.origin, Origin::Base | Origin::Mine))
            .map(|(id, c)| (id, c.knobs.iter().map(|(k, v)| (k, knob_to_json(v))).collect()))
            .collect();
        let _ = std::fs::create_dir_all(data_dir());
        if let Ok(t) = serde_json::to_string_pretty(&m) {
            let _ = std::fs::write(tweaks_path(), t);
        }
    }

    /// Remembers a creation's code and version on the market.
    pub fn mark(&mut self, id: &str, mark: Mark) {
        if let Some(c) = self.list.get_mut(id) {
            c.code = Some(mark.code.clone());
        }
        self.marks.insert(id.to_string(), mark);
        let mine: BTreeMap<&String, &Mark> = self.marks.iter().filter(|(id, _)| self.list.get(*id).is_some_and(|c| c.origin == Origin::Mine)).collect();
        let _ = std::fs::create_dir_all(data_dir());
        if let Ok(t) = serde_json::to_string_pretty(&mine) {
            let _ = std::fs::write(marks_path(), t);
        }
    }

    /// Puts a new version's files (a merged suggestion) in one of the
    /// player's creations' folders. A running one reloads as for any edit.
    pub fn replace_files(&mut self, id: &str, files: BTreeMap<String, String>) -> Result<(), String> {
        let c = self.list.get_mut(id).ok_or("that creation isn't here")?;
        let dir = c.dir.clone().ok_or("that creation has no folder")?;
        for name in files.keys() {
            if !safe_name(name) {
                return Err(format!("a strange file name: {name}"));
            }
        }
        for name in c.files.keys().filter(|n| !files.contains_key(*n)) {
            let _ = std::fs::remove_file(dir.join(name));
        }
        for (name, text) in &files {
            let p = dir.join(name);
            if let Some(parent) = p.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            std::fs::write(&p, text).map_err(|e| format!("couldn't write {}: {e}", p.display()))?;
        }
        if let Ok(m) = toml::from_str::<Manifest>(files.get("potluck.toml").map(|s| s.as_str()).unwrap_or("")) {
            c.manifest = m;
        }
        c.files = files;
        c.stamp = read_dir_files(&dir).1;
        if self.running.iter().any(|r| r == id) {
            self.changed.push(id.to_string());
        }
        Ok(())
    }

    /// Looks for edits on disk to the creations of mine that are running.
    pub fn watch(&mut self, dt: f32) {
        self.since_scan += dt;
        if self.since_scan < 0.4 {
            return;
        }
        self.since_scan = 0.0;
        for id in self.running.clone() {
            let Some(c) = self.list.get_mut(&id) else { continue };
            let Some(dir) = c.dir.clone() else { continue };
            let (files, stamp) = read_dir_files(&dir);
            if stamp != c.stamp && files.contains_key("potluck.toml") {
                c.stamp = stamp;
                if let Ok(m) = toml::from_str::<Manifest>(&files["potluck.toml"]) {
                    c.manifest = m;
                }
                if c.files != files {
                    c.files = files;
                    self.changed.push(id);
                }
            }
        }
    }

    /// Copies a creation into the player's folder to change as their own.
    pub fn remix(&mut self, id: &str, me: &str) -> Result<String, String> {
        let src = self.list.get(id).ok_or("that creation isn't here")?.clone();
        let base = slug(&format!("{}-{}", src.manifest.name, me));
        let mut new_id = base.clone();
        let mut n = 2;
        while self.list.contains_key(&new_id) || mine_dir().join(&new_id).exists() {
            new_id = format!("{base}-{n}");
            n += 1;
        }
        let dir = mine_dir().join(&new_id);
        std::fs::create_dir_all(&dir).map_err(|e| format!("couldn't make {}: {e}", dir.display()))?;
        let mut manifest = src.manifest.clone();
        manifest.remix_of = Some(match &src.code {
            Some(code) => format!("{} ({code})", src.manifest.name),
            None => format!("{} by {}", src.manifest.name, src.manifest.by),
        });
        manifest.by = me.to_string();
        manifest.version = 1;
        manifest.from = match (&src.origin, self.marks.get(id)) {
            (Origin::Base, _) => Some(FromRef { code: None, base: Some(id.to_string()), version: 0 }),
            (_, Some(m)) => Some(FromRef { code: Some(m.code.clone()), base: None, version: m.version }),
            (_, None) => src.code.clone().map(|code| FromRef { code: Some(code), base: None, version: 0 }),
        };
        let mut files = src.files.clone();
        files.insert("potluck.toml".into(), toml::to_string_pretty(&manifest).unwrap_or_default());
        files.insert("AGENTS.md".into(), AGENTS_MD.into());
        files.insert("types/potluck.d.luau".into(), TYPES.into());
        for (name, text) in &files {
            let p = dir.join(name);
            if let Some(parent) = p.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            std::fs::write(&p, text).map_err(|e| format!("couldn't write {}: {e}", p.display()))?;
        }
        let mut c = load_dir(&new_id, &dir).ok_or("couldn't read the remix back")?;
        c.knobs = src.knobs.clone();
        self.list.insert(new_id.clone(), c);
        Ok(new_id)
    }

    /// Adds (or updates) a creation that arrived from a friend or a code.
    pub fn add(&mut self, mut c: Creation) -> String {
        // never let one from outside overwrite one of mine or the base game's
        if let Some(old) = self.list.get(&c.id) {
            if matches!(old.origin, Origin::Mine | Origin::Base) && c.origin != old.origin {
                if old.files == c.files {
                    let mut o = old.clone();
                    o.knobs = c.knobs;
                    let id = o.id.clone();
                    self.list.insert(id.clone(), o);
                    return id;
                }
                c.id = format!("{}-{}", c.id, match &c.origin { Origin::Code(code) => code.to_lowercase(), _ => "friend".into() });
            }
        }
        let id = c.id.clone();
        self.list.insert(id.clone(), c);
        id
    }
}

/// Opens a web page in the system's browser.
pub fn open_link(url: &str) {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return;
    }
    let (cmd, args): (&str, Vec<&str>) = if cfg!(target_os = "windows") {
        ("cmd", vec!["/C", "start", "", url])
    } else if cfg!(target_os = "macos") {
        ("open", vec![url])
    } else {
        ("xdg-open", vec![url])
    };
    let _ = std::process::Command::new(cmd).args(args).spawn();
}

/// Opens a folder in the system's file manager.
pub fn open_folder(dir: &Path) {
    let cmd = if cfg!(target_os = "windows") {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let _ = std::process::Command::new(cmd).arg(dir).spawn();
}
