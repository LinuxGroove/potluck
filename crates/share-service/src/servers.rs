//! Servers: passes that tell a server who a player is, the server list
//! (servers send a heartbeat; the market checks they answer before listing
//! them), and the boards creations keep on them.

use std::sync::OnceLock;

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::codes;
use crate::db::{self, now, Account};
use crate::market::{fail, App, R};

/// A pass lasts long enough to join, not to keep.
pub const PASS_SECONDS: i64 = 300;
/// A server drops off the list this long after its last heartbeat.
pub const LISTED_FOR: i64 = 180;
/// How often the market looks at a server's status page again.
const PROBE_EVERY: i64 = 600;
const MAX_SERVERS: i64 = 5;

pub const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS servers (
    id INTEGER PRIMARY KEY,
    owner TEXT NOT NULL REFERENCES accounts(user_id),
    -- the token's hash: the token itself is shown once
    token_hash TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    about TEXT NOT NULL DEFAULT '',
    address TEXT NOT NULL DEFAULT '',
    players INTEGER NOT NULL DEFAULT 0,
    max_players INTEGER NOT NULL DEFAULT 0,
    join_rule TEXT NOT NULL DEFAULT 'anyone',
    creations TEXT NOT NULL DEFAULT '[]',
    version TEXT NOT NULL DEFAULT '',
    created INTEGER NOT NULL,
    last_seen INTEGER NOT NULL DEFAULT 0,
    -- when the market last reached its status page, and whether it answered
    probed INTEGER NOT NULL DEFAULT 0,
    reachable INTEGER NOT NULL DEFAULT 0,
    -- a moderator took it off the list
    hidden INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS scores (
    server INTEGER NOT NULL REFERENCES servers(id) ON DELETE CASCADE,
    creation TEXT NOT NULL,
    board TEXT NOT NULL,
    lower INTEGER NOT NULL DEFAULT 0,
    who TEXT NOT NULL,
    handle TEXT,
    value REAL NOT NULL,
    at INTEGER NOT NULL,
    PRIMARY KEY (server, creation, board, who)
);
"#;

#[derive(Serialize, Deserialize)]
struct PassBody {
    handle: String,
    name: String,
    aud: String,
    exp: i64,
    /// Under 13, as their game says: listed servers that aren't for all
    /// ages turn them away.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    young: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct Server {
    pub id: i64,
    pub name: String,
    pub about: String,
    pub address: String,
    pub players: i64,
    pub max_players: i64,
    pub join: String,
    pub creations: Vec<Value>,
    pub version: String,
    pub last_seen: i64,
    pub reachable: bool,
    pub hidden: bool,
    pub all_ages: bool,
}

fn server_row(r: &rusqlite::Row) -> rusqlite::Result<Server> {
    let creations: String = r.get(7)?;
    Ok(Server {
        id: r.get(0)?,
        name: r.get(1)?,
        about: r.get(2)?,
        address: r.get(3)?,
        players: r.get(4)?,
        max_players: r.get(5)?,
        join: r.get(6)?,
        creations: serde_json::from_str(&creations).unwrap_or_default(),
        version: r.get(8)?,
        last_seen: r.get(9)?,
        reachable: r.get(10)?,
        hidden: r.get(11)?,
        all_ages: r.get(12)?,
    })
}

const SERVER_COLS: &str = "s.id, s.name, s.about, s.address, s.players, s.max_players, s.join_rule, s.creations, s.version, s.last_seen, s.reachable, s.hidden, s.all_ages";

#[derive(Deserialize, Default)]
pub struct Heartbeat {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub about: String,
    #[serde(default)]
    pub address: String,
    #[serde(default)]
    pub players: i64,
    #[serde(default)]
    pub max_players: i64,
    #[serde(default)]
    pub join: String,
    #[serde(default)]
    pub creations: Vec<Value>,
    #[serde(default)]
    pub version: String,
    /// The server is for players of all ages, under 13 too.
    #[serde(default)]
    pub all_ages: bool,
}

#[derive(Deserialize)]
pub struct BoardPost {
    /// The creation's code, or `base` for one of the base game's.
    #[serde(default)]
    pub creation: String,
    #[serde(default)]
    pub base: Option<String>,
    pub board: String,
    #[serde(default)]
    pub lower: bool,
    pub scores: Vec<ScorePost>,
}

#[derive(Deserialize)]
pub struct ScorePost {
    pub name: String,
    #[serde(default)]
    pub handle: Option<String>,
    pub value: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct Score {
    pub name: String,
    pub handle: Option<String>,
    pub value: f64,
    pub server: String,
    pub at: i64,
}

#[derive(Clone, Debug, Serialize)]
pub struct Board {
    pub name: String,
    pub lower: bool,
    pub scores: Vec<Score>,
}

fn clip(s: &str, n: usize) -> String {
    s.trim().chars().filter(|c| !c.is_control()).take(n).collect()
}

/// The web page a server's address has: wss://host → https://host/status.json.
pub fn status_url(address: &str) -> Option<String> {
    let a = address.trim().trim_end_matches('/');
    let (scheme, rest) = if let Some(r) = a.strip_prefix("wss://").or_else(|| a.strip_prefix("https://")) {
        ("https", r)
    } else if let Some(r) = a.strip_prefix("ws://").or_else(|| a.strip_prefix("http://")) {
        ("http", r)
    } else {
        ("http", a)
    };
    if rest.is_empty() || rest.contains('/') || rest.contains('@') || rest.chars().any(|c| c.is_whitespace()) {
        return None;
    }
    // a bare host gets the game's port, as the game does
    let host_port = if scheme == "http" && !rest.contains(':') { format!("{rest}:47126") } else { rest.to_string() };
    Some(format!("{scheme}://{host_port}/status.json"))
}

fn probe_client() -> &'static reqwest::Client {
    static C: OnceLock<reqwest::Client> = OnceLock::new();
    C.get_or_init(|| reqwest::Client::builder().timeout(std::time::Duration::from_secs(5)).redirect(reqwest::redirect::Policy::none()).build().expect("a client"))
}

/// Addresses the market won't look at: its own machine and private
/// networks, unless the config allows them (tests, a LAN market).
async fn private_address(url: &str) -> bool {
    let Ok(u) = url::Url::parse(url) else { return true };
    let Some(host) = u.host_str() else { return true };
    let port = u.port_or_known_default().unwrap_or(80);
    let Ok(addrs) = tokio::net::lookup_host((host, port)).await else { return true };
    let addrs: Vec<_> = addrs.collect();
    addrs.is_empty()
        || addrs.iter().any(|a| match a.ip() {
            std::net::IpAddr::V4(v) => v.is_loopback() || v.is_private() || v.is_link_local() || v.is_unspecified() || v.is_broadcast() || v.is_documentation(),
            std::net::IpAddr::V6(v) => v.is_loopback() || v.is_unspecified() || (v.segments()[0] & 0xfe00) == 0xfc00 || (v.segments()[0] & 0xffc0) == 0xfe80,
        })
}

impl App {
    /// A pass for one server (or any, with an empty address): the market's
    /// signature over who this is, for a few minutes.
    pub fn pass(&self, me: &Account, server: &str) -> R<String> {
        use base64::Engine;
        let body = PassBody { handle: me.handle.clone(), name: me.name.clone(), aud: clip(server, 200), exp: now() + PASS_SECONDS, young: me.young };
        let json = serde_json::to_string(&body).map_err(|e| crate::market::Fail { status: 500, message: e.to_string() })?;
        let sig = codes::sign(&self.key, &json);
        let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD;
        let sig = base64::engine::general_purpose::STANDARD.decode(sig).unwrap_or_default();
        Ok(format!("{}.{}", b64.encode(json), b64.encode(sig)))
    }

    /// A new server for this maker: its row and a token, shown once.
    pub fn add_server(&self, me: &Account, name: &str) -> R<(i64, String)> {
        let name = clip(name, 40);
        if name.is_empty() {
            return fail(400, "a server needs a name");
        }
        if !self.words.find(&name).is_empty() {
            return fail(400, "that name has words the market doesn't allow");
        }
        let db = self.db();
        let n: i64 = db.query_row("SELECT COUNT(*) FROM servers WHERE owner = ?1", [&me.user_id], |r| r.get(0))?;
        if n >= MAX_SERVERS {
            return fail(400, "you have as many servers as the market lists for one maker");
        }
        let token = codes::token();
        db.execute("INSERT INTO servers (owner, token_hash, name, created) VALUES (?1, ?2, ?3, ?4)", params![me.user_id, codes::sha256_hex(token.as_bytes()), name, now()])?;
        let id = db.last_insert_rowid();
        db::log(&db, &me.user_id, "server", &id.to_string(), &name);
        Ok((id, token))
    }

    pub fn remove_server(&self, me: &Account, id: i64) -> R<()> {
        let db = self.db();
        let n = db.execute("DELETE FROM servers WHERE id = ?1 AND owner = ?2", params![id, me.user_id])?;
        if n == 0 {
            return fail(404, "that isn't one of your servers");
        }
        Ok(())
    }

    pub fn my_servers(&self, me: &Account) -> R<Vec<Server>> {
        let db = self.db();
        let mut st = db.prepare(&format!("SELECT {SERVER_COLS} FROM servers s WHERE s.owner = ?1 ORDER BY s.id"))?;
        let list = st.query_map([&me.user_id], server_row)?.flatten().collect();
        Ok(list)
    }

    fn server_by_token(&self, token: &str) -> R<(i64, String)> {
        let db = self.db();
        let found: Option<(i64, String)> = db
            .query_row("SELECT s.id, s.owner FROM servers s JOIN accounts a ON a.user_id = s.owner WHERE s.token_hash = ?1 AND a.banned = 0", [codes::sha256_hex(token.as_bytes())], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?;
        found.ok_or(crate::market::Fail { status: 401, message: "that server token isn't one the market made, or it was removed".into() })
    }

    /// A server saying it's up. It's listed once the market has reached its
    /// status page at the address it gives.
    pub async fn heartbeat(&self, token: &str, hb: Heartbeat) -> R<Value> {
        let (id, _) = self.server_by_token(token)?;
        let name = clip(&hb.name, 40);
        let about = clip(&hb.about, 200);
        let address = clip(&hb.address, 200);
        if !self.words.find(&name).is_empty() || !self.words.find(&about).is_empty() {
            return fail(400, "the server's name or about has words the market doesn't allow");
        }
        let join = match hb.join.as_str() {
            "signed-in" | "list" => hb.join.clone(),
            _ => "anyone".to_string(),
        };
        let creations: Vec<Value> = hb.creations.iter().take(24).map(|c| json!({ "name": clip(c["name"].as_str().unwrap_or(""), 40), "code": c["code"].as_str().map(codes::clean) })).collect();
        let (probed, was_address): (i64, String) = self.db().query_row("SELECT probed, address FROM servers WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?)))?;
        let mut reachable: Option<bool> = None;
        if address != was_address || now() - probed > PROBE_EVERY {
            reachable = Some(match status_url(&address) {
                Some(url) if self.cfg.private_servers || !private_address(&url).await => match probe_client().get(&url).send().await {
                    Ok(r) if r.status().is_success() => r.json::<Value>().await.map(|v| v.get("protocol").is_some() || v.get("players").is_some()).unwrap_or(false),
                    _ => false,
                },
                _ => false,
            });
        }
        let db = self.db();
        db.execute(
            "UPDATE servers SET name = ?2, about = ?3, address = ?4, players = ?5, max_players = ?6, join_rule = ?7, creations = ?8, version = ?9, last_seen = ?10, all_ages = ?11 WHERE id = ?1",
            params![id, name, about, address, hb.players.clamp(0, 10_000), hb.max_players.clamp(0, 10_000), join, Value::Array(creations).to_string(), clip(&hb.version, 40), now(), hb.all_ages],
        )?;
        if let Some(ok) = reachable {
            db.execute("UPDATE servers SET probed = ?2, reachable = ?3 WHERE id = ?1", params![id, now(), ok])?;
        }
        let listed: bool = db.query_row("SELECT reachable AND NOT hidden FROM servers WHERE id = ?1", [id], |r| r.get(0))?;
        Ok(json!({
            "listed": listed,
            "message": if listed { "listed on the market" } else { "not listed: the market couldn't reach this server's status page at its address" },
        }))
    }

    /// Servers seen lately and reachable, busiest first.
    pub fn listed_servers(&self) -> R<Vec<Server>> {
        let db = self.db();
        let mut st = db.prepare(&format!(
            "SELECT {SERVER_COLS} FROM servers s JOIN accounts a ON a.user_id = s.owner WHERE s.last_seen >= ?1 AND s.reachable = 1 AND s.hidden = 0 AND a.banned = 0 ORDER BY s.players DESC, s.last_seen DESC LIMIT 200"
        ))?;
        let list = st.query_map([now() - LISTED_FOR], server_row)?.flatten().collect();
        Ok(list)
    }

    /// A server's boards: each player's best, as the server keeps them.
    pub fn post_boards(&self, token: &str, boards: Vec<BoardPost>) -> R<usize> {
        let (id, _) = self.server_by_token(token)?;
        let db = self.db();
        let mut n = 0;
        for b in boards.into_iter().take(64) {
            let creation = match &b.base {
                Some(base) if b.creation.is_empty() => match db::creation_by_base(&db, base)? {
                    Some(c) => c.code,
                    None => continue,
                },
                _ => codes::clean(&b.creation),
            };
            let board = clip(&b.board, 40);
            if creation.len() != 8 || board.is_empty() {
                continue;
            }
            for s in b.scores.into_iter().take(100) {
                if !s.value.is_finite() {
                    continue;
                }
                let who = s.handle.clone().map(|h| format!("@{h}")).unwrap_or_else(|| clip(&s.name, 32));
                db.execute(
                    "INSERT INTO scores (server, creation, board, lower, who, handle, value, at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                     ON CONFLICT (server, creation, board, who) DO UPDATE SET value = excluded.value, at = excluded.at, lower = excluded.lower",
                    params![id, creation, board, b.lower, who, s.handle.map(|h| clip(&h, 40)), s.value, now()],
                )?;
                n += 1;
            }
        }
        Ok(n)
    }

    /// A creation's boards across every server, best first.
    pub fn boards_for(&self, code: &str) -> R<Vec<Board>> {
        let db = self.db();
        let mut st = db.prepare("SELECT DISTINCT board, lower FROM scores WHERE creation = ?1 ORDER BY board LIMIT 16")?;
        let names: Vec<(String, bool)> = st.query_map([code], |r| Ok((r.get(0)?, r.get(1)?)))?.flatten().collect();
        let mut out = vec![];
        for (name, lower) in names {
            let order = if lower { "ASC" } else { "DESC" };
            let mut st = db.prepare(&format!(
                "SELECT COALESCE(a.name, sc.who), sc.handle, sc.value, s.name, sc.at FROM scores sc JOIN servers s ON s.id = sc.server
                 LEFT JOIN accounts a ON a.handle = sc.handle
                 WHERE sc.creation = ?1 AND sc.board = ?2 AND s.hidden = 0 ORDER BY sc.value {order} LIMIT 10"
            ))?;
            let scores = st.query_map(params![code, name], |r| Ok(Score { name: r.get(0)?, handle: r.get(1)?, value: r.get(2)?, server: r.get(3)?, at: r.get(4)? }))?.flatten().collect();
            out.push(Board { name, lower, scores });
        }
        Ok(out)
    }

    /// A moderator takes a server off the list, and its scores off boards.
    pub fn hide_server(&self, moderator: &Account, id: i64, hidden: bool) -> R<()> {
        if !moderator.moderator {
            return fail(403, "only moderators can do that");
        }
        let db = self.db();
        db.execute("UPDATE servers SET hidden = ?2 WHERE id = ?1", params![id, hidden])?;
        db::log(&db, &moderator.user_id, if hidden { "hide-server" } else { "show-server" }, &id.to_string(), "");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_pages_are_where_the_game_looks() {
        assert_eq!(status_url("wss://potluck.example.org").as_deref(), Some("https://potluck.example.org/status.json"));
        assert_eq!(status_url("wss://potluck.example.org:47443/").as_deref(), Some("https://potluck.example.org:47443/status.json"));
        assert_eq!(status_url("192.168.1.20").as_deref(), Some("http://192.168.1.20:47126/status.json"));
        assert_eq!(status_url("ws://host:9000").as_deref(), Some("http://host:9000/status.json"));
        assert_eq!(status_url("evil.example/admin"), None);
        assert_eq!(status_url("user@host"), None);
        assert_eq!(status_url(""), None);
    }
}
