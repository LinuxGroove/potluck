//! Running a world for others: a dedicated server's settings (server.toml),
//! who may join and what they may do, passes from the market that say who
//! a player is, and the boards creations keep scores on.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// Who may join a world.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Join {
    /// Anyone who knows the address.
    #[default]
    Anyone,
    /// Players the market vouches for, with a pass.
    SignedIn,
    /// Only the handles in `allow`.
    List,
}

/// Whether players may bring their own creations into a world.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Bring {
    #[default]
    Anyone,
    /// Only players the market vouches for.
    SignedIn,
    Nobody,
}

/// A dedicated server's settings. Every field has a default, so an empty
/// server.toml (or none) is a working server.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct ServerConfig {
    /// What players see in the server list and when they join.
    pub name: String,
    /// A line or two about the world, for the server list.
    pub about: String,
    pub port: u16,
    /// TLS (wss and https) on this port, with `cert` and `key`.
    pub tls_port: Option<u16>,
    pub cert: Option<String>,
    pub key: Option<String>,
    /// The seed of a new world.
    pub seed: u32,
    pub max_players: usize,
    pub join: Join,
    /// Handles that may join when `join = "list"`.
    pub allow: Vec<String>,
    /// Handles that may never join.
    pub ban: Vec<String>,
    pub bring: Bring,
    /// Whether players may place and break blocks.
    pub build: bool,
    /// Announce the world on the local network.
    pub beacon: bool,
    /// Show it in the market's server list. Needs `address` and `market_token`.
    pub list: bool,
    /// The address players use, as the server list gives it: wss://host or host:port.
    pub address: String,
    pub market: String,
    /// From the market's Your servers page.
    pub market_token: String,
    /// Creations this world always runs, by market code (`ABCD2345`, or
    /// `ABCD2345@3` for one version).
    pub creations: Vec<String>,
    /// Other servers creations can send players to, by name: rodeo = "wss://rodeo.example.org".
    pub links: BTreeMap<String, String>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            name: "Potluck server".into(),
            about: String::new(),
            port: crate::net::PORT,
            tls_port: None,
            cert: None,
            key: None,
            seed: crate::SEED,
            max_players: 50,
            join: Join::Anyone,
            allow: vec![],
            ban: vec![],
            bring: Bring::Anyone,
            build: true,
            beacon: true,
            list: false,
            address: String::new(),
            market: crate::share::DEFAULT_MARKET.into(),
            market_token: String::new(),
            creations: vec![],
            links: BTreeMap::new(),
        }
    }
}

impl ServerConfig {
    pub fn read(path: &Path) -> Result<ServerConfig, String> {
        match std::fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text).map_err(|e| format!("{}: {}", path.display(), e.message())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(ServerConfig::default()),
            Err(e) => Err(format!("couldn't read {}: {e}", path.display())),
        }
    }

    /// Problems worth stopping for, in words an operator can act on.
    pub fn check(&self) -> Result<(), String> {
        if self.max_players == 0 {
            return Err("max_players is 0, so nobody could join".into());
        }
        if self.cert.is_some() != self.key.is_some() {
            return Err("TLS needs both cert and key".into());
        }
        if self.join == Join::List && self.allow.is_empty() {
            return Err("join = \"list\" with nobody in allow".into());
        }
        if self.list && (self.address.is_empty() || self.market_token.is_empty()) {
            return Err("list = true needs the address players use and a market_token from the market's Your servers page".into());
        }
        for (name, to) in &self.links {
            if to.trim().is_empty() {
                return Err(format!("the link {name} has no address"));
            }
        }
        for c in &self.creations {
            let code = c.split('@').next().unwrap_or("");
            if crate::share::clean_code(code).len() != 8 {
                return Err(format!("{c} isn't a creation code"));
            }
        }
        Ok(())
    }

    pub fn rules(&self) -> Rules {
        Rules {
            max_players: self.max_players,
            join: self.join,
            allow: self.allow.iter().map(|h| h.trim_start_matches('@').to_lowercase()).collect(),
            ban: self.ban.iter().map(|h| h.trim_start_matches('@').to_lowercase()).collect(),
            bring: self.bring,
            build: self.build,
            address: self.address.clone(),
            links: self.links.clone(),
            market_key: None,
        }
    }

    /// A commented server.toml to start from.
    pub fn example() -> &'static str {
        include_str!("../../../packaging/server.toml")
    }
}

