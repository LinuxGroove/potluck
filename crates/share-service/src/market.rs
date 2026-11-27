//! What the market does, for the game's API and the web pages alike:
//! sharing a version, remixing, loading by code, suggesting and merging
//! changes, stars, reports and moderators' decisions.

use std::collections::BTreeMap;
use std::sync::Mutex;

use ed25519_dalek::SigningKey;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::avatar::Avatar;
use crate::checks::{self, Check, Toml, Words};
use crate::codes;
use crate::config::Config;
use crate::db::{self, now, Account, Creation};
use crate::forgejo::Forgejo;
use crate::identity::Identity;
use crate::store::Store;

pub struct App {
    pub cfg: Config,
    db: Mutex<Connection>,
    pub git: Forgejo,
    pub store: Store,
    pub ids: Identity,
    pub key: SigningKey,
    pub words: Words,
    /// One change to git at a time: shares, remixes and merges are rare next
    /// to reads, and this keeps versions numbered in order.
    pub writing: tokio::sync::Mutex<()>,
}

#[derive(Debug)]
pub struct Fail {
    pub status: u16,
    pub message: String,
}

pub type R<T> = Result<T, Fail>;

pub fn fail<T>(status: u16, message: impl Into<String>) -> R<T> {
    Err(Fail { status, message: message.into() })
}

impl From<rusqlite::Error> for Fail {
    fn from(e: rusqlite::Error) -> Fail {
        eprintln!("database: {e}");
        Fail { status: 500, message: "the market's index had a problem".into() }
    }
}

impl From<crate::forgejo::Error> for Fail {
    fn from(e: crate::forgejo::Error) -> Fail {
        eprintln!("{e}");
        Fail { status: if e.status == 405 { 409 } else { 502 }, message: if e.status == 405 { e.message } else { "the market couldn't save to git just now; try again".into() } }
    }
}

/// Where a remix came from: a creation on the market by code, or one of the
/// base game's by its folder name, at a version.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Parent {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
    #[serde(default)]
    pub version: i64,
}

/// A version's signed list of files, which games download by hash.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Manifest {
    pub format: u32,
    pub code: String,
    pub path: String,
    pub version: i64,
    pub commit: String,
    pub name: String,
    pub by: String,
    pub kind: String,
    pub licence: String,
    pub needs: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<Parent>,
    pub files: BTreeMap<String, FileRef>,
    pub created: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct FileRef {
    pub sha256: String,
    pub size: usize,
}

#[derive(Serialize, Clone, Debug)]
pub struct Shared {
    pub code: String,
    pub version: i64,
    pub url: String,
    /// unlisted, waiting, listed, hidden or removed
    pub listing: String,
    /// ok, waiting or held
    pub state: String,
    pub checks: Vec<Check>,
    pub message: String,
}

pub fn slug(s: &str) -> String {
    let mut out: String = s.to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    while out.contains("--") {
        out = out.replace("--", "-");
    }
    let out: String = out.trim_matches('-').chars().take(40).collect();
    let out = out.trim_end_matches('-').to_string();
    if out.is_empty() { "creation".into() } else { out }
}

/// The base game's creations, the roots of every family tree that starts
/// from something Potluck made. Keep in step with `BASE` in the game's
/// creations.rs.
macro_rules! base {
    ($($id:literal),* $(,)?) => {
        &[$(($id, &[
            ("potluck.toml", include_str!(concat!("../../../creations/", $id, "/potluck.toml"))),
            ("main.luau", include_str!(concat!("../../../creations/", $id, "/main.luau"))),
            ("AGENTS.md", include_str!("../../../creations/AGENTS.md")),
            ("types/potluck.d.luau", include_str!("../../../creations/types/potluck.d.luau")),
        ])),*]
    };
}

pub const BASE: &[(&str, &[(&str, &str)])] = base!["mossback", "woolly", "pebble-crab", "glowbug", "gnome", "duck", "frog", "mole", "pup", "rodeo-ram", "jelly", "tag-bot"];

pub const POTLUCK: &str = "potluck";

impl App {
    pub fn new(cfg: Config, conn: Connection, git: Forgejo, store: Store, ids: Identity, key: SigningKey, words: Words) -> App {
        App { cfg, db: Mutex::new(conn), git, store, ids, key, words, writing: tokio::sync::Mutex::new(()) }
    }

    /// The index, for a moment: never held across an await.
    /// Everything the market needs from its config: the index, git hosting,
    /// the store, sign-in and the signing key in its data folder.
    pub fn open(cfg: Config, words: Words) -> Result<App, String> {
        let http = reqwest::Client::builder().user_agent(concat!("potluck-share/", env!("CARGO_PKG_VERSION"))).timeout(std::time::Duration::from_secs(30)).build().map_err(|e| e.to_string())?;
        std::fs::create_dir_all(&cfg.data).map_err(|e| format!("couldn't make {}: {e}", cfg.data.display()))?;
        let conn = db::open(&cfg.data.join("market.db")).map_err(|e| format!("the index: {e}"))?;
        let store = Store::new(&cfg, http.clone())?;
        let key = codes::signing_key(&cfg.data).map_err(|e| format!("the signing key: {e}"))?;
        let git = Forgejo::new(&cfg.forgejo, http.clone());
        let ids = Identity::new(&cfg.nakama, cfg.dev_accounts, http);
        Ok(App::new(cfg, conn, git, store, ids, key, words))
    }

