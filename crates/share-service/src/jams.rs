//! Jams: a theme for a week ("Things that float"), set by a moderator.
//! While it runs, makers enter creations they've shared to Everyone; when it
//! ends, the four entries that got the most stars while it ran are its
//! favourites, and they go on the arches of every new world's Green for a
//! month.

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

use crate::codes;
use crate::db::{self, now, Account, Creation};
use crate::market::{fail, slug, App, R};

pub const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS jams (
    id INTEGER PRIMARY KEY,
    slug TEXT NOT NULL UNIQUE,
    theme TEXT NOT NULL,
    about TEXT NOT NULL DEFAULT '',
    starts INTEGER NOT NULL,
    ends INTEGER NOT NULL,
    made_by TEXT NOT NULL,
    created INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS jam_entries (
    jam INTEGER NOT NULL REFERENCES jams(id),
    creation INTEGER NOT NULL REFERENCES creations(id),
    entered INTEGER NOT NULL,
    PRIMARY KEY (jam, creation)
);
CREATE INDEX IF NOT EXISTS jam_entries_creation ON jam_entries(creation);
"#;

pub const DAY: i64 = 86400;
/// A jam's favourites: one for each arch on the Green.
pub const FAVOURITES: usize = 4;
/// How long a jam's favourites stay on new worlds' Greens after it ends.
pub const ON_THE_GREEN: i64 = 30 * DAY;
/// Entries one maker may have in one jam.
pub const PER_MAKER: i64 = 3;

#[derive(Clone, Debug, Serialize)]
pub struct Jam {
    pub id: i64,
    pub slug: String,
    pub theme: String,
    pub about: String,
    pub starts: i64,
    pub ends: i64,
}

impl Jam {
    pub fn running(&self) -> bool {
        let n = now();
        self.starts <= n && n < self.ends
    }

    pub fn over(&self) -> bool {
        now() >= self.ends
    }
}

/// An entry in a jam, with the stars it got while the jam ran.
#[derive(Clone, Debug)]
pub struct Entry {
    pub creation: Creation,
    pub stars: i64,
}

/// One of the Green's arches: a creation, and why it's there.
#[derive(Clone, Debug)]
pub struct Arch {
    pub creation: Creation,
    pub why: String,
}

const COLS: &str = "id, slug, theme, about, starts, ends";

fn jam_row(r: &rusqlite::Row) -> rusqlite::Result<Jam> {
    Ok(Jam { id: r.get(0)?, slug: r.get(1)?, theme: r.get(2)?, about: r.get(3)?, starts: r.get(4)?, ends: r.get(5)? })
}

fn jams_where(db: &Connection, clause: &str, args: &[&dyn rusqlite::ToSql]) -> rusqlite::Result<Vec<Jam>> {
    let mut st = db.prepare(&format!("SELECT {COLS} FROM jams {clause}"))?;
    let rows = st.query_map(args, jam_row)?;
    rows.collect()
}

/// A jam's listed entries, most stars while it ran first.
fn entries(db: &Connection, jam: &Jam) -> rusqlite::Result<Vec<Entry>> {
    let stars = "(SELECT COUNT(*) FROM stars s WHERE s.creation = c.id AND s.created >= ?2 AND s.created < ?3)";
    let list = db::creations_where(
        db,
        &format!("JOIN jam_entries e ON e.creation = c.id WHERE e.jam = ?1 AND c.listing = 'listed' ORDER BY {stars} DESC, c.plays DESC, e.entered"),
        &[&jam.id, &jam.starts, &jam.ends],
    )?;
    list.into_iter()
        .map(|c| {
            let n: i64 = db.query_row("SELECT COUNT(*) FROM stars WHERE creation = ?1 AND created >= ?2 AND created < ?3", params![c.id, jam.starts, jam.ends], |r| r.get(0))?;
            Ok(Entry { creation: c, stars: n })
        })
        .collect()
}

impl App {
    /// A new jam, from a moderator: a theme, a few words about it, and when
    /// it runs. Jams don't overlap.
    pub fn make_jam(&self, moderator: &Account, theme: &str, about: &str, starts: i64, days: i64) -> R<Jam> {
        if !moderator.moderator {
            return fail(403, "only moderators can start a jam");
        }
        let theme: String = theme.trim().chars().filter(|c| !c.is_control()).collect();
        let about: String = about.trim().chars().take(400).collect();
        if theme.is_empty() || theme.chars().count() > 60 {
            return fail(400, "a jam's theme is 1 to 60 letters");
        }
        if !(1..=31).contains(&days) {
            return fail(400, "a jam runs for 1 to 31 days");
        }
        let starts = if starts <= 0 { now() } else { starts };
        let ends = starts + days * DAY;
        let db = self.db();
        let overlap: i64 = db.query_row("SELECT COUNT(*) FROM jams WHERE starts < ?2 AND ends > ?1", params![starts, ends], |r| r.get(0))?;
        if overlap > 0 {
            return fail(409, "another jam runs then: one at a time");
        }
        let base = slug(&theme);
        let mut s = base.clone();
        let mut n = 2;
        while db.query_row("SELECT 1 FROM jams WHERE slug = ?1", [&s], |_| Ok(())).optional()?.is_some() {
            s = format!("{base}-{n}");
            n += 1;
        }
        db.execute(
            "INSERT INTO jams (slug, theme, about, starts, ends, made_by, created) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![s, theme, about, starts, ends, moderator.user_id, now()],
        )?;
        let id = db.last_insert_rowid();
        db::log(&db, &moderator.user_id, "jam", &s, &theme);
        Ok(Jam { id, slug: s, theme, about, starts, ends })
    }

    /// Every jam, the newest first.
    pub fn jams(&self) -> R<Vec<Jam>> {
        Ok(jams_where(&self.db(), "ORDER BY starts DESC", &[])?)
    }

    pub fn jam(&self, slug: &str) -> R<Jam> {
        let mut list = jams_where(&self.db(), "WHERE slug = ?1", &[&slug])?;
        list.pop().ok_or(crate::market::Fail { status: 404, message: "there's no jam called that".into() })
    }

    /// The jam running now, if there is one.
    pub fn current_jam(&self) -> R<Option<Jam>> {
        let n = now();
        Ok(jams_where(&self.db(), "WHERE starts <= ?1 AND ends > ?1 ORDER BY starts DESC LIMIT 1", &[&n])?.pop())
    }

    pub fn jam_entries(&self, jam: &Jam) -> R<Vec<Entry>> {
        Ok(entries(&self.db(), jam)?)
    }

    /// The favourites of a jam that's over: its first four entries.
    pub fn favourites(&self, jam: &Jam) -> R<Vec<Entry>> {
        if !jam.over() {
            return Ok(vec![]);
        }
        let mut list = self.jam_entries(jam)?;
        list.truncate(FAVOURITES);
        Ok(list)
    }

    /// The jams a creation is in, the newest first.
    pub fn jams_of(&self, c: &Creation) -> R<Vec<Jam>> {
        Ok(jams_where(&self.db(), "WHERE id IN (SELECT jam FROM jam_entries WHERE creation = ?1) ORDER BY starts DESC", &[&c.id])?)
    }

    /// The maker's creations that could go in the jam running now.
    pub fn could_enter(&self, me: &Account) -> R<Vec<Creation>> {
        let Some(jam) = self.current_jam()? else { return Ok(vec![]) };
        Ok(db::creations_where(
            &self.db(),
            "WHERE c.owner = ?1 AND c.visibility = 'everyone' AND c.listing IN ('listed', 'waiting') AND c.base IS NULL AND c.id NOT IN (SELECT creation FROM jam_entries WHERE jam = ?2) ORDER BY c.updated DESC",
            &[&me.user_id, &jam.id],
        )?)
    }

    /// Puts one of the maker's creations in a jam while it runs. It has to
    /// be shared to Everyone; one waiting for a moderator shows once it's
    /// listed.
    pub fn enter_jam(&self, me: &Account, jam_slug: &str, code: &str) -> R<Jam> {
        let jam = self.jam(jam_slug)?;
        if !jam.running() {
            return fail(409, if jam.over() { "that jam is over" } else { "that jam hasn't started yet" });
        }
        let db = self.db();
        let Some(c) = db::creation_by_code(&db, &codes::clean(code))? else { return fail(404, "no creation has that code") };
        if c.owner != me.user_id {
            return fail(403, "only its maker can enter a creation in a jam");
        }
        if c.visibility != "everyone" || !matches!(c.listing.as_str(), "listed" | "waiting") {
            return fail(409, "share it to Everyone first: jam entries are on the market for anyone to try");
        }
        let mine: i64 = db.query_row(
            "SELECT COUNT(*) FROM jam_entries e JOIN creations c ON c.id = e.creation WHERE e.jam = ?1 AND c.owner = ?2",
            params![jam.id, me.user_id],
            |r| r.get(0),
        )?;
        let already = db.query_row("SELECT 1 FROM jam_entries WHERE jam = ?1 AND creation = ?2", params![jam.id, c.id], |_| Ok(())).optional()?.is_some();
        if already {
            return Ok(jam);
        }
        if mine >= PER_MAKER {
            return fail(409, format!("each maker can enter {PER_MAKER} creations in a jam"));
        }
        db.execute("INSERT INTO jam_entries (jam, creation, entered) VALUES (?1, ?2, ?3)", params![jam.id, c.id, now()])?;
        db::log(&db, &me.user_id, "jam_entry", &c.code, &jam.slug);
        Ok(jam)
    }

    /// Takes the maker's creation out of a jam while it still runs.
    pub fn leave_jam(&self, me: &Account, jam_slug: &str, code: &str) -> R<()> {
        let jam = self.jam(jam_slug)?;
        if jam.over() {
            return fail(409, "that jam is over: its entries stay as they were");
        }
        let db = self.db();
        let Some(c) = db::creation_by_code(&db, &codes::clean(code))? else { return fail(404, "no creation has that code") };
        if c.owner != me.user_id {
            return fail(403, "only its maker can take it out");
        }
        db.execute("DELETE FROM jam_entries WHERE jam = ?1 AND creation = ?2", params![jam.id, c.id])?;
        Ok(())
    }

    /// What goes on the four arches of a new world's Green: favourites of
    /// jams that ended in the last month, then the jam running now, then
    /// what's trending. Younger players only get creations checked for all
    /// ages.
    pub fn green(&self, young: bool) -> R<Vec<Arch>> {
        let mut arches: Vec<Arch> = vec![];
        let fits = |c: &Creation, arches: &[Arch]| c.listing == "listed" && (!young || c.all_ages) && !arches.iter().any(|a| a.creation.id == c.id);
        let n = now();
        let ended = jams_where(&self.db(), "WHERE ends <= ?1 AND ends > ?2 ORDER BY ends DESC", &[&n, &(n - ON_THE_GREEN)])?;
        for jam in &ended {
            for e in self.favourites(jam)? {
                if arches.len() < FAVOURITES && fits(&e.creation, &arches) {
                    arches.push(Arch { why: format!("A favourite in the {} jam", jam.theme), creation: e.creation });
                }
            }
        }
        if let Some(jam) = self.current_jam()? {
            for e in self.jam_entries(&jam)? {
                if arches.len() < FAVOURITES && fits(&e.creation, &arches) {
                    arches.push(Arch { why: format!("In this week's jam, {}", jam.theme), creation: e.creation });
                }
            }
        }
        if arches.len() < FAVOURITES {
            let trending = db::creations_where(
                &self.db(),
                "WHERE c.listing = 'listed' AND c.base IS NULL ORDER BY (c.stars * 3 + c.plays + c.remixes * 5 + 1) * 1.0 / ((strftime('%s','now') - c.updated) / 86400.0 + 2) DESC LIMIT 40",
                &[],
            )?;
            for c in trending {
                if arches.len() < FAVOURITES && fits(&c, &arches) {
                    arches.push(Arch { why: "Trending on the market".into(), creation: c });
                }
            }
        }
        Ok(arches)
    }
}