/// What a host lets players do. A friend's world from the pause menu has
/// the defaults: anyone who knows the address, sixteen at most.
#[derive(Clone, Debug)]
pub struct Rules {
    pub max_players: usize,
    pub join: Join,
    pub allow: Vec<String>,
    pub ban: Vec<String>,
    pub bring: Bring,
    pub build: bool,
    /// The address passes must name, if this world has one.
    pub address: String,
    pub links: BTreeMap<String, String>,
    /// The market's public key, to check passes with.
    pub market_key: Option<String>,
}

impl Default for Rules {
    fn default() -> Self {
        Rules { max_players: 16, join: Join::Anyone, allow: vec![], ban: vec![], bring: Bring::Anyone, build: true, address: String::new(), links: BTreeMap::new(), market_key: None }
    }
}

/// Who the market says a player is.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Pass {
    pub handle: String,
    pub name: String,
    /// The server it's for ("" for any).
    #[serde(default)]
    pub aud: String,
    /// Unix seconds.
    pub exp: i64,
}

pub fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Two ways of writing a server's address that mean the same server.
pub fn same_address(a: &str, b: &str) -> bool {
    fn tidy(s: &str) -> String {
        let s = s.trim().trim_end_matches('/').to_lowercase();
        let (tls, rest) = match s.split_once("://") {
            Some((scheme, rest)) => (scheme == "wss" || scheme == "https", rest.to_string()),
            None => (false, s),
        };
        // the ports a game adds when none is typed
        let default = if tls { ":443" } else { ":47126" };
        rest.strip_suffix(default).map(|r| r.to_string()).unwrap_or(rest)
    }
    tidy(a) == tidy(b)
}

impl Pass {
    /// A pass as the market writes it: the JSON, a dot, the signature.
    pub fn open(text: &str, key: &str, address: &str) -> Result<Pass, &'static str> {
        let (body, sig) = text.rsplit_once('.').ok_or("that isn't a pass")?;
        use base64::Engine;
        let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD;
        let json = b64.decode(body).map_err(|_| "that isn't a pass")?;
        let json = String::from_utf8(json).map_err(|_| "that isn't a pass")?;
        let sig = b64.decode(sig).map_err(|_| "that isn't a pass")?;
        let sig = base64::engine::general_purpose::STANDARD.encode(sig);
        if !crate::share::verify(key, &json, &sig) {
            return Err("the market didn't sign that pass");
        }
        let p: Pass = serde_json::from_str(&json).map_err(|_| "that isn't a pass")?;
        if p.exp < now() {
            return Err("that pass ran out; join again");
        }
        if !p.aud.is_empty() && !address.is_empty() && !same_address(&p.aud, address) {
            return Err("that pass is for another address");
        }
        Ok(p)
    }
}

