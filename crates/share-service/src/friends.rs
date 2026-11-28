//! Friends: each account has a friend code to give out (in person, or in a
//! message), and typing someone's code sends them a request. Two players
//! are friends once the other says yes, and either can remove or block the
//! other. Nobody is found by name: a code is the only way in, which is what
//! keeps younger players' friends to people they (and a parent) know. The
//! parent's OK for a younger player's new friends happens in their game,
//! behind the parent PIN; the market never sees the PIN.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

use crate::codes;
use crate::db::{self, now, Account};
use crate::market::{fail, App, R};

/// Letters in a friend code, shown as two groups of four.
pub const LENGTH: usize = 8;
/// Friends one account may have.
const MAX_FRIENDS: i64 = 200;
/// Requests one account may have waiting for an answer.
const MAX_WAITING: i64 = 50;
/// Codes that aren't anyone's one account may type in an hour.
const TRIES: u32 = 20;

pub const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS friend_codes (
    account TEXT PRIMARY KEY REFERENCES accounts(user_id),
    code TEXT NOT NULL UNIQUE,
    created INTEGER NOT NULL
);
-- both ways round: (a, b) and (b, a)
CREATE TABLE IF NOT EXISTS friends (
    a TEXT NOT NULL REFERENCES accounts(user_id),
    b TEXT NOT NULL REFERENCES accounts(user_id),
    since INTEGER NOT NULL,
    PRIMARY KEY (a, b)
);
CREATE TABLE IF NOT EXISTS friend_requests (
    id INTEGER PRIMARY KEY,
    sender TEXT NOT NULL REFERENCES accounts(user_id),
    receiver TEXT NOT NULL REFERENCES accounts(user_id),
    created INTEGER NOT NULL,
    UNIQUE (sender, receiver)
);
CREATE INDEX IF NOT EXISTS friend_requests_receiver ON friend_requests(receiver);
CREATE TABLE IF NOT EXISTS blocks (
    account TEXT NOT NULL REFERENCES accounts(user_id),
    blocked TEXT NOT NULL REFERENCES accounts(user_id),
    created INTEGER NOT NULL,
    PRIMARY KEY (account, blocked)
);
"#;

/// Someone on a player's friends page.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Person {
    pub handle: String,
    pub name: String,
    /// Friends since, or when the request was sent.
    pub since: i64,
}

/// A request waiting for an answer.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Request {
    pub id: i64,
    pub handle: String,
    pub name: String,
    pub sent: i64,
    /// Under 13: the game asks a parent before saying yes.
    pub young: bool,
}

/// Everything on a player's friends page.
#[derive(Clone, Debug, Serialize)]
pub struct Page {
    pub code: String,
    pub friends: Vec<Person>,
    /// Requests sent to this player.
    pub asking: Vec<Request>,
    /// Requests this player sent, still waiting.
    pub asked: Vec<Person>,
    pub blocked: Vec<Person>,
}

/// What typing a friend code did.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Added {
    pub handle: String,
    pub name: String,
    /// They'd asked first, so now you're friends; otherwise it's a request.
    pub friends: bool,
}

pub fn are_friends(db: &Connection, a: &str, b: &str) -> rusqlite::Result<bool> {
    db.prepare_cached("SELECT 1 FROM friends WHERE a = ?1 AND b = ?2")?.exists(params![a, b])
}