    pub fn db(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.db.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn url(&self, c: &Creation) -> String {
        format!("{}/c/{}", self.cfg.public_url, c.code)
    }

    fn new_code(&self) -> R<String> {
        let db = self.db();
        for _ in 0..20 {
            let code = codes::new_code();
            let taken: Option<i64> = db.query_row("SELECT id FROM creations WHERE code = ?1", [&code], |r| r.get(0)).optional()?;
            if taken.is_none() {
                return Ok(code);
            }
        }
        fail(503, "couldn't find a free code; try again")
    }

    /// The account behind a game's session, made the first time it's seen.
    pub async fn game_account(&self, token: &str, name: &str) -> R<Account> {
        let id = self.ids.user_id(token).await.map_err(|m| Fail { status: 401, message: m })?;
        let name = if name.trim().is_empty() && self.cfg.dev_accounts { token.split(':').nth(2).unwrap_or("").to_string() } else { name.to_string() };
        let a = db::ensure_account(&self.db(), &id, &name)?;
        if a.banned {
            return fail(403, "this account can't share on the market");
        }
        Ok(a)
    }

    fn resolve_from(&self, from: &Parent) -> R<Option<(Creation, i64)>> {
        let db = self.db();
        let c = match (&from.code, &from.base) {
            (Some(code), _) => db::creation_by_code(&db, &codes::clean(code))?,
            (None, Some(base)) => db::creation_by_base(&db, base)?,
            _ => return Ok(None),
        };
        let Some(c) = c else { return Ok(None) };
        if !c.visible_to_all() {
            return Ok(None);
        }
        let v = if from.version > 0 && db::version(&db, c.id, from.version)?.is_some() { from.version } else { c.latest };
        Ok(Some((c, v)))
    }

    /// Makes a creation's repository and its row: empty, or with the history
    /// of the version it's a remix of.
    async fn new_creation(&self, me: &Account, toml: &Toml, visibility: &str, parent: Option<(Creation, i64)>, base: Option<&str>) -> R<Creation> {
        let base_slug = slug(&toml.name);
        self.git.ensure_user(&me.handle, &me.name).await?;
        let mut n = 1;
        let slug = loop {
            let s = if n == 1 { base_slug.clone() } else { format!("{base_slug}-{n}") };
            n += 1;
            let taken = self.db().query_row("SELECT 1 FROM creations WHERE owner = ?1 AND slug = ?2", params![me.user_id, s], |_| Ok(())).optional()?.is_some();
            if taken {
                continue;
            }
            match self.git.create_repo(&me.handle, &s, &toml.about).await {
                Ok(()) => break s,
                // left in git hosting by something the index has forgotten
                Err(e) if e.status == 409 && n < 50 => continue,
                Err(e) => return Err(e.into()),
            }
        };
        let repo = format!("{}/{slug}", me.handle);
        if let Some((p, v)) = &parent {
            let scratch = self.cfg.data.join("tmp");
            let _ = std::fs::create_dir_all(&scratch);
            self.git.copy_commit(&p.path(), &format!("refs/tags/v{v}"), &repo, "main", &scratch).await?;
        }
        let code = self.new_code()?;
        let db = self.db();
        let t = now();
        db.execute(
            "INSERT INTO creations (code, owner, slug, name, kind, about, licence, visibility, listing, parent, parent_version, base, created, updated) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'unlisted', ?9, ?10, ?11, ?12, ?12)",
            params![code, me.user_id, slug, toml.name, toml.kind, toml.about, toml.licence, visibility, parent.as_ref().map(|p| p.0.id), parent.as_ref().map(|p| p.1), base, t],
        )?;
        if let Some((p, _)) = &parent {
            db.execute("UPDATE creations SET remixes = remixes + 1 WHERE id = ?1", [p.id])?;
            db::log(&db, &me.user_id, "remix", &p.code, &code);
        }
        Ok(db::creation_by_code(&db, &code)?.expect("just made"))
    }

    /// Saves files as the next version of a creation: checks, a commit (or
    /// the one a merge just made), a tag, the files in the store, a signed
    /// manifest, and what happens to its listing.
    async fn publish(&self, me: &Account, c: &Creation, files: BTreeMap<String, String>, note: &str, merged: Option<String>) -> R<Shared> {
        let banned = |h: &str| db::is_banned_file(&self.db(), h);
        let (toml, results) = checks::run(&files, &self.words, &banned);
        let toml = toml.unwrap_or_default();
        let n = c.latest + 1;
        let repo = c.path();
        let message = if note.trim().is_empty() { format!("Version {n}") } else { note.trim().chars().take(200).collect() };
        let commit = match merged {
            Some(sha) => sha,
            None => match self.git.commit(&repo, &me.handle, &me.name, &message, &files).await? {
                Some(sha) => sha,
                // A remix shared with nothing changed yet: its history is its parent's.
                None if c.latest == 0 => self.git.head(&repo).await?,
                None => return fail(409, format!("nothing has changed since version {}", c.latest)),
            },
        };
        self.git.tag(&repo, &me.handle, &format!("v{n}"), &commit, &message).await?;
        let mut refs = BTreeMap::new();
        for (path, text) in &files {
            let sha = codes::sha256_hex(text.as_bytes());
            self.store.put(&sha, text.as_bytes()).await.map_err(|e| Fail { status: 502, message: format!("couldn't store the files ({e})") })?;
            refs.insert(path.clone(), FileRef { sha256: sha, size: text.len() });
        }
        let parent_code = c.parent.and_then(|p| db::creation(&self.db(), p).ok().flatten()).map(|p| p.code);
        let manifest = Manifest {
            format: 2,
            code: c.code.clone(),
            path: repo.clone(),
            version: n,
            commit: commit.clone(),
            name: toml.name.clone(),
            by: me.name.clone(),
            kind: toml.kind.clone(),
            licence: toml.licence.clone(),
            needs: toml.needs.clone(),
            from: parent_code.map(|code| Parent { code: Some(code), base: None, version: c.parent_version.unwrap_or(0) }),
            files: refs,
            created: now(),
        };
        let text = serde_json::to_string(&manifest).expect("a manifest");
        let sig = codes::sign(&self.key, &text);
        let size: usize = files.values().map(|v| v.len()).sum();

        let held = checks::failed(&results);
        let wants_everyone = c.visibility == "everyone";
        let state = if held {
            "held"
        } else if wants_everyone && !me.trusted && c.listing != "hidden" && c.listing != "removed" {
            "waiting"
        } else {
            "ok"
        };
        let mut listing = c.listing.clone();
        let msg;
        if listing == "hidden" || listing == "removed" {
            msg = "saved, but it's hidden while a moderator looks at it".to_string();
        } else if held {
            msg = format!("version {n} is saved for you, but nobody else can load it yet: {}", results.iter().filter(|c| c.result == "fail").map(|c| format!("{} ({})", c.name.to_lowercase(), c.note)).collect::<Vec<_>>().join("; "));
        } else if !wants_everyone {
            listing = "unlisted".into();
            msg = "anyone with the code can load it".into();
        } else if me.trusted {
            listing = "listed".into();
            msg = "it's on the market for everyone".into();
        } else {
            if listing != "listed" {
                listing = "waiting".into();
            }
            msg = if listing == "listed" { "the new version is on the market; a moderator will look at what changed".into() } else { "a moderator will look at it before it's listed; anyone with the code can load it now".to_string() };
        }

        let search_text: String = files.iter().filter(|(p, _)| (p.ends_with(".luau") || p.ends_with(".md")) && !p.starts_with("types/") && *p != "AGENTS.md").map(|(_, t)| t.as_str()).collect::<Vec<_>>().join("\n");
        let db = self.db();
        let t = now();
        db.execute(
            "INSERT INTO versions (creation, n, git_commit, manifest, signature, size, checks, state, note, created) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![c.id, n, commit, text, sig, size as i64, serde_json::to_string(&results).unwrap_or_default(), state, message, t],
        )?;
        for (path, f) in &manifest.files {
            db.execute("INSERT INTO files (creation, n, path, sha256, size) VALUES (?1, ?2, ?3, ?4, ?5)", params![c.id, n, path, f.sha256, f.size as i64])?;
        }
        db.execute(
            "UPDATE creations SET latest = ?2, name = ?3, kind = ?4, about = ?5, licence = ?6, listing = ?7, updated = ?8 WHERE id = ?1",
            params![c.id, n, if toml.name.is_empty() { c.name.clone() } else { toml.name.clone() }, toml.kind, toml.about, toml.licence, listing, t],
        )?;
        if state == "waiting" {
            let why = if c.listing == "listed" { "update" } else { "new_maker" };
            db::enqueue(&db, c.id, n, why, "")?;
        }
        // The game's own creations aren't a stranger's code: no person needs to look.
        if !held && c.base.is_none() && checks::warned(&results) {
            let notes: Vec<String> = results.iter().filter(|c| c.result == "warn").map(|c| format!("{}: {}", c.name, c.note)).collect();
            db::enqueue(&db, c.id, n, "checks", &notes.join("; "))?;
        }
        let updated = db::creation(&db, c.id)?.expect("still there");
        db::index_search(&db, &updated, &search_text)?;
        db::log(&db, &me.user_id, "share", &c.code, &format!("version {n}, {state}"));
        Ok(Shared { code: c.code.clone(), version: n, url: self.url(c), listing, state: state.into(), checks: results, message: msg })
    }

    /// Share from the game: a new creation (or a remix), or the next version
    /// of one of the player's own.
    pub async fn share(&self, me: &Account, req: ShareReq) -> R<Shared> {
        let _w = self.writing.lock().await;
        let mut visibility = match req.visibility.as_str() {
            "everyone" => "everyone",
            _ => "code",
        };
        let not_yet = visibility == "everyone" && self.cfg.invite_only && !me.beta;
        if not_yet {
            visibility = "code";
        }
        let files = req.files;
        let toml: Toml = files.get("potluck.toml").and_then(|t| toml::from_str(t).ok()).unwrap_or_default();
        let c = match req.code.as_deref().map(codes::clean).filter(|c| !c.is_empty()) {
            Some(code) => {
                let Some(c) = db::creation_by_code(&self.db(), &code)? else { return fail(404, "no creation has that code") };
                if c.owner != me.user_id {
                    return fail(403, "that's someone else's creation: remix it to share your own");
                }
                if c.listing == "removed" {
                    return fail(410, "that creation was taken down");
                }
                if c.visibility != visibility {
                    let db = self.db();
                    db.execute("UPDATE creations SET visibility = ?2 WHERE id = ?1", params![c.id, visibility])?;
                }
                let mut c = c;
                c.visibility = visibility.into();
                c
            }
            None => {
                if toml.name.trim().is_empty() {
                    return fail(400, "a creation needs a name in its potluck.toml");
                }
                let parent = match &req.from {
                    Some(f) => self.resolve_from(f)?,
                    None => None,
                };
                self.new_creation(me, &toml, visibility, parent, None).await?
            }
        };
        let total: usize = files.iter().map(|(k, v)| k.len() + v.len()).sum();
        if total > checks::MAX_SIZE * 2 || files.len() > checks::MAX_FILES * 2 {
            return fail(413, "that creation is far too big");
        }
        let mut shared = self.publish(me, &c, files, req.note.as_deref().unwrap_or(""), None).await?;
        if not_yet {
            shared.message = format!("{}. Sharing to Everyone is in a closed beta for now: an invite code lets you in", shared.message);
        }
        Ok(shared)
    }

    /// Remix from the web: a copy of a version under the signed-in player,
    /// with the maker's name kept on it, to open in the game by its code.
    pub async fn remix_on_web(&self, me: &Account, code: &str) -> R<Shared> {
        let _w = self.writing.lock().await;
        let Some((parent, v)) = self.resolve_from(&Parent { code: Some(code.to_string()), base: None, version: 0 })? else { return fail(404, "no creation has that code") };
        let files = self.version_files(&parent, v).await?;
        let mut files = files;
        let mut toml: toml::Table = files.get("potluck.toml").and_then(|t| t.parse().ok()).unwrap_or_default();
        let name = toml.get("name").and_then(|v| v.as_str()).unwrap_or(&parent.name).to_string();
        toml.insert("remix_of".into(), toml::Value::String(format!("{} ({})", parent.name, parent.code)));
        toml.insert("by".into(), toml::Value::String(me.name.clone()));
        toml.insert("version".into(), toml::Value::Integer(1));
        let mut from = toml::Table::new();
        from.insert("code".into(), toml::Value::String(parent.code.clone()));
        from.insert("version".into(), toml::Value::Integer(v));
        toml.insert("from".into(), toml::Value::Table(from));
        files.insert("potluck.toml".into(), toml::to_string_pretty(&toml).unwrap_or_default());
        let t = Toml { name: name.clone(), kind: parent.kind.clone(), about: parent.about.clone(), licence: parent.licence.clone(), ..Default::default() };
        let c = self.new_creation(me, &t, "code", Some((parent, v)), None).await?;
        self.publish(me, &c, files, &format!("Remixed {name}"), None).await
    }

    /// A version's files, from the store by hash.
    pub async fn version_files(&self, c: &Creation, n: i64) -> R<BTreeMap<String, String>> {
        let list = db::files(&self.db(), c.id, n)?;
        let mut out = BTreeMap::new();
        for (path, sha, _) in list {
            let bytes = self.store.get(&sha).await.map_err(|e| Fail { status: 502, message: e })?.ok_or(Fail { status: 500, message: format!("{path} is missing from the store") })?;
            out.insert(path, String::from_utf8_lossy(&bytes).to_string());
        }
        Ok(out)
    }

    /// What a code loads: the signed manifest of the newest version anyone
    /// may load (or its maker's newest), counting a play.
    pub fn load(&self, code: &str, version: Option<i64>, who: Option<&Account>, anon: &str) -> R<(Creation, db::Version)> {
        let code = codes::clean(code);
        let db = self.db();
        let Some(c) = db::creation_by_code(&db, &code)? else { return fail(404, "no creation has that code") };
        let mine = who.is_some_and(|w| w.user_id == c.owner || w.moderator);
        if c.listing == "removed" && !mine {
            return fail(410, "that creation was taken down");
        }
        if c.listing == "hidden" && !mine {
            return fail(451, "that creation is hidden while a moderator looks at it");
        }
        let v = match version {
            Some(n) => db::version(&db, c.id, n)?.filter(|v| mine || v.state != "held"),
            None if mine => db::version(&db, c.id, c.latest)?,
            None => db::loadable(&db, c.id)?,
        };
        let Some(v) = v else { return fail(404, "that creation has no version you can load yet") };
        let who_key = who.map(|w| w.user_id.clone()).unwrap_or_else(|| format!("anon:{anon}"));
        if who.map(|w| w.user_id != c.owner).unwrap_or(true) {
            let day = now() / 86400;
            if db.execute("INSERT OR IGNORE INTO plays (creation, who, day) VALUES (?1, ?2, ?3)", params![c.id, who_key, day])? > 0 {
                db.execute("UPDATE creations SET plays = plays + 1 WHERE id = ?1", [c.id])?;
            }
        }
        Ok((c, v))
    }

    pub fn star(&self, me: &Account, code: &str, on: bool) -> R<i64> {
        let db = self.db();
        let Some(c) = db::creation_by_code(&db, &codes::clean(code))? else { return fail(404, "no creation has that code") };
        let changed = if on {
            db.execute("INSERT OR IGNORE INTO stars (user_id, creation, created) VALUES (?1, ?2, ?3)", params![me.user_id, c.id, now()])?
        } else {
            db.execute("DELETE FROM stars WHERE user_id = ?1 AND creation = ?2", params![me.user_id, c.id])?
        };
        if changed > 0 {
            db.execute("UPDATE creations SET stars = (SELECT COUNT(*) FROM stars WHERE creation = ?1) WHERE id = ?1", [c.id])?;
        }
        Ok(db.query_row("SELECT stars FROM creations WHERE id = ?1", [c.id], |r| r.get(0))?)
    }

    pub fn starred(&self, me: &Account, c: &Creation) -> bool {
        self.db().query_row("SELECT 1 FROM stars WHERE user_id = ?1 AND creation = ?2", params![me.user_id, c.id], |_| Ok(())).optional().ok().flatten().is_some()
    }

    /// A player's report. Every report puts the creation in front of a
    /// moderator; enough different players hide it until one decides.
    pub fn report(&self, me: &Account, code: &str, reason: &str, note: &str) -> R<()> {
        let reason = match reason {
            "unkind" | "unsafe" | "broken" | "copied" | "other" => reason,
            _ => return fail(400, "pick a reason: unkind, unsafe, broken, copied or other"),
        };
        let db = self.db();
        let Some(c) = db::creation_by_code(&db, &codes::clean(code))? else { return fail(404, "no creation has that code") };
        if c.owner == me.user_id {
            return fail(400, "you can't report your own creation");
        }
        let recent: i64 = db.query_row("SELECT COUNT(*) FROM reports WHERE reporter = ?1 AND created > ?2", params![me.user_id, now() - 3600], |r| r.get(0))?;
        if recent >= 10 {
            return fail(429, "that's a lot of reports in an hour; a moderator will get to them");
        }
        let note: String = note.chars().take(500).collect();
        db.execute(
            "INSERT INTO reports (creation, version, reporter, reason, note, created) VALUES (?1, ?2, ?3, ?4, ?5, ?6) ON CONFLICT (creation, reporter) DO UPDATE SET reason = ?4, note = ?5, open = 1",
            params![c.id, c.latest, me.user_id, reason, note, now()],
        )?;
        let open: i64 = db.query_row("SELECT COUNT(*) FROM reports WHERE creation = ?1 AND open = 1", [c.id], |r| r.get(0))?;
        db::enqueue(&db, c.id, c.latest, "reports", &format!("{open} open"))?;
        if open >= self.cfg.hide_after_reports as i64 && matches!(c.listing.as_str(), "listed" | "unlisted" | "waiting") {
            db.execute("UPDATE creations SET hidden_from = listing, listing = 'hidden' WHERE id = ?1", [c.id])?;
            if let Some(c) = db::creation(&db, c.id)? {
                db::index_search(&db, &c, "")?;
            }
            db::log(&db, "reports", "hide", &c.code, &format!("{open} reports"));
        }
        Ok(())
    }

    /// Suggest a change: the player's remix, as a pull request to the
    /// creation it was remixed from.
    pub async fn suggest(&self, me: &Account, code: &str, title: &str, body: &str) -> R<i64> {
        let _w = self.writing.lock().await;
        let (remix, parent) = {
            let db = self.db();
            let Some(remix) = db::creation_by_code(&db, &codes::clean(code))? else { return fail(404, "no creation has that code") };
            if remix.owner != me.user_id {
                return fail(403, "you can only suggest changes from your own remix");
            }
            let Some(parent) = remix.parent.and_then(|p| db::creation(&db, p).ok().flatten()) else { return fail(400, "that isn't a remix of anything on the market") };
            if !parent.visible_to_all() {
                return fail(410, "the creation it was remixed from isn't on the market any more");
            }
            if remix.latest == 0 || db::loadable(&db, remix.id)?.map(|v| v.n) != Some(remix.latest) {
                return fail(409, "share your remix first; its newest version has to pass its checks");
            }
            (remix, parent)
        };
        let title: String = if title.trim().is_empty() { format!("A change from {}", me.name) } else { title.trim().chars().take(100).collect() };
        let body: String = body.chars().take(2000).collect();
        let existing: Option<(i64, i64)> = self
            .db()
            .query_row("SELECT id, pull FROM suggestions WHERE creation = ?1 AND from_creation = ?2 AND state = 'open'", params![parent.id, remix.id], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?;
        let scratch = self.cfg.data.join("tmp");
        let _ = std::fs::create_dir_all(&scratch);
        let id = match existing {
            Some((id, _)) => {
                // the same remix suggesting again: its branch moves on to the newest version
                let sha = self.git.copy_commit(&remix.path(), "main", &parent.path(), &format!("suggestion-{id}"), &scratch).await?;
                self.keep_name_on_branch(&parent, &format!("suggestion-{id}"), &sha, me).await?;
                self.db().execute("UPDATE suggestions SET from_version = ?2, title = ?3, body = ?4 WHERE id = ?1", params![id, remix.latest, title, body])?;
                id
            }
            None => {
                let id: i64 = {
                    let db = self.db();
                    db.execute(
                        "INSERT INTO suggestions (creation, from_creation, from_version, pull, by_user, title, body, state, created) VALUES (?1, ?2, ?3, 0, ?4, ?5, ?6, 'opening', ?7)",
                        params![parent.id, remix.id, remix.latest, me.user_id, title, body, now()],
                    )?;
                    db.last_insert_rowid()
                };
                let branch = format!("suggestion-{id}");
                let opened = async {
                    let sha = self.git.copy_commit(&remix.path(), "main", &parent.path(), &branch, &scratch).await?;
                    self.keep_name_on_branch(&parent, &branch, &sha, me).await?;
                    self.git.ensure_user(&me.handle, &me.name).await?;
                    self.git.open_pull(&parent.path(), &branch, &me.handle, &title, &body).await
                }
                .await;
                match opened {
                    Ok(pull) => {
                        self.db().execute("UPDATE suggestions SET pull = ?2, state = 'open' WHERE id = ?1", params![id, pull as i64])?;
                    }
                    Err(e) => {
                        self.db().execute("DELETE FROM suggestions WHERE id = ?1", [id])?;
                        let _ = self.git.delete_branch(&parent.path(), &branch).await;
                        return Err(e.into());
                    }
                }
                id
            }
        };
        db::log(&self.db(), &me.user_id, "suggest", &parent.code, &format!("from {}", remix.code));
        Ok(id)
    }

    /// A remix renames its maker and says what it came from; on a suggestion's
    /// branch that goes back to the original's own, so the change shows (and
    /// merges) only what the remix really changed.
    async fn keep_name_on_branch(&self, parent: &Creation, branch: &str, sha: &str, me: &Account) -> Result<(), crate::forgejo::Error> {
        let repo = parent.path();
        let tree = self.git.tree(&repo, sha).await?;
        let Some(blob) = tree.get("potluck.toml") else { return Ok(()) };
        let theirs = String::from_utf8_lossy(&self.git.read(&repo, "potluck.toml", sha).await?).to_string();
        let ours = String::from_utf8_lossy(&self.git.read(&repo, "potluck.toml", "main").await?).to_string();
        if let Some(kept) = keep_identity(&ours, &theirs) {
            self.git.update_file(&repo, branch, "potluck.toml", blob, &kept, &me.handle, &me.name, "Keep the original's name and maker").await?;
        }
        Ok(())
    }

    pub fn suggestion(&self, id: i64) -> R<Suggestion> {
        let db = self.db();
        let s = db
            .query_row("SELECT id, creation, from_creation, from_version, pull, by_user, title, body, state, merged_version, created FROM suggestions WHERE id = ?1", [id], suggestion_row)
            .optional()?;
        match s {
            Some(s) => Ok(s),
            None => fail(404, "there's no such suggestion"),
        }
    }

    pub fn suggestions_for(&self, creation: i64, open_only: bool) -> R<Vec<Suggestion>> {
        let db = self.db();
        let mut st = db.prepare(&format!(
            "SELECT id, creation, from_creation, from_version, pull, by_user, title, body, state, merged_version, created FROM suggestions WHERE creation = ?1 {} ORDER BY id DESC",
            if open_only { "AND state = 'open'" } else { "AND state != 'opening'" }
        ))?;
        let rows = st.query_map([creation], suggestion_row)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// A suggestion's change, as git shows it.
    pub async fn suggestion_diff(&self, s: &Suggestion) -> R<String> {
        let Some(c) = db::creation(&self.db(), s.creation)? else { return fail(404, "gone") };
        if s.state != "open" {
            // merged or closed: what it changed against the version it was suggested to
            return Ok(String::new());
        }
        Ok(self.git.pull_diff(&c.path(), s.pull as u64).await?)
    }

    /// Merge, by the maker of the creation: the change becomes its next version.
    pub async fn merge(&self, me: &Account, id: i64) -> R<Shared> {
        let _w = self.writing.lock().await;
        let s = self.suggestion(id)?;
        let Some(c) = db::creation(&self.db(), s.creation)? else { return fail(404, "that creation is gone") };
        if c.owner != me.user_id {
            return fail(403, "only its maker can merge a change");
        }
        if s.state != "open" {
            return fail(409, "that suggestion isn't open");
        }
        let by = db::account(&self.db(), &s.by_user)?.map(|a| a.name).unwrap_or_else(|| "someone".into());
        let note = format!("Merged \u{201c}{}\u{201d} from {by}", s.title);
        let before = self.version_files(&c, c.latest).await?;
        self.git.merge_pull(&c.path(), s.pull as u64, &me.handle, &note).await?;
        let head = self.git.head(&c.path()).await?;
        let tree = self.git.tree(&c.path(), &head).await?;
        let mut files = BTreeMap::new();
        for path in tree.keys() {
            let bytes = self.git.read(&c.path(), path, &head).await?;
            files.insert(path.clone(), String::from_utf8_lossy(&bytes).to_string());
        }
        // A remix renames its maker (and says what it's a remix of): merging
        // its change keeps this creation's own name and maker.
        let mut commit = head;
        if let (Some(old), Some(new)) = (before.get("potluck.toml"), files.get("potluck.toml")) {
            if let Some(kept) = keep_identity(old, new) {
                files.insert("potluck.toml".into(), kept);
                if let Some(sha) = self.git.commit(&c.path(), &me.handle, &me.name, "Kept its own name and maker", &files).await? {
                    commit = sha;
                }
            }
        }
        let shared = self.publish(me, &c, files, &note, Some(commit)).await?;
        let db = self.db();
        db.execute("UPDATE suggestions SET state = 'merged', merged_version = ?2 WHERE id = ?1", params![id, shared.version])?;
        db::log(&db, &me.user_id, "merge", &c.code, &format!("suggestion {id}"));
        Ok(shared)
    }

    pub async fn close_suggestion(&self, me: &Account, id: i64) -> R<()> {
        let s = self.suggestion(id)?;
        let Some(c) = db::creation(&self.db(), s.creation)? else { return fail(404, "that creation is gone") };
        if c.owner != me.user_id && s.by_user != me.user_id {
            return fail(403, "only its maker or the one who suggested it can close it");
        }
        if s.state != "open" {
            return Ok(());
        }
        self.git.close_pull(&c.path(), s.pull as u64, &me.handle).await?;
        let _ = self.git.delete_branch(&c.path(), &format!("suggestion-{id}")).await;
        self.db().execute("UPDATE suggestions SET state = 'closed' WHERE id = ?1", [id])?;
        Ok(())
    }

    /// The family tree around a creation: its line back to the first, and
    /// every remix below that first one.
    pub fn family(&self, c: &Creation) -> R<(Vec<Creation>, Vec<(Creation, usize)>)> {
        let db = self.db();
        let mut line = vec![c.clone()];
        let mut at = c.clone();
        while let Some(p) = at.parent.and_then(|p| db::creation(&db, p).ok().flatten()) {
            if line.iter().any(|x| x.id == p.id) || line.len() > 50 {
                break;
            }
            line.push(p.clone());
            at = p;
        }
        line.reverse();
        let root = line[0].clone();
        let mut tree = Vec::new();
        fn walk(db: &Connection, node: &Creation, depth: usize, out: &mut Vec<(Creation, usize)>) -> rusqlite::Result<()> {
            out.push((node.clone(), depth));
            if out.len() > 300 || depth > 30 {
                return Ok(());
            }
            let kids = db::creations_where(db, "WHERE c.parent = ?1 ORDER BY c.created", &[&node.id])?;
            for k in kids {
                walk(db, &k, depth + 1, out)?;
            }
            Ok(())
        }
        walk(&db, &root, 0, &mut tree)?;
        Ok((line, tree))
    }

    /// The base game's creations on the market, owned by Potluck itself, so
    /// remixes of them have somewhere to start their family tree.
    pub async fn seed_base(&self) -> R<()> {
        let _w = self.writing.lock().await;
        let me = {
            let db = self.db();
            let a = db::account(&db, POTLUCK)?;
            if a.is_none() {
                db.execute("INSERT INTO accounts (user_id, handle, name, created, beta, trusted) VALUES (?1, ?1, 'Potluck', ?2, 1, 1)", params![POTLUCK, now()])?;
            }
            db::account(&db, POTLUCK)?.expect("made")
        };
        for (id, list) in BASE {
            let files: BTreeMap<String, String> = list.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
            let toml: Toml = toml::from_str(&files["potluck.toml"]).unwrap_or_default();
            let existing = db::creation_by_base(&self.db(), id)?;
            let c = match existing {
                Some(c) => {
                    let now_files = db::files(&self.db(), c.id, c.latest)?;
                    let same = now_files.len() == files.len() && now_files.iter().all(|(p, h, _)| files.get(p).map(|t| codes::sha256_hex(t.as_bytes())) == Some(h.clone()));
                    if same {
                        continue;
                    }
                    c
                }
                None => self.new_creation(&me, &toml, "everyone", None, Some(id)).await?,
            };
            let s = self.publish(&me, &c, files, &format!("{} from the base game", toml.name), None).await?;
            println!("base creation {id} is {} version {} ({})", codes::pretty(&s.code), s.version, s.state);
        }
        Ok(())
    }

    // ---------- moderation ----------

    /// A moderator's decision on something in the review queue.
    pub fn decide(&self, moderator: &Account, item: i64, decision: &str, note: &str) -> R<()> {
        if !moderator.moderator {
            return fail(403, "only moderators can do that");
        }
        let db = self.db();
        let (creation, version): (i64, i64) = match db.query_row("SELECT creation, version FROM queue WHERE id = ?1", [item], |r| Ok((r.get(0)?, r.get(1)?))).optional()? {
            Some(x) => x,
            None => return fail(404, "that's not in the queue"),
        };
        let Some(c) = db::creation(&db, creation)? else { return fail(404, "gone") };
        let close_all = |what: &str| -> rusqlite::Result<()> {
            db.execute("UPDATE queue SET open = 0, decided_by = ?2, decision = ?3, decided_at = ?4 WHERE creation = ?1 AND open = 1", params![creation, moderator.user_id, what, now()])?;
            Ok(())
        };
        match decision {
            // looked at and fine: the waiting versions are ok, and it's listed if its maker asked
            "list" => {
                db.execute("UPDATE versions SET state = 'ok' WHERE creation = ?1 AND state = 'waiting' AND n <= ?2", params![creation, version])?;
                let listing = if c.visibility == "everyone" { "listed" } else { "unlisted" };
                db.execute("UPDATE creations SET listing = ?2, hidden_from = NULL WHERE id = ?1", params![creation, listing])?;
                db.execute("UPDATE reports SET open = 0 WHERE creation = ?1", [creation])?;
                close_all("list")?;
            }
            // fine to load by code, but not for the market's front page
            "unlist" => {
                db.execute("UPDATE versions SET state = 'ok' WHERE creation = ?1 AND state = 'waiting' AND n <= ?2", params![creation, version])?;
                db.execute("UPDATE creations SET listing = 'unlisted', hidden_from = NULL WHERE id = ?1", [creation])?;
                db.execute("UPDATE reports SET open = 0 WHERE creation = ?1", [creation])?;
                close_all("unlist")?;
            }
            "hide" => {
                db.execute("UPDATE creations SET hidden_from = COALESCE(hidden_from, listing), listing = 'hidden' WHERE id = ?1", [creation])?;
                close_all("hide")?;
            }
            // taken down, and its files can't come back in another creation
            "remove" => {
                db.execute("UPDATE creations SET listing = 'removed' WHERE id = ?1", [creation])?;
                let files = db::files(&db, creation, version)?;
                for (path, sha, _) in files {
                    if path == "AGENTS.md" || path.starts_with("types/") || path == "potluck.toml" {
                        continue;
                    }
                    db.execute("INSERT OR IGNORE INTO banned_files (sha256, created, note) VALUES (?1, ?2, ?3)", params![sha, now(), format!("{} {path}", c.code)])?;
                }
                db.execute("UPDATE reports SET open = 0 WHERE creation = ?1", [creation])?;
                close_all("remove")?;
            }
            // the reports were wrong: back to how it was
            "dismiss" => {
                db.execute("UPDATE creations SET listing = COALESCE(hidden_from, listing), hidden_from = NULL WHERE id = ?1 AND listing = 'hidden'", [creation])?;
                db.execute("UPDATE reports SET open = 0 WHERE creation = ?1", [creation])?;
                // every reason this version waited for was on the one page
                db.execute(
                    "UPDATE queue SET open = 0, decided_by = ?2, decision = 'dismiss', decided_at = ?3 WHERE id = ?1 OR (creation = ?4 AND version = ?5 AND open = 1)",
                    params![item, moderator.user_id, now(), creation, version],
                )?;
            }
            _ => return fail(400, "that isn't a decision"),
        }
        if let Some(c) = db::creation(&db, creation)? {
            let text: String = db.query_row("SELECT text FROM search WHERE code = ?1", [&c.code], |r| r.get(0)).optional()?.unwrap_or_default();
            db::index_search(&db, &c, &text)?;
        }
        db::log(&db, &moderator.user_id, decision, &c.code, note);
        Ok(())
    }

    pub fn set_flag(&self, moderator: &Account, handle: &str, flag: &str, on: bool) -> R<()> {
        if !moderator.moderator {
            return fail(403, "only moderators can do that");
        }
        let column = match flag {
            "trusted" | "beta" | "banned" => flag,
            "moderator" if moderator.user_id != POTLUCK => "moderator",
            _ => return fail(400, "that isn't something a moderator can change"),
        };
        let db = self.db();
        let n = db.execute(&format!("UPDATE accounts SET {column} = ?2 WHERE handle = ?1 COLLATE NOCASE"), params![handle, on])?;
        if n == 0 {
            return fail(404, "there's no maker with that name");
        }
        db::log(&db, &moderator.user_id, &format!("{column}={on}"), handle, "");
        Ok(())
    }

    pub fn make_invites(&self, by: &str, n: usize) -> R<Vec<String>> {
        let db = self.db();
        let mut out = Vec::new();
        for _ in 0..n.min(50) {
            let code = codes::new_code();
            db.execute("INSERT OR IGNORE INTO invites (code, made_by, created) VALUES (?1, ?2, ?3)", params![code, by, now()])?;
            out.push(code);
        }
        Ok(out)
    }

    pub fn redeem_invite(&self, me: &Account, code: &str) -> R<()> {
        let code = codes::clean(code);
        let db = self.db();
        if me.beta {
            return Ok(());
        }
        let n = db.execute("UPDATE invites SET used_by = ?2, used_at = ?3 WHERE code = ?1 AND used_by IS NULL", params![code, me.user_id, now()])?;
        if n == 0 {
            return fail(404, "that invite doesn't work: it's used, or mistyped");
        }
        db.execute("UPDATE accounts SET beta = 1 WHERE user_id = ?1", [&me.user_id])?;
        db::log(&db, &me.user_id, "beta", &me.handle, &code);
        Ok(())
    }

    // ---------- signing in on the web ----------

    /// A one-time link that signs a browser in as this player and opens
    /// `next`, one of the market's own pages. The game shows it as a QR code
    /// too, so a player on a controller can carry on on their phone.
    pub fn signin_link(&self, me: &Account, next: &str) -> R<String> {
        let next = local_path(next).unwrap_or("/me");
        let ticket = codes::token();
        let db = self.db();
        db.execute("DELETE FROM tickets WHERE expires < ?1", [now()])?;
        db.execute("INSERT INTO tickets (ticket, user_id, expires, next) VALUES (?1, ?2, ?3, ?4)", params![ticket, me.user_id, now() + 300, next])?;
        Ok(format!("{}/signin/{ticket}", self.cfg.public_url))
    }

    /// Uses a link: a new web session, and the page to open.
    pub fn use_ticket(&self, ticket: &str) -> R<(String, String)> {
        let db = self.db();
        let found: Option<(String, String)> = db.query_row("SELECT user_id, next FROM tickets WHERE ticket = ?1 AND expires >= ?2", params![ticket, now()], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
        db.execute("DELETE FROM tickets WHERE ticket = ?1", [ticket])?;
        let Some((user, next)) = found else { return fail(410, "that link has been used or has run out: open the market from the game again") };
        let session = codes::token();
        db.execute("INSERT INTO web_sessions (token, user_id, expires) VALUES (?1, ?2, ?3)", params![session, user, now() + 30 * 86400])?;
        Ok((session, next))
    }

    /// A maker's profile, from the web: the name everyone sees, a line about
    /// them and their block head. Once named here, the game's name stops
    /// changing it.
    pub fn update_profile(&self, me: &Account, name: &str, about: &str, avatar: Option<Avatar>) -> R<Account> {
        let name: String = name.trim().chars().filter(|c| !c.is_control()).take(32).collect();
        let about: String = about.trim().chars().map(|c| if c.is_control() { ' ' } else { c }).take(280).collect();
        if name.is_empty() {
            return fail(400, "your name can't be empty");
        }
        let hits: Vec<String> = self.words.find(&name).into_iter().chain(self.words.find(&about)).collect();
        if !hits.is_empty() {
            return fail(400, "your name or about has words the market doesn't allow: try others");
        }
        let avatar = avatar.map(|a| a.encode()).unwrap_or_else(|| me.avatar.clone());
        let db = self.db();
        db.execute("UPDATE accounts SET name = ?2, about = ?3, avatar = ?4, named_on_web = 1 WHERE user_id = ?1", params![me.user_id, name, about, avatar])?;
        db::log(&db, &me.user_id, "profile", &me.handle, "");
        // Search finds creations by their maker's name too.
        db.execute("UPDATE search SET maker = ?2 WHERE code IN (SELECT code FROM creations WHERE owner = ?1)", params![me.user_id, format!("{} {}", name, me.handle)])?;
        Ok(db::account(&db, &me.user_id)?.expect("the account"))
    }

    pub fn web_account(&self, session: &str) -> Option<Account> {
        let db = self.db();
        let user: Option<String> = db.query_row("SELECT user_id FROM web_sessions WHERE token = ?1 AND expires >= ?2", params![session, now()], |r| r.get(0)).optional().ok().flatten();
        user.and_then(|u| db::account(&db, &u).ok().flatten()).filter(|a| !a.banned)
    }

    pub fn sign_out(&self, session: &str) {
        let _ = self.db().execute("DELETE FROM web_sessions WHERE token = ?1", [session]);
    }
}

#[derive(Deserialize)]
pub struct ShareReq {
    #[serde(default)]
    pub code: Option<String>,
    pub files: BTreeMap<String, String>,
    #[serde(default)]
    pub visibility: String,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub from: Option<Parent>,
}

#[derive(Serialize, Clone, Debug)]
pub struct Suggestion {
    pub id: i64,
    pub creation: i64,
    pub from_creation: i64,
    pub from_version: i64,
    pub pull: i64,
    pub by_user: String,
    pub title: String,
    pub body: String,
    pub state: String,
    pub merged_version: Option<i64>,
    pub created: i64,
}

fn suggestion_row(r: &rusqlite::Row) -> rusqlite::Result<Suggestion> {
    Ok(Suggestion {
        id: r.get(0)?,
        creation: r.get(1)?,
        from_creation: r.get(2)?,
        from_version: r.get(3)?,
        pull: r.get(4)?,
        by_user: r.get(5)?,
        title: r.get(6)?,
        body: r.get(7)?,
        state: r.get(8)?,
        merged_version: r.get(9)?,
        created: r.get(10)?,
    })
}

/// A path on the market itself: `/me`, `/c/ABCD2345`. Never another site,
/// so a sign-in link can't be made to send anyone away.
pub fn local_path(p: &str) -> Option<&str> {
    let ok = p.starts_with('/') && !p.starts_with("//") && p.len() < 200 && p.chars().all(|c| c.is_ascii_alphanumeric() || "/-_.@?=&#".contains(c));
    ok.then_some(p)
}

/// The fields of potluck.toml that say whose a creation is. A merged
/// suggestion brings the remix's; this puts the creation's own back, or
/// None when they already match.
pub fn keep_identity(old: &str, merged: &str) -> Option<String> {
    let before: toml::Table = old.parse().ok()?;
    let after: toml::Table = merged.parse().ok()?;
    let ours: toml_edit::DocumentMut = old.parse().ok()?;
    // Edit the merged file in place, so the rest of it keeps its order and layout.
    let mut doc: toml_edit::DocumentMut = merged.parse().ok()?;
    let mut changed = false;
    for key in ["name", "by", "remix_of", "from", "version"] {
        if before.get(key) == after.get(key) {
            continue;
        }
        changed = true;
        match ours.get(key) {
            Some(item) => {
                doc.insert(key, item.clone());
            }
            None => {
                doc.remove(key);
            }
        }
    }
    changed.then(|| doc.to_string())
}