impl Rules {
    /// Whether someone may come in, and as whom: (name, handle). `players`
    /// is how many are here already.
    pub fn admit(&self, name: &str, pass: Option<&str>, players: usize) -> Result<(String, Option<String>), String> {
        if players >= self.max_players {
            return Err(format!("This world is full ({} players).", self.max_players));
        }
        let pass = match (pass, &self.market_key) {
            (Some(p), Some(k)) if !p.is_empty() => match Pass::open(p, k, &self.address) {
                Ok(p) => Some(p),
                // a bad pass is only a problem where a pass is needed
                Err(e) if self.join != Join::Anyone => {
                    let hint = if self.address.is_empty() { String::new() } else { format!(" Join with {}.", self.address) };
                    return Err(format!("The market couldn't vouch for you: {e}.{hint}"));
                }
                Err(_) => None,
            },
            _ => None,
        };
        let handle = pass.as_ref().map(|p| p.handle.to_lowercase());
        if let Some(h) = &handle {
            if self.ban.contains(h) {
                return Err("You can't join this world.".into());
            }
        }
        match self.join {
            Join::Anyone => {}
            Join::SignedIn if handle.is_none() => return Err("This world is for players signed in to the market. Check Settings, then join again.".into()),
            Join::SignedIn => {}
            Join::List => match &handle {
                Some(h) if self.allow.contains(h) => {}
                Some(_) => return Err("This world only lets in the players on its list.".into()),
                None => return Err("This world only lets in the players on its list, signed in to the market.".into()),
            },
        }
        let shown = match &pass {
            Some(p) => p.name.chars().take(20).collect(),
            None => name.chars().take(20).collect(),
        };
        Ok((shown, handle))
    }

    pub fn may_bring(&self, handle: Option<&str>) -> bool {
        match self.bring {
            Bring::Anyone => true,
            Bring::SignedIn => handle.is_some(),
            Bring::Nobody => false,
        }
    }
}

// ---------- how long a tick takes ----------

/// Each frame's time in the world's own systems, for the status page and
/// soak tests.
#[derive(bevy::prelude::Resource)]
pub struct TickTimes {
    started: std::time::Instant,
    frame: Option<std::time::Instant>,
    recent: std::collections::VecDeque<f32>,
}

impl Default for TickTimes {
    fn default() -> Self {
        Self { started: std::time::Instant::now(), frame: None, recent: Default::default() }
    }
}

impl TickTimes {
    pub fn average_ms(&self) -> f32 {
        if self.recent.is_empty() {
            return 0.0;
        }
        (self.recent.iter().sum::<f32>() / self.recent.len() as f32 * 100.0).round() / 100.0
    }

    pub fn max_ms(&self) -> f32 {
        (self.recent.iter().copied().fold(0.0, f32::max) * 100.0).round() / 100.0
    }

    pub fn up_seconds(&self) -> u64 {
        self.started.elapsed().as_secs()
    }
}

pub fn tick_begins(mut t: bevy::prelude::ResMut<TickTimes>) {
    t.frame = Some(std::time::Instant::now());
}

pub fn tick_ends(mut t: bevy::prelude::ResMut<TickTimes>) {
    if let Some(f) = t.frame.take() {
        let ms = f.elapsed().as_secs_f32() * 1000.0;
        t.recent.push_back(ms);
        // about ten seconds at 30 ticks a second
        if t.recent.len() > 300 {
            t.recent.pop_front();
        }
    }
}

// ---------- boards ----------

/// One player's best on a board.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Score {
    pub name: String,
    /// The market handle when the player had a pass.
    #[serde(default)]
    pub handle: Option<String>,
    pub value: f64,
    pub at: i64,
}

/// A board: higher is better unless `lower` says otherwise (times).
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Board {
    pub lower: bool,
    pub scores: Vec<Score>,
    /// Changed since the market last heard.
    #[serde(skip)]
    pub dirty: bool,
}

pub const BOARD_SIZE: usize = 100;

/// Every creation's boards in this world: (creation id, board name).
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Boards {
    pub boards: BTreeMap<String, BTreeMap<String, Board>>,
}

impl Boards {
    pub fn load(path: &Path) -> Boards {
        std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }

    pub fn save(&self, path: &Path) {
        let tmp = path.with_extension("tmp");
        if std::fs::write(&tmp, serde_json::to_string(self).unwrap_or_default()).is_ok() {
            let _ = std::fs::rename(tmp, path);
        }
    }

