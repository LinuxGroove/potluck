//! The web market's pages: Explore, a creation's page (code, versions,
//! remixes, suggested changes), makers, signing in from the game, and the
//! review queue for moderators. Everything shared to Everyone can be read
//! without signing in.

use std::collections::BTreeMap;

use axum::body::Body;
use axum::extract::{Form, Path, Query, State};
use axum::http::{header, HeaderMap, Method, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::Router;
use maud::{html, Markup, PreEscaped, DOCTYPE};
use rusqlite::{params, OptionalExtension};
use serde::Deserialize;

use crate::api::Shared;
use crate::avatar::{self, Avatar};
use crate::checks::Check;
use crate::codes::{self, pretty};
use crate::db::{self, now, Account, Creation};
use crate::highlight;
use crate::market::{App, Fail, Manifest};

const CSS: &str = include_str!("../market/market.css");
const JS: &str = include_str!("../market/market.js");
const COOKIE: &str = "potluck";

pub fn routes() -> Router<Shared> {
    Router::new()
        .route("/", get(explore))
        .route("/static/market.css", get(|| async { ([(header::CONTENT_TYPE, "text/css"), (header::CACHE_CONTROL, "public, max-age=3600")], CSS) }))
        .route("/static/market.js", get(|| async { ([(header::CONTENT_TYPE, "text/javascript"), (header::CACHE_CONTROL, "public, max-age=3600")], JS) }))
        .route("/c/{code}", get(creation))
        .route("/c/{code}/v/{n}", get(version))
        .route("/c/{code}/s/{id}", get(suggestion))
        .route("/c/{code}/star", post(star))
        .route("/c/{code}/all-ages", post(all_ages))
        .route("/c/{code}/remix", post(remix))
        .route("/c/{code}/report", post(report))
        .route("/c/{code}/s/{id}/merge", post(merge))
        .route("/c/{code}/s/{id}/close", post(close))
        .route("/{who}", get(maker))
        .route("/signin", get(signin_help))
        .route("/signin/{ticket}", get(signin))
        .route("/signout", post(signout))
        .route("/me", get(me))
        .route("/me/invite", post(invite))
        .route("/me/profile", get(profile).post(save_profile))
        .route("/me/servers", get(my_servers).post(add_server))
        .route("/me/servers/{id}/remove", post(remove_server))
        .route("/me/devices", get(my_devices))
        .route("/me/devices/code", post(device_code))
        .route("/me/devices/{id}/unlink", post(unlink_device))
        .route("/me/recovery", post(make_recovery))
        .route("/jams", get(jams_page).post(make_jam))
        .route("/jams/{slug}", get(jam_page))
        .route("/jams/{slug}/enter", post(enter_jam))
        .route("/jams/{slug}/leave", post(leave_jam))
        .route("/servers", get(servers_page))
        .route("/servers/{id}/hide", post(hide_server))
        .route("/avatar/{file}", get(avatar))
        .route("/review", get(review))
        .route("/review/{id}", get(review_item).post(decide))
        .route("/review/maker", post(maker_flag))
        .route("/review/invites", post(make_invites))
        .route("/git/{owner}/{repo}/{*rest}", get(git).post(git))
        .route("/robots.txt", get(|| async { "User-agent: *\nDisallow: /review\nDisallow: /git/\nDisallow: /f/\n" }))
}

// ---------- pieces ----------

struct Page {
    me: Option<Account>,
}

fn session(h: &HeaderMap) -> Option<String> {
    let cookies = h.get(header::COOKIE)?.to_str().ok()?;
    cookies.split(';').find_map(|c| c.trim().strip_prefix(&format!("{COOKIE}="))).map(|s| s.to_string())
}

fn page(app: &App, h: &HeaderMap) -> Page {
    let me = session(h).as_deref().and_then(|s| app.web_account(s));
    Page { me }
}

/// A form post must come from the market's own pages.
fn same_site(app: &App, h: &HeaderMap) -> Result<(), Response> {
    let want = app.cfg.public_url.as_str();
    let from = h.get(header::ORIGIN).or_else(|| h.get(header::REFERER)).and_then(|v| v.to_str().ok()).unwrap_or("");
    if from == want || from.starts_with(&format!("{want}/")) {
        Ok(())
    } else {
        Err((StatusCode::FORBIDDEN, "that form didn't come from the market").into_response())
    }
}

const LOGO: &str = r##"<svg viewBox="0 0 32 32" aria-hidden="true"><path d="M4 15h24a12 9 0 0 1-24 0z" fill="#e0663c"/><path d="M2 14h28" stroke="#8a3d22" stroke-width="2.4" stroke-linecap="round"/><path d="M11 10c0-2 2-2 2-4M16 10c0-2 2-2 2-4M21 10c0-2 2-2 2-4" stroke="#e0663c" stroke-width="2" fill="none" stroke-linecap="round"/></svg>"##;

fn layout(app: &App, p: &Page, title: &str, q: &str, body: Markup) -> Html<String> {
    Html(
        html! {
            (DOCTYPE)
            html lang="en" {
                head {
                    meta charset="utf-8";
                    meta name="viewport" content="width=device-width, initial-scale=1";
                    title { (title) " · Potluck market" }
                    link rel="stylesheet" href="/static/market.css";
                    script src="/static/market.js" defer {}
                }
                body {
                    header.top {
                        a.logo href="/" { (PreEscaped(LOGO)) "Potluck" }
                        form.search action="/" method="get" role="search" {
                            input type="search" name="q" value=(q) placeholder="Search creations, makers and code" aria-label="Search";
                        }
                        nav.top {
                            a href="/?sort=new" { "New" }
                            a href="/?sort=remixed" { "Most remixed" }
                            a href="/jams" { "Jams" }
                            a href="/servers" { "Servers" }
                        }
                        div.who {
                            @match &p.me {
                                Some(me) => {
                                    @if me.moderator { a.btn href="/review" { "Review" } }
                                    a.btn href="/me" { (head(&me.handle, 22)) (me.name) }
                                }
                                None => { a.btn href="/signin" { "Sign in" } }
                            }
                        }
                    }
                    main { (body) }
                    footer {
                        "Creations are shared by the people who made them, under the licence each one names. "
                        a href=(app.cfg.public_url) { "Potluck" } " is made by The LinuxGroove team."
                    }
                }
            }
        }
        .into_string(),
    )
}

fn error_page(app: &App, p: &Page, status: u16, message: &str) -> Response {
    let s = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (s, layout(app, p, "Not here", "", html! { div.empty { h1 { "Not here" } p { (message) } p { a.btn href="/" { "Back to Explore" } } } })).into_response()
}

fn fail_page(app: &App, p: &Page, f: Fail) -> Response {
    error_page(app, p, f.status, &f.message)
}

/// "1 star", "2 stars", "1 remix", "3 remixes".
pub fn count(n: i64, one: &str) -> String {
    match (n, one.ends_with('x') || one.ends_with('s')) {
        (1, _) => format!("1 {one}"),
        (_, true) => format!("{n} {one}es"),
        _ => format!("{n} {one}s"),
    }
}

pub fn ago(t: i64) -> String {
    let d = (now() - t).max(0);
    match d {
        0..=59 => "just now".into(),
        60..=3599 => format!("{} min ago", d / 60),
        3600..=86399 => format!("{} h ago", d / 3600),
        86400..=2_591_999 => format!("{} days ago", d / 86400),
        _ => format!("{} months ago", d / 2_592_000),
    }
}

fn card(c: &Creation) -> Markup {
    html! {
        a.card href={ "/c/" (c.code) } {
            span.kind { (c.kind) } " " span.codechip { (pretty(&c.code)) }
            @if c.all_ages { " " span.tag.ok { "all ages" } }
            h3 { (c.name) }
            div.by { (head(&c.handle, 18)) "by " (c.maker) }
            @if !c.about.is_empty() { p { (c.about) } }
            div.stats { span { (count(c.stars, "star")) } span { (count(c.plays, "play")) } span { (count(c.remixes, "remix")) } }
        }
    }
}

/// A maker's block head, drawn by /avatar/HANDLE.svg.
fn head(handle: &str, size: u32) -> Markup {
    html! { img.face src={ "/avatar/" (handle) ".svg" } width=(size) height=(size) alt=""; }
}

fn checks_list(checks: &[Check]) -> Markup {
    html! {
        ul.checks {
            @for c in checks {
                li class=(c.result) { span { b { (c.name) } " · " (c.note) } }
            }
        }
    }
}

fn listing_note(c: &Creation, latest_state: &str, mine: bool) -> Markup {
    html! {
        @match (c.listing.as_str(), latest_state) {
            ("removed", _) => div.note.bad { "This creation was taken down." },
            ("hidden", _) => div.note.bad { "Hidden while a moderator looks at reports about it." },
            (_, "held") if mine => div.note.bad { "Your newest version didn't pass its checks, so nobody else can load it yet. See Checks below." },
            ("waiting", _) if mine => div.note.warn { "A moderator will look at this before it's listed on the market. Anyone with the code can load it now." },
            ("unlisted", _) if mine => div.note.ok { "Shared with anyone who has the code. It isn't listed on the market." },
            _ => {}
        }
    }
}

// ---------- Explore ----------

#[derive(Deserialize, Default)]
struct ExploreQ {
    #[serde(default)]
    sort: String,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    q: String,
    /// "all": only creations checked for all ages
    #[serde(default)]
    ages: String,
}

async fn explore(State(app): State<Shared>, h: HeaderMap, Query(q): Query<ExploreQ>) -> Response {
    let p = page(&app, &h);
    let list = {
        let db = app.db();
        let order = match q.sort.as_str() {
            "new" => "c.created DESC",
            "remixed" => "c.remixes DESC, c.stars DESC",
            _ => "(c.stars * 3 + c.plays + c.remixes * 5 + 1) * 1.0 / ((strftime('%s','now') - c.updated) / 86400.0 + 2) DESC",
        };
        let kind = if crate::checks::KINDS.contains(&q.kind.as_str()) { q.kind.clone() } else { String::new() };
        let fts = db::search_query(&q.q);
        let ages = if q.ages == "all" { "AND EXISTS (SELECT 1 FROM versions v WHERE v.creation = c.id AND v.n = c.latest AND v.all_ages = 1)" } else { "" };
        let r = if !fts.is_empty() {
            db::creations_where(&db, &format!("JOIN search s ON s.code = c.code WHERE c.listing = 'listed' AND search MATCH ?1 AND (?2 = '' OR c.kind = ?2) {ages} ORDER BY rank LIMIT 60"), &[&fts, &kind])
        } else {
            db::creations_where(&db, &format!("WHERE c.listing = 'listed' AND (?1 = '' OR c.kind = ?1) {ages} ORDER BY {order} LIMIT 60"), &[&kind])
        };
        match r {
            Ok(l) => l,
            Err(e) => return fail_page(&app, &p, e.into()),
        }
    };
    let sort = if q.sort.is_empty() { "trending" } else { q.sort.as_str() };
    let all_ages = q.ages == "all";
    let href_ages = |s: &str, k: &str, all: bool| {
        let mut u = format!("/?sort={s}");
        if !k.is_empty() {
            u.push_str(&format!("&kind={k}"));
        }
        if all {
            u.push_str("&ages=all");
        }
        u
    };
    let href = |s: &str, k: &str| href_ages(s, k, all_ages);
    let jam = app.current_jam().ok().flatten();
    let body = html! {
        @if let Some(j) = &jam {
            @if q.q.is_empty() {
                a.jamband href={ "/jams/" (j.slug) } {
                    span.kind { "This week's jam" } " " b { (j.theme) } " " span.sub { "ends " (until(j.ends)) }
                }
            }
        }
        @if !q.q.is_empty() {
            h1 { "Creations matching \u{201c}" (q.q) "\u{201d}" }
        } @else {
            h1 { "Things people made" }
            p.sub { "Every creature, block and mini game here is a creation you can open in Potluck, look inside, remix and share." }
        }
        div.chips {
            @for (s, label) in [("trending", "Trending"), ("new", "New"), ("remixed", "Most remixed")] {
                a.chip.on[sort == s] href=(href(s, &q.kind)) { (label) }
            }
            span { " " }
            a.chip.on[q.kind.is_empty()] href=(href(sort, "")) { "Everything" }
            @for k in crate::checks::KINDS {
                a.chip.on[q.kind == *k] href=(href(sort, k)) { (k) "s" }
            }
            span { " " }
            a.chip.on[all_ages] href=(href_ages(sort, &q.kind, !all_ages)) title="Creations a moderator looked at and found fine for players of every age" { "Checked for all ages" }
        }
        @if list.is_empty() {
            div.empty { "Nothing here yet. Share something from the game and it can be the first." }
        } @else {
            div.grid { @for c in &list { (card(c)) } }
        }
    };
    layout(&app, &p, "Explore", &q.q, body).into_response()
}

// ---------- a creation ----------

#[derive(Deserialize, Default)]
struct CreationQ {
    #[serde(default)]
    tab: String,
    #[serde(default)]
    file: String,
    #[serde(default)]
    v: Option<i64>,
    #[serde(default)]
    done: String,
}

fn can_see(c: &Creation, me: &Option<Account>) -> bool {
    c.visible_to_all() || me.as_ref().is_some_and(|m| m.user_id == c.owner || m.moderator)
}

async fn creation(State(app): State<Shared>, h: HeaderMap, Path(code): Path<String>, Query(q): Query<CreationQ>) -> Response {
    let p = page(&app, &h);
    let clean = codes::clean(&code);
    if clean != code {
        return Redirect::permanent(&format!("/c/{clean}")).into_response();
    }
    let Ok(Some(c)) = db::creation_by_code(&app.db(), &clean) else { return error_page(&app, &p, 404, "No creation has that code. Codes are eight letters and numbers, like N8GT-3WFA.") };
    if !can_see(&c, &p.me) {
        return error_page(&app, &p, if c.listing == "removed" { 410 } else { 451 }, if c.listing == "removed" { "That creation was taken down." } else { "That creation is hidden while a moderator looks at it." });
    }
    let mine = p.me.as_ref().is_some_and(|m| m.user_id == c.owner);
    let (versions, parent, suggestions_open, starred) = {
        let db = app.db();
        let versions = db::versions(&db, c.id).unwrap_or_default();
        let parent = c.parent.and_then(|id| db::creation(&db, id).ok().flatten());
        let open: i64 = db.query_row("SELECT COUNT(*) FROM suggestions WHERE creation = ?1 AND state = 'open'", [c.id], |r| r.get(0)).unwrap_or(0);
        drop(db);
        let starred = p.me.as_ref().is_some_and(|m| app.starred(m, &c));
        (versions, parent, open, starred)
    };
    let boards = app.boards_for(&c.code).unwrap_or_default();
    let in_jams = app.jams_of(&c).unwrap_or_default();
    let can_enter = app.current_jam().ok().flatten().filter(|_| mine).filter(|j| !in_jams.iter().any(|i| i.id == j.id) && c.visibility == "everyone" && matches!(c.listing.as_str(), "listed" | "waiting") && c.base.is_none());
    let shown = q.v.and_then(|n| versions.iter().find(|v| v.n == n && (mine || v.state != "held"))).or_else(|| if mine { versions.first() } else { versions.iter().find(|v| v.state != "held") });
    let Some(shown) = shown.cloned() else { return error_page(&app, &p, 404, "This creation has no version you can see yet.") };
    let manifest: Option<Manifest> = serde_json::from_str(&shown.manifest).ok();
    let checks: Vec<Check> = serde_json::from_str(&shown.checks).unwrap_or_default();
    let tab = match q.tab.as_str() {
        "versions" | "remixes" | "suggestions" => q.tab.as_str(),
        _ => "code",
    };
    let files = match app.version_files(&c, shown.n).await {
        Ok(f) => f,
        Err(e) => return fail_page(&app, &p, e),
    };
    let toml: crate::checks::Toml = files.get("potluck.toml").and_then(|t| toml::from_str(t).ok()).unwrap_or_default();

    let body_tab = match tab {
        "versions" => versions_tab(&c, &versions, mine),
        "remixes" => match app.family(&c) {
            Ok((line, tree)) => family_tab(&c, &line, &tree, &p.me),
            Err(e) => return fail_page(&app, &p, e),
        },
        "suggestions" => {
            let list = app.suggestions_for(c.id, false).unwrap_or_default();
            suggestions_tab(&app, &c, &list)
        }
        _ => code_tab(&c, &files, &q.file, shown.n),
    };
    let tab_link = |t: &str| format!("/c/{}?tab={t}", c.code);
    let clone = format!("git clone {}/git/{}.git", app.cfg.public_url, c.path());
    let body = html! {
        @if q.done == "remixed" { div.note.ok { "Remixed. Load it in Potluck with its code to change it." } }
        @if q.done == "reported" { div.note.ok { "Thanks. A moderator will look at it." } }
        @if q.done == "merged" { div.note.ok { "Merged: that change is the newest version now." } }
        (listing_note(&c, versions.first().map(|v| v.state.as_str()).unwrap_or(""), mine))
        div.head {
            div {
                span.kind { (c.kind) }
                h1 { (c.name) }
                p.sub {
                    (head(&c.handle, 20)) "by " a href={ "/@" (c.handle) } { (c.maker) }
                    @if let Some(pa) = &parent {
                        " · a remix of " a href={ "/c/" (pa.code) } { (pa.name) } " by " (pa.maker)
                    }
                    " · version " (shown.n) @if shown.n != c.latest { " of " (c.latest) }
                    @if c.all_ages { " · " span.tag.ok { "Checked for all ages" } }
                    @for j in &in_jams { " · " a.tag href={ "/jams/" (j.slug) } { "In the " (j.theme) " jam" } }
                }
                @if let Some(j) = &can_enter {
                    form.inline method="post" action={ "/jams/" (j.slug) "/enter" } {
                        input type="hidden" name="code" value=(c.code);
                        button.btn type="submit" { "Enter it in this week's jam: " (j.theme) }
                    }
                }
                @if p.me.as_ref().is_some_and(|m| m.moderator) && c.latest > 0 {
                    form.inline method="post" action={ "/c/" (c.code) "/all-ages" } {
                        button.btn type="submit" name="on" value=(if c.all_ages { "0" } else { "1" }) {
                            @if c.all_ages { "Not for all ages" } @else { "Checked for all ages" }
                        }
                    }
                }
            }
            div.acts {
                span.bigcode title="Type this in Potluck: Load a creation from a code" { (pretty(&c.code)) }
                a.btn.pri href={ "potluck://c/" (c.code) } { "Open in Potluck" }
                @if p.me.is_some() && c.visible_to_all() {
                    form.inline method="post" action={ "/c/" (c.code) "/star" } {
                        button.btn type="submit" name="on" value=(if starred { "0" } else { "1" }) { @if starred { "Starred" } @else { "Star" } " · " (c.stars) }
                    }
                    @if !mine {
                        form.inline method="post" action={ "/c/" (c.code) "/remix" } { button.btn type="submit" { "Remix" } }
                    }
                } @else if p.me.is_none() {
                    a.btn href="/signin" { "Star · " (c.stars) }
                }
            }
        }
        @if !c.about.is_empty() { p { (c.about) } }
        nav.tabs {
            a.on[tab == "code"] href=(tab_link("code")) { "Code" }
            a.on[tab == "versions"] href=(tab_link("versions")) { "Versions" i { (versions.iter().filter(|v| mine || v.state != "held").count()) } }
            a.on[tab == "remixes"] href=(tab_link("remixes")) { "Remixes" i { (c.remixes) } }
            a.on[tab == "suggestions"] href=(tab_link("suggestions")) { "Suggested changes" @if suggestions_open > 0 { i { (suggestions_open) } } }
        }
        div.two {
            div { (body_tab) }
            aside {
                div.box {
                    h4 { "About it" }
                    div.kv {
                        span { "Code" } span.mono { (pretty(&c.code)) " " a.copy href="#" data-copy=(pretty(&c.code)) { "copy" } }
                        span { "Made by" } a href={ "/@" (c.handle) } { (c.maker) }
                        span { "Needs" } span { @if toml.needs.is_empty() { "nothing special" } @else { @for (i, (m, v)) in toml.needs.iter().enumerate() { @if i > 0 { ", " } "@potluck/" (m) " " (v) } } }
                        span { "Licence" } span { (c.licence) }
                        span { "Shared" } span { (ago(c.created)) }
                        span { "Plays" } span { (c.plays) }
                        @if let Some(m) = &manifest { span { "Commit" } span.mono { (&m.commit[..m.commit.len().min(10)]) } }
                    }
                }
                (contents(&toml, &files))
                div.box {
                    h4 { "Checks for version " (shown.n) }
                    (checks_list(&checks))
                }
                @for b in &boards {
                    div.box {
                        h4 { (b.name) }
                        ol.board {
                            @for s in &b.scores {
                                li { b { (s.name) } " " span.score { (number(s.value)) } span.sub { " on " (s.server) } }
                            }
                        }
                    }
                }
                @if c.visible_to_all() {
                    div.box {
                        h4 { "Get the code" }
                        p.mono style="font-size:13px" { (clone) }
                        a.btn.copy href="#" data-copy=(clone) { "Copy" }
                    }
                }
                @if p.me.is_some() && !mine && c.visible_to_all() {
                    details.box {
                        summary { b { "Report it" } }
                        form.stack method="post" action={ "/c/" (c.code) "/report" } {
                            select name="reason" required {
                                option value="unkind" { "It's unkind or hateful" }
                                option value="unsafe" { "It's not safe for young players" }
                                option value="copied" { "It's copied from someone without credit" }
                                option value="broken" { "It's broken or tries to trick people" }
                                option value="other" { "Something else" }
                            }
                            textarea name="note" rows="3" maxlength="500" placeholder="Anything a moderator should know (optional)" {}
                            button.btn.danger type="submit" { "Send the report" }
                        }
                    }
                }
                @if p.me.as_ref().is_some_and(|m| m.moderator) {
                    div.box { a href="/review" { "Moderators: the review queue" } }
                }
            }
        }
    };
    layout(&app, &p, &c.name, "", body).into_response()
}

/// What a creation brings besides its code: its buttons, its own sounds,
/// its Blockbench models, a client half and its tests. Nothing when it has
/// none of them.
fn contents(toml: &crate::checks::Toml, files: &BTreeMap<String, String>) -> Markup {
    let models: Vec<(&String, usize)> = files.iter().filter(|(n, _)| n.ends_with(".bbmodel")).map(|(n, t)| (n, crate::checks::model_cubes(t).unwrap_or(0))).collect();
    let tests = files.contains_key("test.luau");
    if toml.actions.is_empty() && toml.sounds.is_empty() && models.is_empty() && toml.client.is_none() && !tests {
        return html! {};
    }
    html! {
        div.box {
            h4 { "What's in it" }
            ul.inlist {
                @for a in toml.actions.values() {
                    li { "A button, " b { (a.name) } span.sub { " · " @if a.pad.is_empty() { "no pad button" } @else { (a.pad) " on a pad" } @if !a.keys.is_empty() { ", " (a.keys) " on a keyboard" } } }
                }
                @for (name, v) in &toml.sounds {
                    li { "A sound of its own, " b { (name) } @if let Some(c) = v.get("caption").and_then(|c| c.as_str()) { span.sub { " · \u{201c}" (c) "\u{201d}" } } }
                }
                @for (file, cubes) in &models {
                    li { "A Blockbench model, " b { (file) } span.sub { " · " (count(*cubes as i64, "cube")) } }
                }
                @if let Some(client) = &toml.client {
                    li { "A client half, " b { (client) } span.sub { " · runs on each player's own game" } }
                }
                @if tests { li { "Its own tests, " b { "test.luau" } } }
            }
        }
    }
}

/// A Blockbench model on its page: its texture as a picture and its cubes,
/// rather than the file's numbers.
fn model_view(path: &str, text: &str) -> Markup {
    let v: serde_json::Value = serde_json::from_str(text).unwrap_or_default();
    let cubes: Vec<&str> = v.get("elements").and_then(|e| e.as_array()).into_iter().flatten().filter(|e| e.get("type").and_then(|t| t.as_str()).unwrap_or("cube") == "cube").filter_map(|e| e.get("name").and_then(|n| n.as_str())).collect();
    // only a PNG kept in the file, in plain base64: nothing else reaches the page
    let pictures: Vec<&str> = v
        .get("textures")
        .and_then(|t| t.as_array())
        .into_iter()
        .flatten()
        .filter_map(|t| t.get("source").and_then(|s| s.as_str()))
        .filter(|s| s.strip_prefix("data:image/png;base64,").is_some_and(|b| b.bytes().all(|c| c.is_ascii_alphanumeric() || b"+/=".contains(&c))))
        .collect();
    html! {
        div.model {
            p { "A Blockbench model: open " b { (path) } " in " a href="https://www.blockbench.net" { "Blockbench" } " to change it. Each cube is a part of a creature's body, 16 pixels to a block." }
            @for src in &pictures { img.texture src=(src) alt={ "The picture painted on " (path) }; }
            @if !cubes.is_empty() {
                p.sub { (count(cubes.len() as i64, "cube")) ": " (cubes.join(", ")) }
            }
        }
    }
}

fn code_tab(c: &Creation, files: &BTreeMap<String, String>, want: &str, n: i64) -> Markup {
    let main = files.get("potluck.toml").and_then(|t| toml::from_str::<crate::checks::Toml>(t).ok()).map(|t| t.main).filter(|m| !m.is_empty()).unwrap_or_else(|| "main.luau".into());
    let path = if files.contains_key(want) { want.to_string() } else if files.contains_key(&main) { main } else { files.keys().next().cloned().unwrap_or_default() };
    let text = files.get(&path).cloned().unwrap_or_default();
    html! {
        div.files {
            @for f in files.keys() {
                a.on[*f == path] href={ "/c/" (c.code) "?file=" (f) "&v=" (n) } { (f) }
            }
        }
        @if path.ends_with(".bbmodel") { (model_view(&path, &text)) } @else { div.codewrap { (highlight::code(&path, &text)) } }
    }
}

fn state_tag(state: &str) -> Markup {
    html! {
        @match state {
            "held" => span.tag.bad { "held back by a check" },
            "waiting" => span.tag.warn { "a moderator will look" },
            _ => {}
        }
    }
}

fn versions_tab(c: &Creation, versions: &[db::Version], mine: bool) -> Markup {
    html! {
        ul.rows {
            @for v in versions.iter().filter(|v| mine || v.state != "held") {
                li {
                    b { "Version " (v.n) }
                    span { (v.note) }
                    (state_tag(&v.state))
                    @if v.n > 1 { a href={ "/c/" (c.code) "/v/" (v.n) } { "what changed" } }
                    a href={ "/c/" (c.code) "?v=" (v.n) } { "code" }
                    span.mono.when { (&v.git_commit[..v.git_commit.len().min(10)]) " · " (ago(v.created)) }
                }
            }
        }
    }
}

fn family_tab(c: &Creation, line: &[Creation], tree: &[(Creation, usize)], me: &Option<Account>) -> Markup {
    let on_line: Vec<i64> = line.iter().map(|x| x.id).collect();
    let mut private = 0;
    html! {
        @if line.len() > 1 {
            div.line {
                @for (i, x) in line.iter().enumerate() {
                    @if i > 0 { span.arrow { "\u{2192}" } }
                    @if x.id == c.id { span.here { (x.name) } } @else { a href={ "/c/" (x.code) } { (x.name) } }
                    span.by { " by " (x.maker) }
                }
            }
        }
        h2 { "The family tree" }
        ul.tree {
            @for (x, d) in tree {
                @let visible = x.listing == "listed" || on_line.contains(&x.id) || me.as_ref().is_some_and(|m| m.user_id == x.owner);
                @if visible && !matches!(x.listing.as_str(), "removed" | "hidden") {
                    li.root[*d == 0] style={ "--d:" (d) } {
                        @if x.id == c.id { span.here { (x.name) } } @else { a href={ "/c/" (x.code) } { b { (x.name) } } }
                        " by " (x.maker)
                        " " span.codechip { (pretty(&x.code)) }
                        @if x.remixes > 0 { " · " (count(x.remixes, "remix")) }
                    }
                } @else {
                    @let _ = { private += 1; };
                }
            }
        }
        @if private == 1 { p.sub { "One more remix isn't on the market: it's shared by its code, or waiting for a moderator." } }
        @if private > 1 { p.sub { (private) " more remixes aren't on the market: they're shared by their codes, or waiting for a moderator." } }
    }
}

fn suggestions_tab(app: &App, c: &Creation, list: &[crate::market::Suggestion]) -> Markup {
    let db = app.db();
    html! {
        @if list.is_empty() {
            div.empty { "No suggested changes yet. Remix it, change something, and suggest it back from Look inside in the game." }
        } @else {
            ul.rows {
                @for s in list {
                    @let by = db::account(&db, &s.by_user).ok().flatten().map(|a| a.name).unwrap_or_default();
                    li {
                        a href={ "/c/" (c.code) "/s/" (s.id) } { b { (s.title) } }
                        span { "from " (by) }
                        @match s.state.as_str() {
                            "open" => span.tag.warn { "open" },
                            "merged" => span.tag.ok { "merged into version " (s.merged_version.unwrap_or(0)) },
                            _ => span.tag { "closed" },
                        }
                        span.when { (ago(s.created)) }
                    }
                }
            }
        }
    }
}

async fn version(State(app): State<Shared>, h: HeaderMap, Path((code, n)): Path<(String, i64)>) -> Response {
    let p = page(&app, &h);
    let Ok(Some(c)) = db::creation_by_code(&app.db(), &codes::clean(&code)) else { return error_page(&app, &p, 404, "No creation has that code.") };
    let mine = p.me.as_ref().is_some_and(|m| m.user_id == c.owner || m.moderator);
    if !can_see(&c, &p.me) {
        return error_page(&app, &p, 451, "That creation is hidden.");
    }
    let (v, prev) = {
        let db = app.db();
        (db::version(&db, c.id, n).ok().flatten(), db::version(&db, c.id, n - 1).ok().flatten())
    };
    let Some(v) = v.filter(|v| mine || v.state != "held") else { return error_page(&app, &p, 404, "There's no such version.") };
    let new = app.version_files(&c, n).await.unwrap_or_default();
    let old = match &prev {
        Some(pv) => app.version_files(&c, pv.n).await.unwrap_or_default(),
        None => BTreeMap::new(),
    };
    let diff = files_diff(&old, &new);
    let body = html! {
        p.sub { a href={ "/c/" (c.code) "?tab=versions" } { "\u{2190} " (c.name) } }
        h1 { "Version " (n) }
        p.sub { (v.note) " · " (ago(v.created)) " " (state_tag(&v.state)) }
        @if diff.is_empty() { div.empty { "No files changed." } } @else { (highlight::diff(&diff)) }
    };
    layout(&app, &p, &format!("{} version {n}", c.name), "", body).into_response()
}

/// Every changed file between two versions as one unified diff.
pub fn files_diff(old: &BTreeMap<String, String>, new: &BTreeMap<String, String>) -> String {
    let mut out = String::new();
    let mut paths: Vec<&String> = old.keys().chain(new.keys()).collect();
    paths.sort();
    paths.dedup();
    for p in paths {
        let a = old.get(p).map(|s| s.as_str()).unwrap_or("");
        let b = new.get(p).map(|s| s.as_str()).unwrap_or("");
        if a != b {
            out.push_str(&highlight::diff_text(p, a, b));
        }
    }
    out
}

async fn suggestion(State(app): State<Shared>, h: HeaderMap, Path((code, id)): Path<(String, i64)>) -> Response {
    let p = page(&app, &h);
    let s = match app.suggestion(id) {
        Ok(s) => s,
        Err(e) => return fail_page(&app, &p, e),
    };
    let (c, from, by) = {
        let db = app.db();
        (db::creation(&db, s.creation).ok().flatten(), db::creation(&db, s.from_creation).ok().flatten(), db::account(&db, &s.by_user).ok().flatten())
    };
    let Some(c) = c.filter(|c| c.code == codes::clean(&code) && can_see(c, &p.me)) else { return error_page(&app, &p, 404, "There's no such suggestion.") };
    let diff = match s.state.as_str() {
        "open" => app.suggestion_diff(&s).await.unwrap_or_default(),
        "merged" => {
            let n = s.merged_version.unwrap_or(c.latest);
            let new = app.version_files(&c, n).await.unwrap_or_default();
            let old = app.version_files(&c, n - 1).await.unwrap_or_default();
            files_diff(&old, &new)
        }
        _ => String::new(),
    };
    let mine = p.me.as_ref().is_some_and(|m| m.user_id == c.owner);
    let theirs = p.me.as_ref().is_some_and(|m| m.user_id == s.by_user);
    let (add, del) = crate::api::count_diff(&diff);
    let body = html! {
        p.sub { a href={ "/c/" (c.code) "?tab=suggestions" } { "\u{2190} " (c.name) } }
        h1 { (s.title) }
        p.sub {
            "Suggested by " (by.map(|b| b.name).unwrap_or_default())
            @if let Some(f) = &from { " from " a href={ "/c/" (f.code) } { (f.name) } " (version " (s.from_version) ")" }
            " · " (ago(s.created)) " · " span.tag.ok { "+" (add) } " " span.tag.bad { "\u{2212}" (del) }
        }
        @if !s.body.is_empty() { p { (s.body) } }
        @match s.state.as_str() {
            "merged" => div.note.ok { "Merged into version " (s.merged_version.unwrap_or(0)) "." },
            "closed" => div.note.warn { "Closed without merging." },
            _ => {
                @if mine {
                    div.note.warn {
                        "Try it in Potluck before you merge: Look inside " (c.name) ", then Suggested changes. "
                        form.inline method="post" action={ "/c/" (c.code) "/s/" (s.id) "/merge" } { button.btn.pri type="submit" { "Merge it" } }
                        " "
                        form.inline method="post" action={ "/c/" (c.code) "/s/" (s.id) "/close" } { button.btn type="submit" { "Not this one" } }
                    }
                } @else if theirs {
                    form.inline method="post" action={ "/c/" (c.code) "/s/" (s.id) "/close" } { button.btn type="submit" { "Take it back" } }
                }
            }
        }
        @if diff.is_empty() { div.empty { "No change to show." } } @else { (highlight::diff(&diff)) }
    };
    layout(&app, &p, &s.title, "", body).into_response()
}

// ---------- actions ----------

fn need_me(app: &App, p: &Page) -> Result<Account, Response> {
    p.me.clone().ok_or_else(|| Redirect::to("/signin").into_response()).map_err(|r| {
        let _ = app;
        r
    })
}

#[derive(Deserialize)]
struct StarForm {
    on: String,
}

async fn star(State(app): State<Shared>, h: HeaderMap, Path(code): Path<String>, Form(f): Form<StarForm>) -> Response {
    let p = page(&app, &h);
    if let Err(r) = same_site(&app, &h) {
        return r;
    }
    let me = match need_me(&app, &p) {
        Ok(m) => m,
        Err(r) => return r,
    };
    match app.star(&me, &code, f.on == "1") {
        Ok(_) => Redirect::to(&format!("/c/{}", codes::clean(&code))).into_response(),
        Err(e) => fail_page(&app, &p, e),
    }
}

/// A moderator marks a creation's newest version checked for all ages, or not.
async fn all_ages(State(app): State<Shared>, h: HeaderMap, Path(code): Path<String>, Form(f): Form<StarForm>) -> Response {
    let p = page(&app, &h);
    if let Err(r) = same_site(&app, &h) {
        return r;
    }
    let me = match need_me(&app, &p) {
        Ok(m) => m,
        Err(r) => return r,
    };
    match app.set_all_ages(&me, &code, f.on == "1") {
        Ok(()) => Redirect::to(&format!("/c/{}", codes::clean(&code))).into_response(),
        Err(e) => fail_page(&app, &p, e),
    }
}

async fn remix(State(app): State<Shared>, h: HeaderMap, Path(code): Path<String>) -> Response {
    let p = page(&app, &h);
    if let Err(r) = same_site(&app, &h) {
        return r;
    }
    let me = match need_me(&app, &p) {
        Ok(m) => m,
        Err(r) => return r,
    };
    match app.remix_on_web(&me, &code).await {
        Ok(s) => Redirect::to(&format!("/c/{}?done=remixed", s.code)).into_response(),
        Err(e) => fail_page(&app, &p, e),
    }
}

#[derive(Deserialize)]
struct ReportForm {
    reason: String,
    #[serde(default)]
    note: String,
}

async fn report(State(app): State<Shared>, h: HeaderMap, Path(code): Path<String>, Form(f): Form<ReportForm>) -> Response {
    let p = page(&app, &h);
    if let Err(r) = same_site(&app, &h) {
        return r;
    }
    let me = match need_me(&app, &p) {
        Ok(m) => m,
        Err(r) => return r,
    };
    match app.report(&me, &code, &f.reason, &f.note) {
        Ok(()) => Redirect::to(&format!("/c/{}?done=reported", codes::clean(&code))).into_response(),
        Err(e) => fail_page(&app, &p, e),
    }
}

async fn merge(State(app): State<Shared>, h: HeaderMap, Path((code, id)): Path<(String, i64)>) -> Response {
    let p = page(&app, &h);
    if let Err(r) = same_site(&app, &h) {
        return r;
    }
    let me = match need_me(&app, &p) {
        Ok(m) => m,
        Err(r) => return r,
    };
    match app.merge(&me, id).await {
        Ok(_) => Redirect::to(&format!("/c/{}?done=merged", codes::clean(&code))).into_response(),
        Err(e) => fail_page(&app, &p, e),
    }
}

async fn close(State(app): State<Shared>, h: HeaderMap, Path((code, id)): Path<(String, i64)>) -> Response {
    let p = page(&app, &h);
    if let Err(r) = same_site(&app, &h) {
        return r;
    }
    let me = match need_me(&app, &p) {
        Ok(m) => m,
        Err(r) => return r,
    };
    match app.close_suggestion(&me, id).await {
        Ok(()) => Redirect::to(&format!("/c/{}?tab=suggestions", codes::clean(&code))).into_response(),
        Err(e) => fail_page(&app, &p, e),
    }
}

// ---------- makers and signing in ----------

async fn maker(State(app): State<Shared>, h: HeaderMap, Path(who): Path<String>) -> Response {
    let p = page(&app, &h);
    let Some(handle) = who.strip_prefix('@') else { return error_page(&app, &p, 404, "There's no page here.") };
    let Ok(Some(a)) = db::account_by_handle(&app.db(), handle) else { return error_page(&app, &p, 404, "There's no maker with that name.") };
    let me_too = p.me.as_ref().is_some_and(|m| m.user_id == a.user_id);
    let list = db::creations_where(&app.db(), "WHERE c.owner = ?1 AND c.listing = 'listed' ORDER BY c.updated DESC", &[&a.user_id]).unwrap_or_default();
    let body = html! {
        div.maker {
            (head(&a.handle, 72))
            div {
                h1 { (a.name) }
                p.sub { "@" (a.handle) " · making things since " (ago(a.created)) @if me_too { " · " a href="/me/profile" { "edit your profile" } } }
                @if !a.about.is_empty() { p.about { (a.about) } }
            }
        }
        @if list.is_empty() { div.empty { "Nothing on the market yet." } } @else { div.grid { @for c in &list { (card(c)) } } }
    };
    layout(&app, &p, &a.name, "", body).into_response()
}

async fn signin_help(State(app): State<Shared>, h: HeaderMap) -> Response {
    let p = page(&app, &h);
    let body = html! {
        div.box style="max-width:620px" {
            h1 { "Sign in from Potluck" }
            p { "There's no password to remember: your account is the one the game made for you. In Potluck, pause and choose " b { "Open the market" } ". Your browser opens here, signed in." }
            p { "Playing on a controller? Choose " b { "Your profile" } " in the game's settings and point your phone's camera at the code it shows." }
            p.sub { "You don't need to sign in to look around, play or read code. Signing in is for stars, remixes and reports." }
        }
    };
    layout(&app, &p, "Sign in", "", body).into_response()
}

async fn signin(State(app): State<Shared>, h: HeaderMap, Path(ticket): Path<String>) -> Response {
    let p = page(&app, &h);
    match app.use_ticket(&ticket) {
        Ok((session, next)) => {
            let secure = if app.cfg.public_url.starts_with("https://") { "; Secure" } else { "" };
            let cookie = format!("{COOKIE}={session}; Path=/; Max-Age={}; HttpOnly; SameSite=Lax{secure}", 30 * 86400);
            let next = crate::market::local_path(&next).unwrap_or("/me");
            ([(header::SET_COOKIE, cookie)], Redirect::to(next)).into_response()
        }
        Err(e) => fail_page(&app, &p, e),
    }
}

async fn signout(State(app): State<Shared>, h: HeaderMap) -> Response {
    if let Err(r) = same_site(&app, &h) {
        return r;
    }
    if let Some(s) = session(&h) {
        app.sign_out(&s);
    }
    ([(header::SET_COOKIE, format!("{COOKIE}=; Path=/; Max-Age=0; HttpOnly; SameSite=Lax"))], Redirect::to("/")).into_response()
}

#[derive(Deserialize, Default)]
struct MeQ {
    #[serde(default)]
    done: String,
}

async fn me(State(app): State<Shared>, h: HeaderMap, Query(q): Query<MeQ>) -> Response {
    let p = page(&app, &h);
    let me = match need_me(&app, &p) {
        Ok(m) => m,
        Err(r) => return r,
    };
    let list = db::creations_where(&app.db(), "WHERE c.owner = ?1 AND c.listing != 'removed' ORDER BY c.updated DESC", &[&me.user_id]).unwrap_or_default();
    let in_beta = me.beta || !app.cfg.invite_only;
    let body = html! {
        @if q.done == "beta" { div.note.ok { "You're in the beta: you can share to Everyone from the game." } }
        @if q.done == "profile" { div.note.ok { "Saved. Everyone sees your new profile now." } }
        div.head {
            div.maker {
                (head(&me.handle, 64))
                div {
                    h1 { (me.name) }
                    p.sub { "@" (me.handle) " · " a href={ "/@" (me.handle) } { "your public page" } }
                    @if !me.about.is_empty() { p.about { (me.about) } }
                }
            }
            div.acts {
                a.btn.pri href="/me/profile" { "Edit your profile" }
                a.btn href="/me/devices" { "Your devices" }
                a.btn href="/me/servers" { "Your servers" }
                form.inline method="post" action="/signout" { button.btn type="submit" { "Sign out" } }
            }
        }
        @if !in_beta {
            div.box style="max-width:520px" {
                h4 { "Join the closed beta" }
                p.sub { "Anyone with a code can load what you share. Sharing to Everyone, on the market, needs an invite for now." }
                form.stack method="post" action="/me/invite" {
                    input name="code" placeholder="Invite code" required maxlength="16" autocomplete="off";
                    button.btn.pri type="submit" { "Join" }
                }
            }
        }
        h2 { "What you've shared" }
        @if list.is_empty() {
            div.empty { "Nothing yet. In Potluck, Look inside something you made and choose Share." }
        } @else {
            ul.rows {
                @for c in &list {
                    li {
                        a href={ "/c/" (c.code) } { b { (c.name) } }
                        span.codechip { (pretty(&c.code)) }
                        span.tag { (c.listing) }
                        span.when { "version " (c.latest) " · " (ago(c.updated)) }
                    }
                }
            }
        }
    };
    layout(&app, &p, "You", "", body).into_response()
}

#[derive(Deserialize)]
struct InviteForm {
    code: String,
}

async fn invite(State(app): State<Shared>, h: HeaderMap, Form(f): Form<InviteForm>) -> Response {
    let p = page(&app, &h);
    if let Err(r) = same_site(&app, &h) {
        return r;
    }
    let me = match need_me(&app, &p) {
        Ok(m) => m,
        Err(r) => return r,
    };
    match app.redeem_invite(&me, &f.code) {
        Ok(()) => Redirect::to("/me?done=beta").into_response(),
        Err(e) => fail_page(&app, &p, e),
    }
}

// ---------- the profile ----------

/// The profile page: a name, a line about you and a block head, picked from
/// a few choices so it works with a thumb on a phone and needs no upload.
async fn profile(State(app): State<Shared>, h: HeaderMap) -> Response {
    let p = page(&app, &h);
    let me = match need_me(&app, &p) {
        Ok(m) => m,
        Err(r) => return r,
    };
    let a = Avatar::parse(&me.avatar, &me.handle);
    let body = html! {
        form.profile method="post" action="/me/profile" {
            div.head {
                h1 { "Your profile" }
                div.acts { a.btn href="/me" { "Cancel" } button.btn.pri type="submit" { "Save" } }
            }
            div.two {
                div {
                    div.box {
                        label.field { span { "Name" } input name="name" value=(me.name) required maxlength="32" autocomplete="nickname"; }
                        p.sub.hint { "Everyone sees this beside what you share. Your handle stays @" (me.handle) "." }
                        @if !me.young {
                            label.field { span { "About you" } textarea name="about" maxlength="280" rows="3" placeholder="What do you like making?" { (me.about) } }
                        }
                    }
                    div.box {
                        h4 { "Head colour" }
                        div.swatches { @for c in avatar::COLOURS { label.sw style={ "--c:#" (c) } { input type="radio" name="head" value=(c) checked[a.head == *c] aria-label={ "#" (c) }; span {} } } }
                        h4 { "Eyes" }
                        div.picks { @for e in avatar::EYES { label.pick { input type="radio" name="eyes" value=(e) checked[a.eyes == *e]; span { (e) } } } }
                        h4 { "On top" }
                        div.picks { @for t in avatar::TOPS { label.pick { input type="radio" name="top" value=(t) checked[a.top == *t]; span { (t) } } } }
                        h4 { "Its colour" }
                        div.swatches { @for c in avatar::COLOURS { label.sw style={ "--c:#" (c) } { input type="radio" name="top_colour" value=(c) checked[a.top_colour == *c] aria-label={ "#" (c) }; span {} } } }
                    }
                }
                aside {
                    div.box.preview {
                        img #face-preview src={ "/avatar/" (me.handle) ".svg?look=" (a.encode()) } width="160" height="160" alt="Your block head";
                        p.sub { "Your block head, on the market and in friends' worlds." }
                    }
                }
            }
        }
    };
    layout(&app, &p, "Your profile", "", body).into_response()
}

#[derive(Deserialize)]
struct ProfileForm {
    name: String,
    #[serde(default)]
    about: String,
    #[serde(default)]
    head: String,
    #[serde(default)]
    eyes: String,
    #[serde(default)]
    top: String,
    #[serde(default)]
    top_colour: String,
}

async fn save_profile(State(app): State<Shared>, h: HeaderMap, Form(f): Form<ProfileForm>) -> Response {
    let p = page(&app, &h);
    if let Err(r) = same_site(&app, &h) {
        return r;
    }
    let me = match need_me(&app, &p) {
        Ok(m) => m,
        Err(r) => return r,
    };
    let avatar = Avatar::from_choices(&f.head, &f.eyes, &f.top, &f.top_colour);
    match app.update_profile(&me, &f.name, &f.about, avatar) {
        Ok(_) => Redirect::to("/me?done=profile").into_response(),
        Err(e) => fail_page(&app, &p, e),
    }
}

#[derive(Deserialize, Default)]
struct AvatarQ {
    #[serde(default)]
    look: String,
}

/// A maker's block head as SVG, by handle: `/avatar/jo.svg`. With `?look=`
/// it draws those choices instead, for the profile page's preview.
async fn avatar(State(app): State<Shared>, Path(file): Path<String>, Query(q): Query<AvatarQ>) -> Response {
    let handle = file.strip_suffix(".svg").unwrap_or(&file);
    let saved = db::account_by_handle(&app.db(), handle).ok().flatten().map(|a| a.avatar).unwrap_or_default();
    let svg = Avatar::parse(if q.look.is_empty() { &saved } else { &q.look }, handle).svg();
    ([(header::CONTENT_TYPE, "image/svg+xml"), (header::CACHE_CONTROL, "public, max-age=300")], svg).into_response()
}

// ---------- the review queue ----------

fn moderator(app: &App, p: &Page) -> Result<Account, Response> {
    match &p.me {
        Some(m) if m.moderator => Ok(m.clone()),
        Some(_) => Err(error_page(app, p, 403, "The review queue is for moderators.")),
        None => Err(Redirect::to("/signin").into_response()),
    }
}

fn why_text(why: &str) -> &'static str {
    match why {
        "new_maker" => "a new maker's first share to Everyone",
        "update" => "a new version from a maker who isn't trusted yet",
        "reports" => "players reported it",
        "checks" => "a check wants a person to look",
        _ => "to look at",
    }
}

