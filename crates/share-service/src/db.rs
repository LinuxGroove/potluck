//! The market's own index in SQLite: accounts, creations and their versions,
//! stars, plays, reports, the review queue and suggested changes. Git history
//! lives in Forgejo and files in the store; this is what's searched and shown.

use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;

pub fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

const SCHEMA: &str = r#"
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;
CREATE TABLE IF NOT EXISTS accounts (
    user_id TEXT PRIMARY KEY,
    handle TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    created INTEGER NOT NULL,
    beta INTEGER NOT NULL DEFAULT 0,
    moderator INTEGER NOT NULL DEFAULT 0,
    trusted INTEGER NOT NULL DEFAULT 0,
    banned INTEGER NOT NULL DEFAULT 0,
    -- the profile, edited on the web: a line about them and their block head
    about TEXT NOT NULL DEFAULT '',
    avatar TEXT NOT NULL DEFAULT '',
    -- once a name is set on the web, the game's name no longer changes it
    named_on_web INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS web_sessions (
    token TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES accounts(user_id),
    expires INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS tickets (
    ticket TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    expires INTEGER NOT NULL,
    -- the market page the link opens once it has signed the browser in
    next TEXT NOT NULL DEFAULT '/me'
);
CREATE TABLE IF NOT EXISTS invites (
    code TEXT PRIMARY KEY,
    made_by TEXT NOT NULL,
    created INTEGER NOT NULL,
    used_by TEXT,
    used_at INTEGER
);
CREATE TABLE IF NOT EXISTS creations (
    id INTEGER PRIMARY KEY,
    code TEXT NOT NULL UNIQUE,
    owner TEXT NOT NULL REFERENCES accounts(user_id),
    slug TEXT NOT NULL,
    name TEXT NOT NULL,
    kind TEXT NOT NULL,
    about TEXT NOT NULL DEFAULT '',
    licence TEXT NOT NULL DEFAULT '',
    -- what the maker asked for: 'everyone' or 'code'
    visibility TEXT NOT NULL,
    -- 'unlisted' (works by code), 'waiting' (for a person), 'listed', 'hidden', 'removed'
    listing TEXT NOT NULL,
    -- where a hidden creation goes back to when a moderator clears it
    hidden_from TEXT,
    parent INTEGER REFERENCES creations(id),
    parent_version INTEGER,
    base TEXT UNIQUE,
    latest INTEGER NOT NULL DEFAULT 0,
    stars INTEGER NOT NULL DEFAULT 0,
    plays INTEGER NOT NULL DEFAULT 0,
    remixes INTEGER NOT NULL DEFAULT 0,
    created INTEGER NOT NULL,
    updated INTEGER NOT NULL,
    UNIQUE (owner, slug)
);
CREATE TABLE IF NOT EXISTS versions (
    creation INTEGER NOT NULL REFERENCES creations(id),
    n INTEGER NOT NULL,
    git_commit TEXT NOT NULL,
    manifest TEXT NOT NULL,
    signature TEXT NOT NULL,
    size INTEGER NOT NULL,
    checks TEXT NOT NULL,
    -- 'ok', 'waiting' (a person looks before it's listed) or 'held' (failed a check)
    state TEXT NOT NULL,
    note TEXT NOT NULL DEFAULT '',
    created INTEGER NOT NULL,
    PRIMARY KEY (creation, n)
);
CREATE TABLE IF NOT EXISTS files (
    creation INTEGER NOT NULL,
    n INTEGER NOT NULL,
    path TEXT NOT NULL,
    sha256 TEXT NOT NULL,
    size INTEGER NOT NULL,
    PRIMARY KEY (creation, n, path)
);
CREATE INDEX IF NOT EXISTS files_by_hash ON files(sha256);
CREATE TABLE IF NOT EXISTS stars (
    user_id TEXT NOT NULL,
    creation INTEGER NOT NULL,
    created INTEGER NOT NULL,
    PRIMARY KEY (user_id, creation)
);
CREATE TABLE IF NOT EXISTS plays (
    creation INTEGER NOT NULL,
    who TEXT NOT NULL,
    day INTEGER NOT NULL,
    PRIMARY KEY (creation, who, day)
);
CREATE TABLE IF NOT EXISTS reports (
    id INTEGER PRIMARY KEY,
    creation INTEGER NOT NULL,
    version INTEGER NOT NULL,
    reporter TEXT NOT NULL,
    reason TEXT NOT NULL,
    note TEXT NOT NULL DEFAULT '',
    created INTEGER NOT NULL,
    open INTEGER NOT NULL DEFAULT 1,
    UNIQUE (creation, reporter)
);
CREATE TABLE IF NOT EXISTS queue (
    id INTEGER PRIMARY KEY,
    creation INTEGER NOT NULL,
    version INTEGER NOT NULL,
    -- 'new_maker', 'update', 'reports' or 'checks'
    why TEXT NOT NULL,
    detail TEXT NOT NULL DEFAULT '',
    created INTEGER NOT NULL,
    open INTEGER NOT NULL DEFAULT 1,
    decided_by TEXT,
    decision TEXT,
    decided_at INTEGER
);
CREATE TABLE IF NOT EXISTS suggestions (
    id INTEGER PRIMARY KEY,
    creation INTEGER NOT NULL,
    from_creation INTEGER NOT NULL,
    from_version INTEGER NOT NULL,
    pull INTEGER NOT NULL,
    by_user TEXT NOT NULL,
    title TEXT NOT NULL,
    body TEXT NOT NULL DEFAULT '',
    -- 'open', 'merged' or 'closed'
    state TEXT NOT NULL,
    merged_version INTEGER,
    created INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS banned_files (
    sha256 TEXT PRIMARY KEY,
    created INTEGER NOT NULL,
    note TEXT NOT NULL DEFAULT ''
);
CREATE TABLE IF NOT EXISTS log (
    id INTEGER PRIMARY KEY,
    at INTEGER NOT NULL,
    actor TEXT NOT NULL,
    action TEXT NOT NULL,
    target TEXT NOT NULL,
    note TEXT NOT NULL DEFAULT ''
);
CREATE VIRTUAL TABLE IF NOT EXISTS search USING fts5(code UNINDEXED, name, about, maker, text, tokenize = 'unicode61');
"#;

pub fn open(path: &Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    conn.execute_batch(SCHEMA)?;
    Ok(conn)
}

#[derive(Clone, Debug, Serialize)]
pub struct Account {
    pub user_id: String,
    pub handle: String,
    pub name: String,
    pub created: i64,
    pub beta: bool,
    pub moderator: bool,
    pub trusted: bool,
    pub banned: bool,
    pub about: String,
    pub avatar: String,
    pub named_on_web: bool,
}

fn account_row(r: &Row) -> rusqlite::Result<Account> {
    Ok(Account {
        user_id: r.get(0)?,
        handle: r.get(1)?,
        name: r.get(2)?,
        created: r.get(3)?,
        beta: r.get(4)?,
        moderator: r.get(5)?,
        trusted: r.get(6)?,
        banned: r.get(7)?,
        about: r.get(8)?,
        avatar: r.get(9)?,
        named_on_web: r.get(10)?,
    })
}

const ACCOUNT: &str = "SELECT user_id, handle, name, created, beta, moderator, trusted, banned, about, avatar, named_on_web FROM accounts";

pub fn account(db: &Connection, user_id: &str) -> rusqlite::Result<Option<Account>> {
    db.query_row(&format!("{ACCOUNT} WHERE user_id = ?1"), [user_id], account_row).optional()
}

pub fn account_by_handle(db: &Connection, handle: &str) -> rusqlite::Result<Option<Account>> {
    db.query_row(&format!("{ACCOUNT} WHERE handle = ?1 COLLATE NOCASE"), [handle], account_row).optional()
}

/// Names Forgejo or the market keep for themselves.
const RESERVED: &[&str] = &[
    "actions", "admin", "api", "assets", "avatar", "avatars", "forgejo", "ghost", "gitea", "profile", "root", "c", "explore", "git", "issues", "login", "market", "moderator", "new", "notifications", "org", "potluck", "pulls", "repo", "review", "search", "signin", "signout", "static", "user", "users", "f", "me", "help", "about",
];

/// A handle for a new account from the name a player chose in game: letters,
/// digits, dashes and underscores, unique, never one of the reserved names.
pub fn free_handle(db: &Connection, name: &str) -> rusqlite::Result<String> {
    let mut base: String = name.chars().filter_map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { Some(c.to_ascii_lowercase()) } else if c == ' ' || c == '.' { Some('-') } else { None }).collect();
    base = base.trim_matches(|c| c == '-' || c == '_').chars().take(24).collect();
    if base.len() < 2 {
        base = format!("maker{base}");
    }
    let mut handle = base.clone();
    let mut n = 2;
    while RESERVED.contains(&handle.as_str()) || account_by_handle(db, &handle)?.is_some() {
        handle = format!("{base}{n}");
        n += 1;
    }
    Ok(handle)
}

pub fn ensure_account(db: &Connection, user_id: &str, name: &str) -> rusqlite::Result<Account> {
    if let Some(mut a) = account(db, user_id)? {
        let clean: String = name.trim().chars().take(32).collect();
        if !clean.is_empty() && clean != a.name && !a.named_on_web {
            db.execute("UPDATE accounts SET name = ?2 WHERE user_id = ?1", params![user_id, clean])?;
            a.name = clean;
        }
        return Ok(a);
    }
    let name: String = name.trim().chars().take(32).collect();
    let name = if name.is_empty() { "builder".to_string() } else { name };
    let handle = free_handle(db, &name)?;
    db.execute("INSERT INTO accounts (user_id, handle, name, created) VALUES (?1, ?2, ?3, ?4)", params![user_id, handle, name, now()])?;
    Ok(account(db, user_id)?.expect("account just made"))
}

#[derive(Clone, Debug, Serialize)]
pub struct Creation {
    pub id: i64,
    pub code: String,
    pub owner: String,
    pub handle: String,
    pub maker: String,
    pub slug: String,
    pub name: String,
    pub kind: String,
    pub about: String,
    pub licence: String,
    pub visibility: String,
    pub listing: String,
    pub parent: Option<i64>,
    pub parent_version: Option<i64>,
    pub base: Option<String>,
    pub latest: i64,
    pub stars: i64,
    pub plays: i64,
    pub remixes: i64,
    pub created: i64,
    pub updated: i64,
}

impl Creation {
    pub fn path(&self) -> String {
        format!("{}/{}", self.handle, self.slug)
    }
    pub fn visible_to_all(&self) -> bool {
        matches!(self.listing.as_str(), "listed" | "unlisted" | "waiting")
    }
}

pub const CREATION: &str = "SELECT c.id, c.code, c.owner, a.handle, a.name, c.slug, c.name, c.kind, c.about, c.licence, c.visibility, c.listing, c.parent, c.parent_version, c.base, c.latest, c.stars, c.plays, c.remixes, c.created, c.updated FROM creations c JOIN accounts a ON a.user_id = c.owner";

pub fn creation_row(r: &Row) -> rusqlite::Result<Creation> {
    Ok(Creation {
        id: r.get(0)?,
        code: r.get(1)?,
        owner: r.get(2)?,
        handle: r.get(3)?,
        maker: r.get(4)?,
        slug: r.get(5)?,
        name: r.get(6)?,
        kind: r.get(7)?,
        about: r.get(8)?,
        licence: r.get(9)?,
        visibility: r.get(10)?,
        listing: r.get(11)?,
        parent: r.get(12)?,
        parent_version: r.get(13)?,
        base: r.get(14)?,
        latest: r.get(15)?,
        stars: r.get(16)?,
        plays: r.get(17)?,
        remixes: r.get(18)?,
        created: r.get(19)?,
        updated: r.get(20)?,
    })
}

pub fn creation(db: &Connection, id: i64) -> rusqlite::Result<Option<Creation>> {
    db.query_row(&format!("{CREATION} WHERE c.id = ?1"), [id], creation_row).optional()
}

pub fn creation_by_code(db: &Connection, code: &str) -> rusqlite::Result<Option<Creation>> {
    db.query_row(&format!("{CREATION} WHERE c.code = ?1"), [code], creation_row).optional()
}

pub fn creation_by_base(db: &Connection, base: &str) -> rusqlite::Result<Option<Creation>> {
    db.query_row(&format!("{CREATION} WHERE c.base = ?1"), [base], creation_row).optional()
}

pub fn creations_where(db: &Connection, clause: &str, args: &[&dyn rusqlite::ToSql]) -> rusqlite::Result<Vec<Creation>> {
    let mut st = db.prepare(&format!("{CREATION} {clause}"))?;
    let rows = st.query_map(args, creation_row)?;
    rows.collect()
}

#[derive(Clone, Debug, Serialize)]
pub struct Version {
    pub creation: i64,
    pub n: i64,
    pub git_commit: String,
    pub manifest: String,
    pub signature: String,
    pub size: i64,
    pub checks: String,
    pub state: String,
    pub note: String,
    pub created: i64,
}

fn version_row(r: &Row) -> rusqlite::Result<Version> {
    Ok(Version {
        creation: r.get(0)?,
        n: r.get(1)?,
        git_commit: r.get(2)?,
        manifest: r.get(3)?,
        signature: r.get(4)?,
        size: r.get(5)?,
        checks: r.get(6)?,
        state: r.get(7)?,
        note: r.get(8)?,
        created: r.get(9)?,
    })
}

const VERSION: &str = "SELECT creation, n, git_commit, manifest, signature, size, checks, state, note, created FROM versions";

pub fn version(db: &Connection, creation: i64, n: i64) -> rusqlite::Result<Option<Version>> {
    db.query_row(&format!("{VERSION} WHERE creation = ?1 AND n = ?2"), params![creation, n], version_row).optional()
}

pub fn versions(db: &Connection, creation: i64) -> rusqlite::Result<Vec<Version>> {
    let mut st = db.prepare(&format!("{VERSION} WHERE creation = ?1 ORDER BY n DESC"))?;
    let rows = st.query_map([creation], version_row)?;
    rows.collect()
}

/// The newest version others may load: anything that passed its checks.
pub fn loadable(db: &Connection, creation: i64) -> rusqlite::Result<Option<Version>> {
    db.query_row(&format!("{VERSION} WHERE creation = ?1 AND state != 'held' ORDER BY n DESC LIMIT 1"), [creation], version_row).optional()
}

pub fn files(db: &Connection, creation: i64, n: i64) -> rusqlite::Result<Vec<(String, String, i64)>> {
    let mut st = db.prepare("SELECT path, sha256, size FROM files WHERE creation = ?1 AND n = ?2 ORDER BY path")?;
    let rows = st.query_map(params![creation, n], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
    rows.collect()
}

pub fn log(db: &Connection, actor: &str, action: &str, target: &str, note: &str) {
    let _ = db.execute("INSERT INTO log (at, actor, action, target, note) VALUES (?1, ?2, ?3, ?4, ?5)", params![now(), actor, action, target, note]);
}

/// Puts a creation in front of moderators, once per open reason.
pub fn enqueue(db: &Connection, creation: i64, version: i64, why: &str, detail: &str) -> rusqlite::Result<()> {
    let open: Option<i64> = db
        .query_row("SELECT id FROM queue WHERE creation = ?1 AND why = ?2 AND open = 1", params![creation, why], |r| r.get(0))
        .optional()?;
    match open {
        Some(id) => {
            db.execute("UPDATE queue SET version = ?2, detail = ?3 WHERE id = ?1", params![id, version, detail])?;
        }
        None => {
            db.execute("INSERT INTO queue (creation, version, why, detail, created) VALUES (?1, ?2, ?3, ?4, ?5)", params![creation, version, why, detail, now()])?;
        }
    }
    Ok(())
}

pub fn is_banned_file(db: &Connection, sha256: &str) -> bool {
    db.query_row("SELECT 1 FROM banned_files WHERE sha256 = ?1", [sha256], |_| Ok(())).optional().ok().flatten().is_some()
}

/// Keeps the search index's row for a creation in step.
pub fn index_search(db: &Connection, c: &Creation, text: &str) -> rusqlite::Result<()> {
    db.execute("DELETE FROM search WHERE code = ?1", [&c.code])?;
    if c.listing == "listed" {
        db.execute("INSERT INTO search (code, name, about, maker, text) VALUES (?1, ?2, ?3, ?4, ?5)", params![c.code, c.name, c.about, format!("{} {}", c.maker, c.handle), text])?;
    }
    Ok(())
}

/// Turns what someone typed into an FTS5 query: each word as a prefix.
pub fn search_query(q: &str) -> String {
    q.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .take(8)
        .map(|w| format!("\"{}\"*", w.to_lowercase()))
        .collect::<Vec<_>>()
        .join(" ")
}