    /// Keeps a player's best. True when it's a new best.
    pub fn submit(&mut self, creation: &str, board: &str, lower: bool, name: &str, handle: Option<&str>, value: f64) -> bool {
        if !value.is_finite() || board.is_empty() || board.len() > 40 {
            return false;
        }
        let creation_boards = self.boards.entry(creation.to_string()).or_default();
        if creation_boards.len() >= 16 && !creation_boards.contains_key(board) {
            return false;
        }
        let b = creation_boards.entry(board.to_string()).or_insert_with(|| Board { lower, ..Default::default() });
        b.lower = lower;
        let better = |a: f64, b: f64| if lower { a < b } else { a > b };
        // a player is the same player by handle, or by name without one
        let same = |s: &Score| match (handle, &s.handle) {
            (Some(h), Some(sh)) => h == sh,
            (None, None) => s.name == name,
            _ => false,
        };
        let improved = match b.scores.iter_mut().find(|s| same(s)) {
            Some(s) if better(value, s.value) => {
                s.value = value;
                s.name = name.to_string();
                s.at = now();
                true
            }
            Some(_) => false,
            None => {
                b.scores.push(Score { name: name.to_string(), handle: handle.map(|h| h.to_string()), value, at: now() });
                true
            }
        };
        if improved {
            b.scores.sort_by(|x, y| if lower { x.value.total_cmp(&y.value) } else { y.value.total_cmp(&x.value) });
            b.scores.truncate(BOARD_SIZE);
            b.dirty = true;
        }
        improved
    }

    pub fn top(&self, creation: &str, board: &str, n: usize) -> Vec<Score> {
        self.boards.get(creation).and_then(|c| c.get(board)).map(|b| b.scores.iter().take(n).cloned().collect()).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_server_toml_is_a_working_server() {
        let c: ServerConfig = toml::from_str("").unwrap();
        assert_eq!(c, ServerConfig::default());
        assert!(c.check().is_ok());
        let example: ServerConfig = toml::from_str(ServerConfig::example()).unwrap();
        assert!(example.check().is_ok(), "the example works as it is");
        assert!(toml::from_str::<ServerConfig>("nmae = \"typo\"").is_err(), "typos are caught");
        let listed: ServerConfig = toml::from_str("list = true").unwrap();
        assert!(listed.check().is_err(), "listing needs an address and a token");
    }

    #[test]
    fn rules_let_in_who_they_should() {
        let mut r = Rules { max_players: 2, ..Default::default() };
        assert_eq!(r.admit("Jo", None, 0).unwrap(), ("Jo".to_string(), None));
        assert!(r.admit("Jo", None, 2).is_err(), "full");
        r.join = Join::SignedIn;
        assert!(r.admit("Jo", None, 0).is_err(), "no pass");
        r.join = Join::List;
        r.allow = vec!["ada".into()];
        assert!(r.admit("Ada", Some("nonsense"), 0).is_err(), "a pass that isn't one");
        r.bring = Bring::SignedIn;
        assert!(!r.may_bring(None) && r.may_bring(Some("ada")));
        assert!(same_address("Riverbend.example.org", "ws://riverbend.example.org:47126/"));
        assert!(same_address("wss://riverbend.example.org", "https://riverbend.example.org:443"));
        assert!(!same_address("wss://riverbend.example.org", "riverbend.example.org:47443"));
    }

    #[test]
    fn boards_keep_each_players_best() {
        let mut b = Boards::default();
        assert!(b.submit("rodeo", "longest ride", false, "Ada", Some("ada"), 12.0));
        assert!(!b.submit("rodeo", "longest ride", false, "Ada", Some("ada"), 8.0), "not a best");
        assert!(b.submit("rodeo", "longest ride", false, "Bo", None, 20.0));
        assert!(b.submit("rodeo", "fastest lap", true, "Bo", None, 31.5));
        assert!(b.submit("rodeo", "fastest lap", true, "Bo", None, 29.0), "lower is better here");
        let top = b.top("rodeo", "longest ride", 10);
        assert_eq!(top.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["Bo", "Ada"]);
        assert_eq!(b.top("rodeo", "fastest lap", 1)[0].value, 29.0);
        assert!(!b.submit("rodeo", "x", false, "Bo", None, f64::NAN));
    }
}