#[derive(Deserialize, Default)]
struct ReviewQ {
    #[serde(default)]
    invites: String,
}

async fn review(State(app): State<Shared>, h: HeaderMap, Query(q): Query<ReviewQ>) -> Response {
    let p = page(&app, &h);
    if let Err(r) = moderator(&app, &p) {
        return r;
    }
    let items: Vec<(i64, Creation, i64, String, String, i64)> = {
        let db = app.db();
        let mut st = match db.prepare("SELECT id, creation, version, why, detail, created FROM queue WHERE open = 1 ORDER BY CASE why WHEN 'reports' THEN 0 ELSE 1 END, created LIMIT 200") {
            Ok(s) => s,
            Err(e) => return fail_page(&app, &p, e.into()),
        };
        let rows: Vec<(i64, i64, i64, String, String, i64)> = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?))).map(|r| r.flatten().collect()).unwrap_or_default();
        rows.into_iter().filter_map(|(id, cid, v, why, detail, t)| db::creation(&db, cid).ok().flatten().map(|c| (id, c, v, why, detail, t))).collect()
    };
    let log: Vec<(i64, String, String, String, String)> = {
        let db = app.db();
        let mut st = db.prepare("SELECT l.at, COALESCE(a.handle, l.actor), l.action, l.target, l.note FROM log l LEFT JOIN accounts a ON a.user_id = l.actor WHERE l.action IN ('list','unlist','hide','remove','dismiss','hide','trusted=true','trusted=false','beta=true','banned=true','moderator=true') ORDER BY l.id DESC LIMIT 30").unwrap();
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))).map(|r| r.flatten().collect()).unwrap_or_default()
    };
    // One row per version, with every reason it's waiting.
    let mut groups: Vec<(i64, Creation, i64, Vec<(String, String)>, i64)> = Vec::new();
    for (id, c, v, why, detail, t) in items {
        match groups.iter_mut().find(|g| g.1.id == c.id && g.2 == v) {
            Some(g) => g.3.push((why, detail)),
            None => groups.push((id, c, v, vec![(why, detail)], t)),
        }
    }
    let body = html! {
        h1 { "Review queue" }
        p.sub { "Creations wait here before they're listed for Everyone, when players report them, and when a check wants a person to look. Reported ones come first." }
        @if !q.invites.is_empty() {
            div.note.ok { "New invite codes: " span.mono { (q.invites.replace(',', ", ")) } }
        }
        div.two {
            div {
                @if groups.is_empty() {
                    div.empty { "Nothing waiting. Nice." }
                } @else {
                    ul.rows {
                        @for (id, c, v, whys, t) in &groups {
                            li {
                                a href={ "/review/" (id) } { b { (c.name) } }
                                span { "by " (c.maker) " · version " (v) }
                                @for (why, _) in whys {
                                    span.tag.warn[why != "reports"].bad[why == "reports"] { (why_text(why)) }
                                }
                                @for (_, detail) in whys {
                                    @if !detail.is_empty() { span.sub { (detail) } }
                                }
                                span.when { (ago(*t)) }
                            }
                        }
                    }
                }
                h2 { "Lately" }
                @if log.is_empty() {
                    p.sub { "Nothing decided yet." }
                } @else {
                    ul.rows {
                        @for (t, who, action, target, note) in &log {
                            li { b { (action) } span { a href={ "/c/" (target) } { (target) } } span.sub { (note) } span.when { (who) " · " (ago(*t)) } }
                        }
                    }
                }
            }
            aside {
                div.box {
                    h4 { "Makers" }
                    form.stack method="post" action="/review/maker" {
                        input name="handle" placeholder="handle" required;
                        select name="flag" {
                            option value="beta" { "In the beta" }
                            option value="trusted" { "Trusted: their updates are listed without a look" }
                            option value="banned" { "Banned from sharing" }
                            option value="moderator" { "A moderator" }
                        }
                        select name="on" { option value="1" { "Yes" } option value="0" { "No" } }
                        button.btn type="submit" { "Change" }
                    }
                }
                div.box {
                    h4 { "Invites to the beta" }
                    form.stack method="post" action="/review/invites" {
                        input name="n" type="number" min="1" max="50" value="5";
                        button.btn type="submit" { "Make invite codes" }
                    }
                }
            }
        }
    };
    layout(&app, &p, "Review", "", body).into_response()
}

