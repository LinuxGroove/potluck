//! Worlds shared from home. A player's game opens its world to the internet
//! peer to peer (iroh: QUIC dialled by a key, through home routers) and the
//! market only introduces players: it gives the world a short code, keeps
//! the host's key and addresses while its heartbeat lasts, and answers a
//! friend who types the code with the host's key and a pass that names
//! both of them. Who may have that pass is decided here: friends only, or
//! anyone with the code, and never a stranger's world for a younger player.
//! The relay that carries the few connections that can't go direct asks
//! `relay_allows` before it carries anything.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::codes;
use crate::db::{self, now, Account};
use crate::friends::{are_friends, blocked_between};
use crate::market::{fail, App, Fail, R};

/// Letters in a world code: short enough to read across a room.
pub const LENGTH: usize = 6;
/// A world is open this long after its last heartbeat (the game sends one a minute).
pub const LIVE_FOR: i64 = 150;
/// The relay carries a host's key this long after its heartbeat.
const HOST_RELAY: i64 = 300;
/// And a player's key this long after their pass: one long evening.
const PLAYER_RELAY: i64 = 8 * 3600;
/// Worlds one account keeps codes for.
const MAX_WORLDS: i64 = 8;
/// World codes that aren't anything one account may type in an hour.
const TRIES: u32 = 20;

pub const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS worlds (
    code TEXT PRIMARY KEY,
    owner TEXT NOT NULL REFERENCES accounts(user_id),
    -- the game's name for the world, so the same world keeps its code
    slot TEXT NOT NULL,
    name TEXT NOT NULL,
    -- 'friends' or 'code' (anyone with the code)
    who TEXT NOT NULL DEFAULT 'friends',
    -- the host's iroh key and the addresses it can be reached on, while shared
    key TEXT NOT NULL DEFAULT '',
    addrs TEXT NOT NULL DEFAULT '[]',
    players INTEGER NOT NULL DEFAULT 0,
    max_players INTEGER NOT NULL DEFAULT 0,
    beat INTEGER NOT NULL DEFAULT 0,
    created INTEGER NOT NULL,
    UNIQUE (owner, slot)
);
CREATE TABLE IF NOT EXISTS world_invites (
    world TEXT NOT NULL REFERENCES worlds(code) ON DELETE CASCADE,
    friend TEXT NOT NULL REFERENCES accounts(user_id),
    sent INTEGER NOT NULL,
    PRIMARY KEY (world, friend)
);
-- keys the relay may carry, until when
CREATE TABLE IF NOT EXISTS relay_keys (
    key TEXT PRIMARY KEY,
    until INTEGER NOT NULL
);
"#;

/// A host's heartbeat: the first opens the world, and each keeps it open.
#[derive(Deserialize, Default)]
pub struct Beat {
    /// The game's name for this world (its seed, say).
    pub slot: String,
    #[serde(default)]
    pub name: String,
    /// "friends" or "code".
    #[serde(default)]
    pub who: String,
    /// The host's iroh key, as hex.
    pub key: String,
    /// Addresses it listens on, for friends on the same network.
    #[serde(default)]
    pub addrs: Vec<String>,
    #[serde(default)]
    pub players: i64,
    #[serde(default)]
    pub max_players: i64,
    /// A fresh code: the old one stops working.
    #[serde(default)]
    pub new_code: bool,
}

/// A world as the game shows it in a list.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Open {
    pub code: String,
    pub name: String,
    pub by: String,
    pub handle: String,
    pub players: i64,
    pub max_players: i64,
    /// The host invited this player.
    pub invited: bool,
}

/// What a friend needs to reach a world.
#[derive(Clone, Debug, Serialize)]
pub struct Way {
    pub code: String,
    pub name: String,
    pub by: String,
    pub handle: String,
    pub key: String,
    pub addrs: Vec<String>,
    pub relay: String,
    pub pass: String,
}

