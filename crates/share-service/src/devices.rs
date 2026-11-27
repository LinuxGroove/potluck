//! One account on several devices, with no email or password. Each device
//! signs in to the game server with an anonymous id of its own, and an
//! account starts on the first device the market sees. Another device joins
//! it with a short link code, made on a device that's already in (or on the
//! web) and typed on the new one, or with a recovery code made on the web
//! for when every device is lost. Codes are kept only as hashes.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

use crate::codes;
use crate::db::{self, now, Account};
use crate::market::{fail, App, Fail, R};

/// A link code works this long.
pub const LINK_SECONDS: i64 = 600;
/// Letters in a recovery code, shown in four groups of five.
pub const RECOVERY_LENGTH: usize = 20;
/// Wrong codes one device may try in an hour.
const TRIES: u32 = 10;
/// Devices on one account, counting the first.
const MAX_DEVICES: i64 = 16;
/// How often a linked device's last visit is written down.
const SEEN_EVERY: i64 = 3600;

pub const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS devices (
    id INTEGER PRIMARY KEY,
    -- the game server's id for a device that plays as an account it didn't start
    device TEXT NOT NULL UNIQUE,
    account TEXT NOT NULL REFERENCES accounts(user_id),
    label TEXT NOT NULL DEFAULT '',
    linked INTEGER NOT NULL,
    seen INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS devices_account ON devices(account);
CREATE TABLE IF NOT EXISTS link_codes (
    hash TEXT PRIMARY KEY,
    account TEXT NOT NULL REFERENCES accounts(user_id),
    expires INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS recovery (
    account TEXT PRIMARY KEY REFERENCES accounts(user_id),
    hash TEXT NOT NULL UNIQUE,
    created INTEGER NOT NULL
);
"#;

/// A device linked to an account, as its page lists it.
#[derive(Clone, Debug, Serialize)]
pub struct Device {
    pub id: i64,
    pub label: String,
    pub linked: i64,
    pub seen: i64,
}

/// What linking did: the account this device plays as now.
#[derive(Clone, Debug, Serialize)]
pub struct Linked {
    pub handle: String,
    pub name: String,
    /// A recovery code did it, and is used up.
    pub recovery: bool,
}

/// The account a device plays as: the one it was linked to, or its own.
pub fn account_of(db: &Connection, device: &str) -> rusqlite::Result<String> {
    let row: Option<(String, i64)> = db.query_row("SELECT account, seen FROM devices WHERE device = ?1", [device], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
    match row {
        Some((account, seen)) => {
            if seen < now() - SEEN_EVERY {
                db.execute("UPDATE devices SET seen = ?2 WHERE device = ?1", params![device, now()])?;
            }
            Ok(account)
        }
        None => Ok(device.to_string()),
    }
}

fn tries() -> &'static Mutex<HashMap<String, (u32, i64)>> {
    static TRIED: OnceLock<Mutex<HashMap<String, (u32, i64)>>> = OnceLock::new();
    TRIED.get_or_init(Default::default)
}

fn too_many(device: &str) -> bool {
    let mut t = tries().lock().unwrap();
    let n = now();
    t.retain(|_, (_, first)| *first > n - 3600);
    t.get(device).is_some_and(|(count, _)| *count >= TRIES)
}

fn missed(device: &str) {
    tries().lock().unwrap().entry(device.to_string()).or_insert((0, now())).0 += 1;
}

fn hash(code: &str) -> String {
    codes::sha256_hex(code.as_bytes())
}

impl App {
    /// The device behind a game's session, and the account it plays as.
    pub async fn game_device(&self, token: &str) -> R<(String, String)> {
        let device = self.ids.user_id(token).await.map_err(|m| Fail { status: 401, message: m })?;
        let account = account_of(&self.db(), &device)?;
        Ok((device, account))
    }

    /// A code another device types to play as this account, for ten
    /// minutes. It works once, and making one ends any earlier one.
    pub fn link_code(&self, me: &Account) -> R<String> {
        let code = codes::new_code();
        let db = self.db();
        db.execute("DELETE FROM link_codes WHERE expires < ?1 OR account = ?2", params![now(), me.user_id])?;
        db.execute("INSERT INTO link_codes (hash, account, expires) VALUES (?1, ?2, ?3)", params![hash(&code), me.user_id, now() + LINK_SECONDS])?;
        Ok(code)
    }

    /// A new recovery code, shown once: it replaces any earlier one.
    pub fn recovery_code(&self, me: &Account) -> R<String> {
        let code = codes::letters(RECOVERY_LENGTH);
        let db = self.db();
        db.execute(
            "INSERT INTO recovery (account, hash, created) VALUES (?1, ?2, ?3) ON CONFLICT(account) DO UPDATE SET hash = excluded.hash, created = excluded.created",
            params![me.user_id, hash(&code), now()],
        )?;
        db::log(&db, &me.user_id, "recovery_code", &me.handle, "");
        Ok(code)
    }

    /// When this account's recovery code was made, if it has one.
    pub fn recovery_made(&self, me: &Account) -> R<Option<i64>> {
        Ok(self.db().query_row("SELECT created FROM recovery WHERE account = ?1", [&me.user_id], |r| r.get(0)).optional()?)
    }

    /// The devices linked to this account (the first isn't in the list).
    pub fn devices(&self, me: &Account) -> R<Vec<Device>> {
        let db = self.db();
        let mut st = db.prepare("SELECT id, label, linked, seen FROM devices WHERE account = ?1 ORDER BY linked")?;
        let list = st.query_map([&me.user_id], |r| Ok(Device { id: r.get(0)?, label: r.get(1)?, linked: r.get(2)?, seen: r.get(3)? }))?.collect::<Result<Vec<_>, _>>()?;
        Ok(list)
    }

    /// The device behind a game's session plays as the account a code
    /// belongs to from now on: a link code, or a recovery code, which is
    /// then used up.
    pub async fn link(&self, token: &str, code: &str, label: &str) -> R<Linked> {
        let device = self.ids.user_id(token).await.map_err(|m| Fail { status: 401, message: m })?;
        self.link_device(&device, code, label)
    }

    pub fn link_device(&self, device: &str, code: &str, label: &str) -> R<Linked> {
        if too_many(device) {
            return fail(429, "that's a lot of wrong codes: wait an hour and try again");
        }
        let code = codes::clean(code);
        let label: String = label.trim().chars().filter(|c| !c.is_control()).take(40).collect();
        let db = self.db();
        let tx = db.unchecked_transaction()?;
        let (account, recovery): (Option<String>, bool) = match code.len() {
            codes::LENGTH => (tx.query_row("SELECT account FROM link_codes WHERE hash = ?1 AND expires >= ?2", params![hash(&code), now()], |r| r.get(0)).optional()?, false),
            RECOVERY_LENGTH => (tx.query_row("SELECT account FROM recovery WHERE hash = ?1", [hash(&code)], |r| r.get(0)).optional()?, true),
            _ => (None, false),
        };
        let Some(account) = account else {
            missed(device);
            return fail(404, "that code isn't one, or it has run out: make a new one on your other device");
        };
        let current = account_of(&tx, device)?;
        if current == account {
            return fail(409, "this device already plays as that account");
        }
        let count: i64 = tx.query_row("SELECT COUNT(*) FROM devices WHERE account = ?1", [&account], |r| r.get(0))?;
        if count + 1 >= MAX_DEVICES {
            return fail(409, "that account has all the devices it can: unlink one on the market first");
        }
        // A device that started an account of its own leaves it behind, so
        // only an account with nothing to lose can go.
        if current == device {
            if let Some(own) = db::account(&tx, device)? {
                let keeps: i64 = tx.query_row(
                    "SELECT (SELECT COUNT(*) FROM creations WHERE owner = ?1) + (SELECT COUNT(*) FROM servers WHERE owner = ?1) + (SELECT COUNT(*) FROM devices WHERE account = ?1)",
                    [device],
                    |r| r.get(0),
                )?;
                if keeps > 0 || own.moderator {
                    return fail(409, "this device has its own creations, servers or devices on the market, and they'd be left behind. Link the other way: make a code on this device and type it on the other one");
                }
                // its stars and the beta come along; then the empty account goes
                tx.execute("UPDATE creations SET stars = stars - 1 WHERE id IN (SELECT creation FROM stars WHERE user_id = ?1) AND id IN (SELECT creation FROM stars WHERE user_id = ?2)", params![device, account])?;
                tx.execute("INSERT OR IGNORE INTO stars (user_id, creation, created) SELECT ?2, creation, created FROM stars WHERE user_id = ?1", params![device, account])?;
                tx.execute("DELETE FROM stars WHERE user_id = ?1", [device])?;
                if own.beta {
                    tx.execute("UPDATE accounts SET beta = 1 WHERE user_id = ?1", [&account])?;
                }
                for table in ["web_sessions", "tickets"] {
                    tx.execute(&format!("DELETE FROM {table} WHERE user_id = ?1"), [device])?;
                }
                for table in ["link_codes", "recovery"] {
                    tx.execute(&format!("DELETE FROM {table} WHERE account = ?1"), [device])?;
                }
                tx.execute("DELETE FROM accounts WHERE user_id = ?1", [device])?;
            }
        }
        tx.execute(
            "INSERT INTO devices (device, account, label, linked, seen) VALUES (?1, ?2, ?3, ?4, ?4) ON CONFLICT(device) DO UPDATE SET account = excluded.account, label = excluded.label, linked = excluded.linked, seen = excluded.seen",
            params![device, account, label, now()],
        )?;
        if recovery {
            tx.execute("DELETE FROM recovery WHERE account = ?1", [&account])?;
        } else {
            tx.execute("DELETE FROM link_codes WHERE hash = ?1", [hash(&code)])?;
        }
        let a = db::account(&tx, &account)?.ok_or(Fail { status: 404, message: "that account has gone".into() })?;
        db::log(&tx, &account, if recovery { "recovered" } else { "linked" }, &a.handle, &label);
        tx.commit()?;
        Ok(Linked { handle: a.handle, name: a.name, recovery })
    }

    /// Takes a linked device off this account, from its page on the web.
    pub fn unlink(&self, me: &Account, id: i64) -> R<()> {
        let db = self.db();
        if db.execute("DELETE FROM devices WHERE id = ?1 AND account = ?2", params![id, me.user_id])? == 0 {
            return fail(404, "that device isn't linked to you");
        }
        db::log(&db, &me.user_id, "unlinked", &me.handle, &id.to_string());
        Ok(())
    }

    /// The device behind a game's session stops playing as the account it
    /// was linked to, and starts one of its own next time.
    pub async fn leave(&self, token: &str) -> R<()> {
        let (device, account) = self.game_device(token).await?;
        if device == account {
            return fail(409, "this device isn't linked to another one");
        }
        let db = self.db();
        db.execute("DELETE FROM devices WHERE device = ?1", [&device])?;
        db::log(&db, &account, "left", &device, "");
        Ok(())
    }
}