async fn review_item(State(app): State<Shared>, h: HeaderMap, Path(id): Path<i64>) -> Response {
    let p = page(&app, &h);
    if let Err(r) = moderator(&app, &p) {
        return r;
    }
    let row: Option<(i64, i64, String, String, i64)> = app.db().query_row("SELECT creation, version, why, detail, open FROM queue WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))).optional().ok().flatten();
    let Some((cid, n, why, detail, open)) = row else { return error_page(&app, &p, 404, "That's not in the queue.") };
    let Ok(Some(c)) = db::creation(&app.db(), cid) else { return error_page(&app, &p, 404, "Gone.") };
    // The other reasons this version is waiting, shown on the same page.
    let mut whys = vec![(why, detail)];
    if open == 1 {
        let db = app.db();
        if let Ok(mut st) = db.prepare("SELECT why, detail FROM queue WHERE creation = ?1 AND version = ?2 AND open = 1 AND id != ?3 ORDER BY id") {
            whys.extend(st.query_map(params![cid, n, id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))).map(|r| r.flatten().collect::<Vec<_>>()).unwrap_or_default());
        }
    }
    let (v, reports, maker, last_ok) = {
        let db = app.db();
        let v = db::version(&db, cid, n).ok().flatten();
        let mut st = db.prepare("SELECT a.handle, r.reason, r.note, r.created, r.open FROM reports r JOIN accounts a ON a.user_id = r.reporter WHERE r.creation = ?1 ORDER BY r.created DESC").unwrap();
        let reports: Vec<(String, String, String, i64, bool)> = st.query_map([cid], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))).map(|r| r.flatten().collect()).unwrap_or_default();
        let maker = db::account(&db, &c.owner).ok().flatten();
        let last_ok: Option<i64> = db.query_row("SELECT MAX(n) FROM versions WHERE creation = ?1 AND n < ?2 AND state = 'ok'", params![cid, n], |r| r.get(0)).ok().flatten();
        (v, reports, maker, last_ok)
    };
    let new = app.version_files(&c, n).await.unwrap_or_default();
    let old = match last_ok {
        Some(o) => app.version_files(&c, o).await.unwrap_or_default(),
        None => BTreeMap::new(),
    };
    let diff = files_diff(&old, &new);
    let checks: Vec<Check> = v.as_ref().and_then(|v| serde_json::from_str(&v.checks).ok()).unwrap_or_default();
    let body = html! {
        p.sub { a href="/review" { "\u{2190} Review queue" } }
        div.head {
            div {
                h1 { (c.name) " · version " (n) }
                p.sub { "by " a href={ "/@" (c.handle) } { (c.maker) } @for (why, detail) in &whys { " · " (why_text(why)) @if !detail.is_empty() { " (" (detail) ")" } } " · now " b { (c.listing) } }
            }
            div.acts { a.btn href={ "/c/" (c.code) "?v=" (n) } { "Its page" } }
        }
        @if open == 0 { div.note.ok { "Already decided." } }
        div.two {
            div {
                h2 { @if last_ok.is_some() { "What changed since version " (last_ok.unwrap_or(0)) ", the last one looked at" } @else { "Everything in it" } }
                @if diff.is_empty() { div.empty { "No files changed." } } @else { (highlight::diff(&diff)) }
            }
            aside {
                div.box {
                    h4 { "Decide" }
                    form.stack method="post" action={ "/review/" (id) } {
                        textarea name="note" rows="2" placeholder="A note for the log (optional)" {}
                        button.btn.pri type="submit" name="decision" value="list" { "Fine: list it" @if c.visibility != "everyone" { " (stays shared by code)" } }
                        button.btn type="submit" name="decision" value="list_all" { "Fine for all ages too: list it" }
                        button.btn type="submit" name="decision" value="unlist" { "Fine by code, not for the market" }
                        @if whys.iter().any(|(w, _)| w == "reports") { button.btn type="submit" name="decision" value="dismiss" { "The reports are wrong" } }
                        button.btn type="submit" name="decision" value="hide" { "Hide it for now" }
                        button.btn.danger type="submit" name="decision" value="remove" { "Take it down, and its files" }
                    }
                }
                div.box { h4 { "Checks" } (checks_list(&checks)) }
                @if let Some(m) = &maker {
                    div.box {
                        h4 { "The maker" }
                        div.kv {
                            span { "Handle" } span { "@" (m.handle) }
                            span { "Since" } span { (ago(m.created)) }
                            span { "Beta" } span { (if m.beta { "yes" } else { "no" }) }
                            span { "Trusted" } span { (if m.trusted { "yes" } else { "no" }) }
                        }
                        @if !m.trusted {
                            form.inline method="post" action="/review/maker" {
                                input type="hidden" name="handle" value=(m.handle);
                                input type="hidden" name="flag" value="trusted";
                                input type="hidden" name="on" value="1";
                                button.btn type="submit" { "Trust this maker" }
                            }
                        }
                    }
                }
                @if !reports.is_empty() {
                    div.box {
                        h4 { "Reports" }
                        ul.rows {
                            @for (who, reason, note, t, open) in &reports {
                                li { b { (reason) } span { (note) } span.when { (who) " · " (ago(*t)) @if !open { " · done" } } }
                            }
                        }
                    }
                }
            }
        }
    };
    layout(&app, &p, "Review", "", body).into_response()
}

