//! Settings, kept as TOML in the player's data folder, and where that folder is.

use std::path::PathBuf;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Resource, Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Settings {
    /// The name friends see.
    pub name: String,
    pub mouse_sensitivity: f32,
    pub stick_sensitivity: f32,
    pub invert_y: bool,
    pub fov: f32,
    /// "low", "medium", "high" or "ultra" (`render::Tier`).
    pub lighting: String,
    /// The game server: signing in, and share codes made before the market.
    pub server: String,
    /// The market, where creations are shared and remixed.
    pub market: String,
    /// A random id for signing in to the game server, made on first run.
    pub device: String,
    /// Survive (health, breaking takes time, blocks run out) or Build.
    pub survival: bool,
    /// The player was asked their age, the first time they went online.
    pub age_asked: bool,
    /// Under 13: sharing by code only, with friends; only all-ages servers
    /// from the market; no suggestions to strangers.
    pub young: bool,
    /// A parent can turn sharing off.
    pub sharing: bool,
    /// A parent can turn off joining servers from the market's list.
    pub servers: bool,
    /// The parent's PIN, hashed with this device's id. Empty: no PIN yet.
    pub parent_pin: String,
    /// Creations' actions moved to other buttons, by `creation/action`.
    pub bindings: std::collections::BTreeMap<String, Binding>,
}

/// A creation's action on buttons the player chose.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct Binding {
    pub pad: String,
    pub keys: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            name: default_name(),
            mouse_sensitivity: 1.0,
            stick_sensitivity: 1.0,
            invert_y: false,
            fov: 90.0,
            lighting: "medium".into(),
            server: crate::share::DEFAULT_SERVER.into(),
            market: crate::share::DEFAULT_MARKET.into(),
            device: String::new(),
            survival: false,
            age_asked: false,
            young: false,
            sharing: true,
            servers: true,
            parent_pin: String::new(),
            bindings: Default::default(),
        }
    }
}

fn default_name() -> String {
    let user = std::env::var("USER").or_else(|_| std::env::var("USERNAME")).unwrap_or_default();
    let user: String = user.chars().filter(|c| c.is_alphanumeric() || *c == '_').take(16).collect();
    if user.is_empty() || user == "root" { "builder".into() } else { user }
}

/// Where Potluck keeps settings, creations and worlds.
pub fn data_dir() -> PathBuf {
    if let Ok(d) = std::env::var("POTLUCK_HOME") {
        return PathBuf::from(d);
    }
    let home = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("."));
    if cfg!(target_os = "windows") {
        std::env::var("APPDATA").map(PathBuf::from).unwrap_or(home).join("Potluck")
    } else if cfg!(target_os = "macos") {
        home.join("Library/Application Support/Potluck")
    } else {
        std::env::var("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|_| home.join(".local/share")).join("potluck")
    }
}

impl Settings {
    fn path() -> PathBuf {
        data_dir().join("settings.toml")
    }

    pub fn load() -> Self {
        let mut s: Settings = std::fs::read_to_string(Self::path()).ok().and_then(|t| toml::from_str(&t).ok()).unwrap_or_default();
        if s.device.len() < 16 {
            s.device = random_hex(32);
            s.save();
        }
        s
    }

    /// A PIN as it's kept: never the digits themselves.
    /// The pad button and key an action is on: the player's choice, or the
    /// creation's.
    pub fn binding(&self, creation: &str, action: &str, def: &crate::creations::ActionDef) -> (String, String) {
        match self.bindings.get(&format!("{creation}/{action}")) {
            Some(b) => (b.pad.clone(), b.keys.clone()),
            None => (def.pad.clone(), def.keys.clone()),
        }
    }

    pub fn pin_hash(&self, pin: &str) -> String {
        use sha2::Digest;
        let d = sha2::Sha256::digest(format!("potluck parent pin\n{}\n{}", self.device, pin.trim()).as_bytes());
        d.iter().map(|b| format!("{b:02x}")).collect()
    }

    pub fn pin_matches(&self, pin: &str) -> bool {
        !self.parent_pin.is_empty() && self.pin_hash(pin) == self.parent_pin
    }

    pub fn save(&self) {
        let _ = std::fs::create_dir_all(data_dir());
        if let Ok(t) = toml::to_string_pretty(self) {
            let _ = std::fs::write(Self::path(), t);
        }
    }
}

/// Random hex digits from the clock and the process (no extra crate needed).
pub fn random_hex(n: usize) -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut out = String::new();
    let mut i = 0u64;
    while out.len() < n {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u64(i);
        h.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0));
        out.push_str(&format!("{:016x}", h.finish()));
        i += 1;
    }
    out.truncate(n);
    out
}
