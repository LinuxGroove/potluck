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
    /// The game server for share codes.
    pub server: String,
    /// A random id for signing in to the game server, made on first run.
    pub device: String,
    /// Survive (health, breaking takes time, blocks run out) or Build.
    pub survival: bool,
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
            device: String::new(),
            survival: false,
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