#[derive(Deserialize)]
struct DecideForm {
    decision: String,
    #[serde(default)]
    note: String,
}

async fn decide(State(app): State<Shared>, h: HeaderMap, Path(id): Path<i64>, Form(f): Form<DecideForm>) -> Response {
    let p = page(&app, &h);
    if let Err(r) = same_site(&app, &h) {
        return r;
    }
    let m = match moderator(&app, &p) {
        Ok(m) => m,
        Err(r) => return r,
    };
    match app.decide(&m, id, &f.decision, &f.note) {
        Ok(()) => Redirect::to("/review").into_response(),
        Err(e) => fail_page(&app, &p, e),
    }
}

#[derive(Deserialize)]
struct FlagForm {
    handle: String,
    flag: String,
    on: String,
}

async fn maker_flag(State(app): State<Shared>, h: HeaderMap, Form(f): Form<FlagForm>) -> Response {
    let p = page(&app, &h);
    if let Err(r) = same_site(&app, &h) {
        return r;
    }
    let m = match moderator(&app, &p) {
        Ok(m) => m,
        Err(r) => return r,
    };
    match app.set_flag(&m, f.handle.trim().trim_start_matches('@'), &f.flag, f.on == "1") {
        Ok(()) => Redirect::to("/review").into_response(),
        Err(e) => fail_page(&app, &p, e),
    }
}