fn tries() -> &'static Mutex<HashMap<String, (u32, i64)>> {
    static TRIED: OnceLock<Mutex<HashMap<String, (u32, i64)>>> = OnceLock::new();
    TRIED.get_or_init(Default::default)
}

fn too_many(who: &str) -> bool {
    let mut t = tries().lock().unwrap();
    let n = now();
    t.retain(|_, (_, first)| *first > n - 3600);
    t.get(who).is_some_and(|(count, _)| *count >= TRIES)
}

fn missed(who: &str) {
    tries().lock().unwrap().entry(who.to_string()).or_insert((0, now())).0 += 1;
}

fn clip(s: &str, n: usize) -> String {
    s.trim().chars().filter(|c| !c.is_control()).take(n).collect()
}

/// An iroh key as the game sends it: 64 hex digits.
pub fn is_key(k: &str) -> bool {
    k.len() == 64 && k.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

/// `K7FQ2M` as `K7F-Q2M`.
pub fn pretty(code: &str) -> String {
    if code.len() == LENGTH { format!("{}-{}", &code[..3], &code[3..]) } else { code.to_string() }
}

fn allow_relay(db: &rusqlite::Connection, key: &str, until: i64) -> rusqlite::Result<()> {
    db.execute("INSERT INTO relay_keys (key, until) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET until = max(until, excluded.until)", params![key, until])?;
    Ok(())
}

struct Row {
    code: String,
    owner: String,
    name: String,
    who: String,
    key: String,
    addrs: String,
    players: i64,
    max_players: i64,
    beat: i64,
}

fn world(db: &rusqlite::Connection, code: &str) -> rusqlite::Result<Option<Row>> {
    db.query_row("SELECT code, owner, name, who, key, addrs, players, max_players, beat FROM worlds WHERE code = ?1", [code], |r| {
        Ok(Row { code: r.get(0)?, owner: r.get(1)?, name: r.get(2)?, who: r.get(3)?, key: r.get(4)?, addrs: r.get(5)?, players: r.get(6)?, max_players: r.get(7)?, beat: r.get(8)? })
    })
    .optional()
}

impl App {
    /// The relay this market's worlds use, as the game should dial it.
    pub fn relay_url(&self) -> String {
        self.cfg.relay.trim_end_matches('/').to_string()
    }

    /// Opens a world, or keeps it open: its code (the same one each time,
    /// unless `new_code`) and where its page is.
    pub fn share_world(&self, me: &Account, b: Beat) -> R<Value> {
        if me.banned {
            return fail(403, "this account can't share worlds");
        }
        if !is_key(&b.key) {
            return fail(400, "that isn't a key the game makes");
        }
        let slot = clip(&b.slot, 64);
        if slot.is_empty() {
            return fail(400, "which world?");
        }
        let mut name = clip(&b.name, 40);
        if name.is_empty() || !self.words.find(&name).is_empty() {
            name = format!("{}'s world", me.name);
        }
        // a younger player's world is for their friends
        let who = if b.who == "code" && !me.young { "code" } else { "friends" };
        let addrs: Vec<String> = b.addrs.iter().take(8).map(|a| clip(a, 64)).filter(|a| a.parse::<std::net::SocketAddr>().is_ok()).collect();
        let db = self.db();
        let tx = db.unchecked_transaction()?;
        let had: Option<String> = tx.query_row("SELECT code FROM worlds WHERE owner = ?1 AND slot = ?2", params![me.user_id, slot], |r| r.get(0)).optional()?;
        let code = match had {
            Some(c) if !b.new_code => c,
            had => {
                if had.is_none() {
                    let n: i64 = tx.query_row("SELECT COUNT(*) FROM worlds WHERE owner = ?1", [&me.user_id], |r| r.get(0))?;
                    if n >= MAX_WORLDS {
                        // the one shared longest ago makes room
                        let oldest: String = tx.query_row("SELECT code FROM worlds WHERE owner = ?1 ORDER BY beat LIMIT 1", [&me.user_id], |r| r.get(0))?;
                        tx.execute("DELETE FROM world_invites WHERE world = ?1", [&oldest])?;
                        tx.execute("DELETE FROM worlds WHERE code = ?1", [&oldest])?;
                    }
                }
                let mut fresh = None;
                for _ in 0..8 {
                    let c = codes::letters(LENGTH);
                    if !tx.prepare_cached("SELECT 1 FROM worlds WHERE code = ?1")?.exists([&c])? {
                        fresh = Some(c);
                        break;
                    }
                }
                let Some(fresh) = fresh else { return fail(500, "couldn't make a code: try again") };
                // a new code: invites go with the old one
                if let Some(old) = had {
                    tx.execute("DELETE FROM world_invites WHERE world = ?1", [&old])?;
                    tx.execute("DELETE FROM worlds WHERE code = ?1", [&old])?;
                }
                tx.execute("INSERT INTO worlds (code, owner, slot, name, created) VALUES (?1, ?2, ?3, ?4, ?5)", params![fresh, me.user_id, slot, name, now()])?;
                fresh
            }
        };
        let t = now();
        tx.execute(
            "UPDATE worlds SET name = ?2, who = ?3, key = ?4, addrs = ?5, players = ?6, max_players = ?7, beat = ?8 WHERE code = ?1",
            params![code, name, who, b.key, serde_json::to_string(&addrs).unwrap_or_default(), b.players.clamp(0, 999), b.max_players.clamp(0, 999), t],
        )?;
        allow_relay(&tx, &b.key, t + HOST_RELAY)?;
        tx.execute("DELETE FROM relay_keys WHERE until < ?1", [t])?;
        tx.commit()?;
        Ok(json!({
            "code": code, "pretty": pretty(&code), "url": format!("{}/w/{code}", self.cfg.public_url),
            "who": who, "name": name, "relay": self.relay_url(), "market_key": codes::public_key(&self.key),
        }))
    }

    /// Closes a world: its code keeps, for next time.
    pub fn stop_world(&self, me: &Account, code: &str) -> R<()> {
        let code = codes::clean(code);
        let db = self.db();
        db.execute("UPDATE worlds SET key = '', addrs = '[]', players = 0, beat = 0 WHERE code = ?1 AND owner = ?2", params![code, me.user_id])?;
        Ok(())
    }

    /// Whether a player may come into a world: Ok, or why not, in words
    /// for them. The host is checked as much as the world.
    fn may_join(&self, db: &rusqlite::Connection, me: &Account, w: &Row) -> R<()> {
        if w.owner == me.user_id {
            return fail(409, "that's your own world");
        }
        if w.key.is_empty() || w.beat < now() - LIVE_FOR {
            return fail(410, "that world isn't open right now: ask its host to share it again");
        }
        let host = db::account(db, &w.owner)?.ok_or(Fail { status: 404, message: "that world's host has gone".into() })?;
        if host.banned || me.banned || blocked_between(db, &me.user_id, &w.owner)? {
            // as if it weren't there
            return fail(404, "that isn't a world code anyone has shared");
        }
        let friends = are_friends(db, &me.user_id, &w.owner)?;
        if me.young {
            // a friend's world, kept to their friends: never a stranger's
            if !friends {
                return fail(403, format!("you can join {}'s world once you're friends: swap friend codes under Friends", host.name));
            }
            if w.who != "friends" {
                return fail(403, format!("{}'s world is open to anyone with the code just now, so younger players can't join: it works when it's set to Friends", host.name));
            }
        }
        if w.max_players > 0 && w.players >= w.max_players {
            return fail(409, format!("{}'s world is full just now ({} players)", host.name, w.max_players));
        }
        if w.who == "friends" && !friends {
            return fail(403, format!("{}'s world is for their friends: swap friend codes with them under Friends", host.name));
        }
        Ok(())
    }

    /// The way into a world by its code, with a pass for this player that
    /// names the host's key and theirs (`peer`).
    pub fn join_world(&self, me: &Account, code: &str, peer: &str) -> R<Way> {
        if too_many(&me.user_id) {
            return fail(429, "that's a lot of codes that aren't worlds: wait an hour and try again");
        }
        if !is_key(peer) {
            return fail(400, "that isn't a key the game makes");
        }
        let code = codes::clean(code);
        let db = self.db();
        let Some(w) = (if code.len() == LENGTH { world(&db, &code)? } else { None }) else {
            missed(&me.user_id);
            return fail(404, "that isn't a world code anyone has shared");
        };
        self.may_join(&db, me, &w)?;
        let host = db::account(&db, &w.owner)?.ok_or(Fail { status: 404, message: "that world's host has gone".into() })?;
        allow_relay(&db, peer, now() + PLAYER_RELAY)?;
        db.execute("DELETE FROM world_invites WHERE world = ?1 AND friend = ?2", params![w.code, me.user_id])?;
        drop(db);
        let pass = self.peer_pass(me, &w.key, peer)?;
        Ok(Way { code: w.code, name: w.name, by: host.name, handle: host.handle, key: w.key, addrs: serde_json::from_str(&w.addrs).unwrap_or_default(), relay: self.relay_url(), pass })
    }

    /// Tells a friend about a world: it goes to the top of their list.
    pub fn invite(&self, me: &Account, code: &str, handle: &str) -> R<String> {
        let code = codes::clean(code);
        let db = self.db();
        let Some(w) = world(&db, &code)?.filter(|w| w.owner == me.user_id) else { return fail(404, "that isn't one of your worlds") };
        let Some(friend) = db::account_by_handle(&db, handle.trim_start_matches('@'))? else { return fail(404, "nobody has that name on the market") };
        if !are_friends(&db, &me.user_id, &friend.user_id)? {
            return fail(403, "you can invite your friends");
        }
        if friend.young && w.who != "friends" {
            return fail(403, format!("{} is under 13, so they can join when your world is set to Friends", friend.name));
        }
        db.execute("INSERT INTO world_invites (world, friend, sent) VALUES (?1, ?2, ?3) ON CONFLICT(world, friend) DO UPDATE SET sent = excluded.sent", params![w.code, friend.user_id, now()])?;
        Ok(friend.name)
    }

    /// Worlds open now that this player may join: their friends' and the
    /// ones they were invited to, invitations first.
    pub fn open_worlds(&self, me: &Account) -> R<Vec<Open>> {
        let db = self.db();
        let mut st = db.prepare(
            "SELECT w.code, w.name, a.name, a.handle, w.players, w.max_players, w.who,
                    EXISTS (SELECT 1 FROM world_invites i WHERE i.world = w.code AND i.friend = ?1)
             FROM worlds w JOIN friends f ON f.a = ?1 AND f.b = w.owner JOIN accounts a ON a.user_id = w.owner
             WHERE w.key != '' AND w.beat >= ?2 AND a.banned = 0
             ORDER BY 8 DESC, w.beat DESC LIMIT 50",
        )?;
        let rows = st.query_map(params![me.user_id, now() - LIVE_FOR], |r| {
            let who: String = r.get(6)?;
            Ok((Open { code: r.get(0)?, name: r.get(1)?, by: r.get(2)?, handle: r.get(3)?, players: r.get(4)?, max_players: r.get(5)?, invited: r.get(7)? }, who))
        })?;
        let mut out = vec![];
        for row in rows {
            let (o, who) = row?;
            // a younger player only sees the worlds they could join
            if me.young && who != "friends" {
                continue;
            }
            out.push(o);
        }
        Ok(out)
    }

    /// Whether the relay may carry this key: a host's while its world is
    /// open, a player's for a while after their pass.
    pub fn relay_allows(&self, key: &str) -> R<bool> {
        if !is_key(key) {
            return Ok(false);
        }
        let db = self.db();
        Ok(db.prepare_cached("SELECT 1 FROM relay_keys WHERE key = ?1 AND until >= ?2")?.exists(params![key, now()])?)
    }
}