/// Either has blocked the other.
pub fn blocked_between(db: &Connection, a: &str, b: &str) -> rusqlite::Result<bool> {
    db.prepare_cached("SELECT 1 FROM blocks WHERE (account = ?1 AND blocked = ?2) OR (account = ?2 AND blocked = ?1)")?.exists(params![a, b])
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

fn people(db: &Connection, sql: &str, me: &str) -> rusqlite::Result<Vec<Person>> {
    let mut st = db.prepare(sql)?;
    let list = st.query_map([me], |r| Ok(Person { handle: r.get(0)?, name: r.get(1)?, since: r.get(2)? }))?.collect();
    list
}

impl App {
    /// This account's friend code, made the first time it's asked for.
    pub fn friend_code(&self, me: &Account) -> R<String> {
        let db = self.db();
        if let Some(code) = db.query_row("SELECT code FROM friend_codes WHERE account = ?1", [&me.user_id], |r| r.get(0)).optional()? {
            return Ok(code);
        }
        drop(db);
        self.new_friend_code(me)
    }

    /// A new friend code: the old one stops working, and friends stay friends.
    pub fn new_friend_code(&self, me: &Account) -> R<String> {
        let db = self.db();
        for _ in 0..8 {
            let code = codes::letters(LENGTH);
            let taken = db.prepare_cached("SELECT 1 FROM friend_codes WHERE code = ?1")?.exists([&code])?;
            if taken {
                continue;
            }
            db.execute(
                "INSERT INTO friend_codes (account, code, created) VALUES (?1, ?2, ?3) ON CONFLICT(account) DO UPDATE SET code = excluded.code, created = excluded.created",
                params![me.user_id, code, now()],
            )?;
            return Ok(code);
        }
        fail(500, "couldn't make a code: try again")
    }

    pub fn friends_page(&self, me: &Account) -> R<Page> {
        let code = self.friend_code(me)?;
        let db = self.db();
        let friends = people(&db, "SELECT a.handle, a.name, f.since FROM friends f JOIN accounts a ON a.user_id = f.b WHERE f.a = ?1 ORDER BY lower(a.name)", &me.user_id)?;
        let mut st = db.prepare("SELECT r.id, a.handle, a.name, r.created, a.young FROM friend_requests r JOIN accounts a ON a.user_id = r.sender WHERE r.receiver = ?1 ORDER BY r.created")?;
        let asking = st.query_map([&me.user_id], |r| Ok(Request { id: r.get(0)?, handle: r.get(1)?, name: r.get(2)?, sent: r.get(3)?, young: r.get(4)? }))?.collect::<Result<Vec<_>, _>>()?;
        let asked = people(&db, "SELECT a.handle, a.name, r.created FROM friend_requests r JOIN accounts a ON a.user_id = r.receiver WHERE r.sender = ?1 ORDER BY r.created", &me.user_id)?;
        let blocked = people(&db, "SELECT a.handle, a.name, b.created FROM blocks b JOIN accounts a ON a.user_id = b.blocked WHERE b.account = ?1 ORDER BY lower(a.name)", &me.user_id)?;
        Ok(Page { code, friends, asking, asked, blocked })
    }

    /// Someone's friend code, typed: a request to them, or friends at once
    /// when they'd already asked.
    pub fn add_friend(&self, me: &Account, code: &str) -> R<Added> {
        if too_many(&me.user_id) {
            return fail(429, "that's a lot of codes that aren't anyone's: wait an hour and try again");
        }
        let code = codes::clean(code);
        let db = self.db();
        let them: Option<String> = if code.len() == LENGTH { db.query_row("SELECT account FROM friend_codes WHERE code = ?1", [&code], |r| r.get(0)).optional()? } else { None };
        let Some(them) = them else {
            missed(&me.user_id);
            return fail(404, "that isn't anyone's friend code: check it with them");
        };
        let other = db::account(&db, &them)?.ok_or(crate::market::Fail { status: 404, message: "that account has gone".into() })?;
        if them == me.user_id {
            return fail(409, "that's your own code: give it to a friend");
        }
        if other.banned || me.banned {
            return fail(403, "that code can't be used");
        }
        if blocked_between(&db, &me.user_id, &them)? {
            // the same answer as a wrong code: a block isn't something to find out
            return fail(404, "that isn't anyone's friend code: check it with them");
        }
        if are_friends(&db, &me.user_id, &them)? {
            return fail(409, format!("you're already friends with {}", other.name));
        }
        let tx = db.unchecked_transaction()?;
        let theirs: Option<i64> = tx.query_row("SELECT id FROM friend_requests WHERE sender = ?1 AND receiver = ?2", params![them, me.user_id], |r| r.get(0)).optional()?;
        let friends = if theirs.is_some() {
            befriend(&tx, &me.user_id, &them)?;
            true
        } else {
            let waiting: i64 = tx.query_row("SELECT COUNT(*) FROM friend_requests WHERE sender = ?1", [&me.user_id], |r| r.get(0))?;
            if waiting >= MAX_WAITING {
                return fail(409, "you have a lot of requests waiting for an answer already");
            }
            tx.execute("INSERT OR IGNORE INTO friend_requests (sender, receiver, created) VALUES (?1, ?2, ?3)", params![me.user_id, them, now()])?;
            false
        };
        db::log(&tx, &me.user_id, if friends { "friends" } else { "friend_request" }, &other.handle, "");
        tx.commit()?;
        Ok(Added { handle: other.handle, name: other.name, friends })
    }

    /// Yes or no to a request someone sent this player.
    pub fn answer_friend(&self, me: &Account, id: i64, yes: bool) -> R<Option<Person>> {
        let db = self.db();
        let sender: Option<String> = db.query_row("SELECT sender FROM friend_requests WHERE id = ?1 AND receiver = ?2", params![id, me.user_id], |r| r.get(0)).optional()?;
        let Some(sender) = sender else { return fail(404, "that request has gone") };
        let tx = db.unchecked_transaction()?;
        tx.execute("DELETE FROM friend_requests WHERE id = ?1", [id])?;
        let out = if yes {
            befriend(&tx, &me.user_id, &sender)?;
            db::account(&tx, &sender)?.map(|a| Person { handle: a.handle, name: a.name, since: now() })
        } else {
            None
        };
        tx.commit()?;
        Ok(out)
    }

    /// No longer friends, from either side.
    pub fn remove_friend(&self, me: &Account, handle: &str) -> R<()> {
        let db = self.db();
        let Some(other) = db::account_by_handle(&db, handle.trim_start_matches('@'))? else { return fail(404, "nobody has that name on the market") };
        let n = db.execute("DELETE FROM friends WHERE (a = ?1 AND b = ?2) OR (a = ?2 AND b = ?1)", params![me.user_id, other.user_id])?;
        db.execute("DELETE FROM friend_requests WHERE sender = ?1 AND receiver = ?2", params![me.user_id, other.user_id])?;
        db.execute("DELETE FROM world_invites WHERE friend = ?2 AND world IN (SELECT code FROM worlds WHERE owner = ?1)", params![me.user_id, other.user_id])?;
        db.execute("DELETE FROM world_invites WHERE friend = ?1 AND world IN (SELECT code FROM worlds WHERE owner = ?2)", params![me.user_id, other.user_id])?;
        if n == 0 {
            return fail(404, "you weren't friends");
        }
        db::log(&db, &me.user_id, "unfriend", &other.handle, "");
        Ok(())
    }

    /// Ends a friendship and any request, and stops them sending another or
    /// joining this player's worlds.
    pub fn block(&self, me: &Account, handle: &str) -> R<()> {
        let db = self.db();
        let Some(other) = db::account_by_handle(&db, handle.trim_start_matches('@'))? else { return fail(404, "nobody has that name on the market") };
        if other.user_id == me.user_id {
            return fail(409, "that's you");
        }
        let tx = db.unchecked_transaction()?;
        tx.execute("DELETE FROM friends WHERE (a = ?1 AND b = ?2) OR (a = ?2 AND b = ?1)", params![me.user_id, other.user_id])?;
        tx.execute("DELETE FROM friend_requests WHERE (sender = ?1 AND receiver = ?2) OR (sender = ?2 AND receiver = ?1)", params![me.user_id, other.user_id])?;
        tx.execute("DELETE FROM world_invites WHERE (friend = ?2 AND world IN (SELECT code FROM worlds WHERE owner = ?1)) OR (friend = ?1 AND world IN (SELECT code FROM worlds WHERE owner = ?2))", params![me.user_id, other.user_id])?;
        tx.execute("INSERT OR IGNORE INTO blocks (account, blocked, created) VALUES (?1, ?2, ?3)", params![me.user_id, other.user_id, now()])?;
        db::log(&tx, &me.user_id, "block", &other.handle, "");
        tx.commit()?;
        Ok(())
    }

    pub fn unblock(&self, me: &Account, handle: &str) -> R<()> {
        let db = self.db();
        let Some(other) = db::account_by_handle(&db, handle.trim_start_matches('@'))? else { return fail(404, "nobody has that name on the market") };
        db.execute("DELETE FROM blocks WHERE account = ?1 AND blocked = ?2", params![me.user_id, other.user_id])?;
        Ok(())
    }
}

fn befriend(db: &Connection, a: &str, b: &str) -> R<()> {
    for who in [a, b] {
        let n: i64 = db.query_row("SELECT COUNT(*) FROM friends WHERE a = ?1", [who], |r| r.get(0))?;
        if n >= MAX_FRIENDS {
            return fail(409, "one of you has as many friends as the market keeps");
        }
    }
    let t = now();
    db.execute("INSERT OR IGNORE INTO friends (a, b, since) VALUES (?1, ?2, ?3), (?2, ?1, ?3)", params![a, b, t])?;
    db.execute("DELETE FROM friend_requests WHERE (sender = ?1 AND receiver = ?2) OR (sender = ?2 AND receiver = ?1)", params![a, b])?;
    Ok(())
}