#[derive(Deserialize)]
struct InvitesForm {
    n: usize,
}

async fn make_invites(State(app): State<Shared>, h: HeaderMap, Form(f): Form<InvitesForm>) -> Response {
    let p = page(&app, &h);
    if let Err(r) = same_site(&app, &h) {
        return r;
    }
    let m = match moderator(&app, &p) {
        Ok(m) => m,
        Err(r) => return r,
    };
    match app.make_invites(&m.user_id, f.n.max(1)) {
        Ok(codes) => Redirect::to(&format!("/review?invites={}", codes.iter().map(|c| pretty(c)).collect::<Vec<_>>().join(","))).into_response(),
        Err(e) => fail_page(&app, &p, e),
    }
}

// ---------- git clone ----------

/// `git clone <market>/git/<maker>/<creation>.git`, for creations anyone
/// may load, read only.
async fn git(State(app): State<Shared>, method: Method, h: HeaderMap, Path((owner, repo, rest)): Path<(String, String, String)>, uri: axum::http::Uri, body: axum::body::Bytes) -> Response {
    let Some(slug) = repo.strip_suffix(".git") else { return StatusCode::NOT_FOUND.into_response() };
    let allowed = matches!((method.as_str(), rest.as_str()), ("GET", "info/refs") | ("POST", "git-upload-pack"));
    if !allowed || (rest == "info/refs" && uri.query() != Some("service=git-upload-pack")) {
        return (StatusCode::FORBIDDEN, "creations can be cloned here, not pushed").into_response();
    }
    let visible = {
        let db = app.db();
        db.query_row(
            "SELECT c.listing FROM creations c JOIN accounts a ON a.user_id = c.owner WHERE a.handle = ?1 AND c.slug = ?2",
            params![owner, slug],
            |r| r.get::<_, String>(0),
        )
        .optional()
        .ok()
        .flatten()
        .is_some_and(|l| matches!(l.as_str(), "listed" | "unlisted" | "waiting"))
    };
    if !visible {
        return (StatusCode::NOT_FOUND, "no such creation").into_response();
    }
    let pass: Vec<(&str, &str)> = ["content-type", "content-encoding", "git-protocol"].into_iter().filter_map(|k| h.get(k).and_then(|v| v.to_str().ok()).map(|v| (k, v))).collect();
    let b = if body.is_empty() { crate::forgejo::bytes_body::Body::Empty } else { crate::forgejo::bytes_body::Body::Bytes(body.to_vec()) };
    match app.git.git(&format!("{owner}/{slug}"), &rest, uri.query(), method, &pass, b).await {
        Ok(r) => {
            let status = StatusCode::from_u16(r.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
            let ctype = r.headers().get(reqwest::header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("application/octet-stream").to_string();
            let stream = r.bytes_stream();
            Response::builder().status(status).header(header::CONTENT_TYPE, ctype).header(header::CACHE_CONTROL, "no-cache").body(Body::from_stream(stream)).unwrap_or_else(|_| StatusCode::BAD_GATEWAY.into_response())
        }
        Err(e) => (StatusCode::NOT_FOUND, e.message).into_response(),
    }
}

// ---------- servers ----------

/// A score as people read it: whole numbers without the point.
fn number(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        format!("{v:.2}")
    }
}

fn join_text(j: &str) -> &'static str {
    match j {
        "signed-in" => "signed-in players",
        "list" => "invited players",
        _ => "anyone",
    }
}

async fn servers_page(State(app): State<Shared>, h: HeaderMap) -> Response {
    let p = page(&app, &h);
    let list = match app.listed_servers() {
        Ok(l) => l,
        Err(e) => return fail_page(&app, &p, e),
    };
    let moderator = p.me.as_ref().is_some_and(|m| m.moderator);
    let body = html! {
        h1 { "Servers" }
        p.sub { "Worlds that are always on. In Potluck, open Join a friend's world, then Servers on the market, or type an address." }
        @if list.is_empty() {
            div.empty { "No servers are listed right now." }
        } @else {
            ul.rows.servers {
                @for s in &list {
                    li {
                        b { (s.name) }
                        span { (s.players) " of " (s.max_players) " playing" }
                        span.mono { (s.address) " " a.copy href="#" data-copy=(s.address) { "copy" } }
                        @if !s.about.is_empty() { span.sub { (s.about) } }
                        @let names: Vec<&str> = s.creations.iter().filter_map(|c| c["name"].as_str()).collect();
                        @if !names.is_empty() { span.sub { "Runs " (names.join(", ")) } }
                        span.tag { "for " (join_text(&s.join)) }
                        @if s.all_ages { span.tag.ok { "all ages" } }
                        @if moderator {
                            form.inline method="post" action={ "/servers/" (s.id) "/hide" } { button.btn type="submit" { "Take off the list" } }
                        }
                    }
                }
            }
        }
        p.sub { "Running a server of your own? " a href="/me/servers" { "Add it to the list" } "." }
    };
    layout(&app, &p, "Servers", "", body).into_response()
}

async fn my_servers(State(app): State<Shared>, h: HeaderMap) -> Response {
    let p = page(&app, &h);
    let me = match need_me(&app, &p) {
        Ok(m) => m,
        Err(r) => return r,
    };
    my_servers_page(&app, &p, &me, None)
}

/// Your servers, with a new server's token when one was just made: it's
/// shown on this page only, never in an address.
fn my_servers_page(app: &App, p: &Page, me: &Account, new: Option<(i64, String)>) -> Response {
    let list = match app.my_servers(me) {
        Ok(l) => l,
        Err(e) => return fail_page(app, p, e),
    };
    let body = html! {
        h1 { "Your servers" }
        p.sub { "A server shows in the list while it's on and the market can reach it at the address it gives. Put its token in the server's server.toml, with list = true and that address." }
        @if let Some((_, token)) = &new {
            div.note.ok {
                "Your new server's token, shown only this once. Put it in its server.toml:"
                pre.mono { "list = true\naddress = \"wss://your.server.example\"\nmarket_token = \"" (token) "\"" }
            }
        }
        @if list.is_empty() {
            div.empty { "No servers yet." }
        } @else {
            ul.rows {
                @for s in &list {
                    li {
                        b { (s.name) }
                        @if s.address.is_empty() { span.sub { "hasn't been in touch yet" } } @else { span.mono { (s.address) } }
                        @if s.last_seen > 0 { span.sub { "last heard " (ago(s.last_seen)) } }
                        @if s.hidden { span.tag.bad { "taken off the list by a moderator" } }
                        @else if s.last_seen > 0 && !s.reachable { span.tag.warn { "the market couldn't reach its status page" } }
                        @else if s.reachable { span.tag { "listed" } }
                        form.inline method="post" action={ "/me/servers/" (s.id) "/remove" } { button.btn type="submit" { "Remove" } }
                    }
                }
            }
        }
        div.box style="max-width:520px" {
            h4 { "Add a server" }
            form.stack method="post" action="/me/servers" {
                input name="name" placeholder="Its name" required maxlength="40";
                button.btn.pri type="submit" { "Make a token" }
            }
        }
    };
    layout(app, p, "Your servers", "", body).into_response()
}

#[derive(Deserialize)]
struct AddServerForm {
    name: String,
}

async fn add_server(State(app): State<Shared>, h: HeaderMap, Form(f): Form<AddServerForm>) -> Response {
    let p = page(&app, &h);
    if let Err(r) = same_site(&app, &h) {
        return r;
    }
    let me = match need_me(&app, &p) {
        Ok(m) => m,
        Err(r) => return r,
    };
    match app.add_server(&me, &f.name) {
        Ok((id, token)) => my_servers_page(&app, &p, &me, Some((id, token))),
        Err(e) => fail_page(&app, &p, e),
    }
}

async fn remove_server(State(app): State<Shared>, h: HeaderMap, Path(id): Path<i64>) -> Response {
    let p = page(&app, &h);
    if let Err(r) = same_site(&app, &h) {
        return r;
    }
    let me = match need_me(&app, &p) {
        Ok(m) => m,
        Err(r) => return r,
    };
    match app.remove_server(&me, id) {
        Ok(()) => Redirect::to("/me/servers").into_response(),
        Err(e) => fail_page(&app, &p, e),
    }
}

async fn hide_server(State(app): State<Shared>, h: HeaderMap, Path(id): Path<i64>) -> Response {
    let p = page(&app, &h);
    if let Err(r) = same_site(&app, &h) {
        return r;
    }
    let me = match need_me(&app, &p) {
        Ok(m) => m,
        Err(r) => return r,
    };
    match app.hide_server(&me, id, true) {
        Ok(()) => Redirect::to("/servers").into_response(),
        Err(e) => fail_page(&app, &p, e),
    }
}

// ---------- devices ----------

/// What the devices page has just made, to show once.
enum Made {
    Nothing,
    Unlinked,
    LinkCode(String),
    Recovery(String),
}

async fn my_devices(State(app): State<Shared>, h: HeaderMap, Query(q): Query<MeQ>) -> Response {
    let p = page(&app, &h);
    let me = match need_me(&app, &p) {
        Ok(m) => m,
        Err(r) => return r,
    };
    devices_page(&app, &p, &me, if q.done == "unlinked" { Made::Unlinked } else { Made::Nothing })
}

/// Your devices: the first, the ones linked since, a code to link another
/// and the recovery code. New codes are shown on this page only.
fn devices_page(app: &App, p: &Page, me: &Account, made: Made) -> Response {
    let (list, recovery) = match (app.devices(me), app.recovery_made(me)) {
        (Ok(l), Ok(r)) => (l, r),
        (Err(e), _) | (_, Err(e)) => return fail_page(app, p, e),
    };
    let body = html! {
        h1 { "Your devices" }
        p.sub { "Play as you on every device: your name, your block head and what you've shared come along. Nobody needs an email or a password." }
        @match &made {
            Made::Unlinked => { div.note.ok { "Unlinked. That device starts an account of its own next time it plays." } }
            Made::LinkCode(code) => {
                div.note.ok {
                    p { "On the new device, in Potluck, choose Settings, then Link a device, and type:" }
                    p { span.bigcode { (pretty(code)) } }
                    p { "It works once, in the next ten minutes." }
                }
            }
            Made::Recovery(code) => {
                div.note.ok {
                    p { "Your recovery code, shown only this once. Write it down and keep it somewhere safe:" }
                    p { span.bigcode { (pretty(code)) } }
                    p { "If you lose every device, type it in Potluck under Settings, then Link a device. It works once, and making a new one stops this one working." }
                }
            }
            Made::Nothing => {}
        }
        ul.rows {
            li {
                b { "The first device" }
                span.sub { "started this account " (ago(me.created)) }
                span.tag { "always yours" }
            }
            @for d in &list {
                li {
                    b { @if d.label.is_empty() { "A device" } @else { (d.label) } }
                    span.sub { "linked " (ago(d.linked)) }
                    span.when { "last here " (ago(d.seen)) }
                    form.inline method="post" action={ "/me/devices/" (d.id) "/unlink" } { button.btn type="submit" { "Unlink" } }
                }
            }
        }
        div.two {
            div.box {
                h4 { "Link a device" }
                p.sub { "Make a code here, or in Potluck on a device that's already yours, and type it on the new one under Settings, then Link a device." }
                form method="post" action="/me/devices/code" { button.btn.pri type="submit" { "Make a code" } }
            }
            div.box {
                h4 { "Recovery code" }
                @match recovery {
                    Some(at) => p.sub { "You made one " (ago(at)) ". If every device is lost, it brings your account to a new one." },
                    None => p.sub { "If every device is lost, a recovery code brings your account to a new one. Make one and keep it somewhere safe." },
                }
                form method="post" action="/me/recovery" { button.btn type="submit" { @if recovery.is_some() { "Make a new one" } @else { "Make a recovery code" } } }
            }
        }
    };
    layout(app, p, "Your devices", "", body).into_response()
}

async fn device_code(State(app): State<Shared>, h: HeaderMap) -> Response {
    let p = page(&app, &h);
    if let Err(r) = same_site(&app, &h) {
        return r;
    }
    let me = match need_me(&app, &p) {
        Ok(m) => m,
        Err(r) => return r,
    };
    match app.link_code(&me) {
        Ok(code) => devices_page(&app, &p, &me, Made::LinkCode(code)),
        Err(e) => fail_page(&app, &p, e),
    }
}

async fn make_recovery(State(app): State<Shared>, h: HeaderMap) -> Response {
    let p = page(&app, &h);
    if let Err(r) = same_site(&app, &h) {
        return r;
    }
    let me = match need_me(&app, &p) {
        Ok(m) => m,
        Err(r) => return r,
    };
    match app.recovery_code(&me) {
        Ok(code) => devices_page(&app, &p, &me, Made::Recovery(code)),
        Err(e) => fail_page(&app, &p, e),
    }
}

async fn unlink_device(State(app): State<Shared>, h: HeaderMap, Path(id): Path<i64>) -> Response {
    let p = page(&app, &h);
    if let Err(r) = same_site(&app, &h) {
        return r;
    }
    let me = match need_me(&app, &p) {
        Ok(m) => m,
        Err(r) => return r,
    };
    match app.unlink(&me, id) {
        Ok(()) => Redirect::to("/me/devices?done=unlinked").into_response(),
        Err(e) => fail_page(&app, &p, e),
    }
}

// ---------- Jams ----------

fn entries_count(n: usize) -> String {
    if n == 1 { "1 entry".into() } else { format!("{n} entries") }
}

/// "in 3 days", "in 5 h", "in 20 min", or "now" once it's passed.
pub fn until(t: i64) -> String {
    let d = t - now();
    match d {
        i64::MIN..=0 => "now".into(),
        1..=3599 => format!("in {} min", (d + 59) / 60),
        3600..=86399 => format!("in {} h", d / 3600),
        _ => format!("in {} days", d / 86400),
    }
}

/// Days since 1970 for a calendar date (Howard Hinnant's days_from_civil).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// Midnight UTC of a YYYY-MM-DD date, as a form's date field sends it.
pub fn parse_day(s: &str) -> Option<i64> {
    let mut it = s.trim().split('-');
    let (y, m, d): (i64, i64, i64) = (it.next()?.parse().ok()?, it.next()?.parse().ok()?, it.next()?.parse().ok()?);
    if it.next().is_some() || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    Some(days_from_civil(y, m, d) * 86400)
}

/// The calendar day of a time, as YYYY-MM-DD (UTC).
pub fn day_of(t: i64) -> String {
    let z = t.div_euclid(86400) + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02}")
}

fn jam_dates(j: &crate::jams::Jam) -> String {
    if j.running() {
        format!("Runs until {} (ends {})", day_of(j.ends - 1), until(j.ends))
    } else if j.over() {
        format!("Ran {} to {}", day_of(j.starts), day_of(j.ends - 1))
    } else {
        format!("Starts {} ({})", day_of(j.starts), until(j.starts))
    }
}

async fn jams_page(State(app): State<Shared>, h: HeaderMap) -> Response {
    let p = page(&app, &h);
    let all = match app.jams() {
        Ok(l) => l,
        Err(e) => return fail_page(&app, &p, e),
    };
    let current = all.iter().find(|j| j.running()).cloned();
    let entries = current.as_ref().map(|j| app.jam_entries(j).unwrap_or_default()).unwrap_or_default();
    let mine = match (&p.me, &current) {
        (Some(me), Some(_)) => app.could_enter(me).unwrap_or_default(),
        _ => vec![],
    };
    let past: Vec<(crate::jams::Jam, Vec<crate::jams::Entry>)> = all.iter().filter(|j| j.over()).take(12).map(|j| (j.clone(), app.favourites(j).unwrap_or_default())).collect();
    let coming: Vec<&crate::jams::Jam> = all.iter().filter(|j| !j.running() && !j.over()).collect();
    let body = html! {
        h1 { "Jams" }
        p.sub { "A theme each week. Make something for it, share it to Everyone and enter it here. When the jam ends, the four entries with the most stars go on the Green of every new world for a month." }
        @match &current {
            Some(j) => {
                div.jam {
                    span.kind { "This week" }
                    h2 { a href={ "/jams/" (j.slug) } { (j.theme) } }
                    @if !j.about.is_empty() { p { (j.about) } }
                    p.sub { (jam_dates(j)) " · " (entries_count(entries.len())) }
                    (enter_form(j, &mine))
                }
                @if entries.is_empty() {
                    div.empty { "No entries yet. Yours could be the first." }
                } @else {
                    div.grid { @for e in &entries { (card(&e.creation)) } }
                }
            }
            None => { div.empty { "No jam is running just now." } }
        }
        @if !coming.is_empty() {
            h2 { "Coming up" }
            ul.rows { @for j in &coming { li { a href={ "/jams/" (j.slug) } { b { (j.theme) } } span.sub { (jam_dates(j)) } } } }
        }
        @if !past.is_empty() {
            h2 { "Past jams" }
            @for (j, favs) in &past {
                h3 { a href={ "/jams/" (j.slug) } { (j.theme) } " " span.sub { (jam_dates(j)) } }
                @if favs.is_empty() { p.sub { "No favourites: nobody entered." } } @else { div.grid { @for e in favs { (card(&e.creation)) } } }
            }
        }
        @if p.me.as_ref().is_some_and(|m| m.moderator) {
            h2 { "Start a jam" }
            form.stack method="post" action="/jams" {
                label { "Theme " input type="text" name="theme" maxlength="60" required placeholder="Things that float"; }
                label { "A few words about it " textarea name="about" maxlength="400" rows="2" {} }
                label { "Starts " input type="date" name="starts"; span.sub { " (today if left empty; days start at midnight UTC)" } }
                label { "Days " input type="number" name="days" min="1" max="31" value="7"; }
                button.btn.pri type="submit" { "Start the jam" }
            }
        }
    };
    layout(&app, &p, "Jams", "", body).into_response()
}

/// The form a maker uses to enter one of their creations.
fn enter_form(j: &crate::jams::Jam, mine: &[Creation]) -> Markup {
    html! {
        @if !mine.is_empty() {
            form.inline method="post" action={ "/jams/" (j.slug) "/enter" } {
                select name="code" aria-label="Your creation" { @for c in mine { option value=(c.code) { (c.name) } } }
                " " button.btn.pri type="submit" { "Enter it" }
            }
        }
    }
}

async fn jam_page(State(app): State<Shared>, h: HeaderMap, Path(slug): Path<String>, Query(q): Query<MeQ>) -> Response {
    let p = page(&app, &h);
    let j = match app.jam(&slug) {
        Ok(j) => j,
        Err(e) => return fail_page(&app, &p, e),
    };
    let entries = app.jam_entries(&j).unwrap_or_default();
    let mine = match &p.me {
        Some(me) if j.running() => app.could_enter(me).unwrap_or_default(),
        _ => vec![],
    };
    let me = p.me.as_ref().map(|m| m.user_id.clone()).unwrap_or_default();
    let body = html! {
        @if q.done == "entered" { div.note.ok { "Entered. Players can find it here, and stars it gets until the jam ends count." } }
        @if q.done == "left" { div.note.ok { "Taken out of the jam." } }
        span.kind { "Jam" }
        h1 { (j.theme) }
        @if !j.about.is_empty() { p { (j.about) } }
        p.sub { (jam_dates(&j)) " · " (entries_count(entries.len())) }
        (enter_form(&j, &mine))
        @if j.over() && !entries.is_empty() {
            h2 { "Favourites" }
            p.sub { "The most stars while the jam ran. They're on the Green of new worlds for a month after it ends." }
        }
        @if entries.is_empty() {
            div.empty { @if j.over() { "Nobody entered this one." } @else { "No entries yet." } }
        } @else {
            div.grid {
                @for (i, e) in entries.iter().enumerate() {
                    div {
                        (card(&e.creation))
                        p.sub {
                            @if j.over() && i < crate::jams::FAVOURITES { span.tag.ok { "favourite" } " " }
                            (count(e.stars, "star")) " in the jam"
                            @if !j.over() && e.creation.owner == me {
                                form.inline method="post" action={ "/jams/" (j.slug) "/leave" } {
                                    input type="hidden" name="code" value=(e.creation.code);
                                    " " button.btn type="submit" { "Take it out" }
                                }
                            }
                        }
                    }
                }
            }
        }
    };
    layout(&app, &p, &j.theme, "", body).into_response()
}

#[derive(Deserialize)]
struct JamForm {
    theme: String,
    #[serde(default)]
    about: String,
    #[serde(default)]
    starts: String,
    #[serde(default)]
    days: String,
}

async fn make_jam(State(app): State<Shared>, h: HeaderMap, Form(f): Form<JamForm>) -> Response {
    let p = page(&app, &h);
    if let Err(r) = same_site(&app, &h) {
        return r;
    }
    let me = match need_me(&app, &p) {
        Ok(m) => m,
        Err(r) => return r,
    };
    let starts = if f.starts.trim().is_empty() {
        0
    } else {
        match parse_day(&f.starts) {
            Some(t) => t,
            None => return error_page(&app, &p, 400, "That start date isn't one: use the date picker."),
        }
    };
    let days = f.days.trim().parse().unwrap_or(7);
    match app.make_jam(&me, &f.theme, &f.about, starts, days) {
        Ok(j) => Redirect::to(&format!("/jams/{}", j.slug)).into_response(),
        Err(e) => fail_page(&app, &p, e),
    }
}

#[derive(Deserialize)]
struct CodeForm {
    code: String,
}

async fn enter_jam(State(app): State<Shared>, h: HeaderMap, Path(slug): Path<String>, Form(f): Form<CodeForm>) -> Response {
    let p = page(&app, &h);
    if let Err(r) = same_site(&app, &h) {
        return r;
    }
    let me = match need_me(&app, &p) {
        Ok(m) => m,
        Err(r) => return r,
    };
    match app.enter_jam(&me, &slug, &f.code) {
        Ok(j) => Redirect::to(&format!("/jams/{}?done=entered", j.slug)).into_response(),
        Err(e) => fail_page(&app, &p, e),
    }
}

async fn leave_jam(State(app): State<Shared>, h: HeaderMap, Path(slug): Path<String>, Form(f): Form<CodeForm>) -> Response {
    let p = page(&app, &h);
    if let Err(r) = same_site(&app, &h) {
        return r;
    }
    let me = match need_me(&app, &p) {
        Ok(m) => m,
        Err(r) => return r,
    };
    match app.leave_jam(&me, &slug, &f.code) {
        Ok(()) => Redirect::to(&format!("/jams/{slug}?done=left")).into_response(),
        Err(e) => fail_page(&app, &p, e),
    }
}
