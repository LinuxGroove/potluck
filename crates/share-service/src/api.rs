//! The JSON API the game uses. Every call that knows who you are carries
//! the game server session (`Authorization: Bearer ...`) and the player's
//! name (`X-Potluck-Name`).

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::codes;
use crate::db::{self, Account};
use crate::market::{App, Fail, ShareReq};
use crate::store::is_hash;

pub type Shared = Arc<App>;

impl IntoResponse for Fail {
    fn into_response(self) -> Response {
        let status = StatusCode::from_u16(self.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        (status, Json(json!({ "error": self.message }))).into_response()
    }
}

type R<T> = Result<T, Fail>;

pub fn routes() -> Router<Shared> {
    Router::new()
        .route("/api/key", get(key))
        .route("/api/me", get(me))
        .route("/api/base", get(base))
        .route("/api/share", post(share))
        .route("/api/mine", get(mine))
        .route("/api/c/{code}", get(load))
        .route("/api/c/{code}/report", post(report))
        .route("/api/c/{code}/star", post(star))
        .route("/api/c/{code}/suggest", post(suggest))
        .route("/api/c/{code}/suggestions", get(suggestions))
        .route("/api/suggestions/{id}", get(suggestion))
        .route("/api/suggestions/{id}/merge", post(merge))
        .route("/api/suggestions/{id}/close", post(close))
        .route("/api/signin", post(signin))
        .route("/api/invite", post(invite))
        .route("/api/link", post(link))
        .route("/api/link/code", post(link_code))
        .route("/api/link/leave", post(leave))
        .route("/api/pass", post(pass))
        .route("/api/servers", get(servers))
        .route("/api/servers/heartbeat", post(heartbeat))
        .route("/api/servers/boards", post(boards_post))
        .route("/api/c/{code}/boards", get(boards))
        .route("/f/{sha}", get(file))
}

fn bearer(h: &HeaderMap) -> Option<String> {
    h.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer ")).map(|s| s.trim().to_string())
}

fn name(h: &HeaderMap) -> String {
    h.get("x-potluck-name").and_then(|v| v.to_str().ok()).unwrap_or("").to_string()
}

/// Whether the game says its player is under 13, when it says.
fn young(h: &HeaderMap) -> Option<bool> {
    match h.get("x-potluck-young").and_then(|v| v.to_str().ok()) {
        Some("1") => Some(true),
        Some("0") => Some(false),
        _ => None,
    }
}

pub async fn who(app: &App, h: &HeaderMap) -> R<Account> {
    match bearer(h) {
        Some(t) => app.game_account(&t, &name(h), young(h)).await,
        None => Err(Fail { status: 401, message: "sign in to the game server first".into() }),
    }
}

async fn maybe_who(app: &App, h: &HeaderMap) -> Option<Account> {
    if bearer(h).is_some() { who(app, h).await.ok() } else { None }
}

async fn key(State(app): State<Shared>) -> Json<Value> {
    Json(json!({ "key": codes::public_key(&app.key) }))
}

async fn me(State(app): State<Shared>, h: HeaderMap) -> R<Json<Value>> {
    let a = who(&app, &h).await?;
    let (device, _) = app.game_device(&bearer(&h).unwrap_or_default()).await?;
    Ok(Json(json!({
        "handle": a.handle, "name": a.name, "beta": a.beta || !app.cfg.invite_only, "moderator": a.moderator, "trusted": a.trusted,
        "linked": device != a.user_id, "young": a.young,
        "url": format!("{}/@{}", app.cfg.public_url, a.handle),
        "about": a.about, "avatar": format!("{}/avatar/{}.svg", app.cfg.public_url, a.handle), "named_on_web": a.named_on_web,
    })))
}

async fn base(State(app): State<Shared>) -> R<Json<Value>> {
    let db = app.db();
    let list = db::creations_where(&db, "WHERE c.base IS NOT NULL", &[])?;
    let map: serde_json::Map<String, Value> = list.into_iter().map(|c| (c.base.clone().unwrap_or_default(), json!({ "code": c.code, "version": c.latest }))).collect();
    Ok(Json(Value::Object(map)))
}

async fn share(State(app): State<Shared>, h: HeaderMap, Json(req): Json<ShareReq>) -> R<Json<Value>> {
    let a = who(&app, &h).await?;
    let s = app.share(&a, req).await?;
    Ok(Json(serde_json::to_value(s).unwrap_or_default()))
}

async fn mine(State(app): State<Shared>, h: HeaderMap) -> R<Json<Value>> {
    let a = who(&app, &h).await?;
    let db = app.db();
    let list = db::creations_where(&db, "WHERE c.owner = ?1 AND c.listing != 'removed' ORDER BY c.updated DESC", &[&a.user_id])?;
    let mut out = Vec::new();
    for c in list {
        let open: i64 = db.query_row("SELECT COUNT(*) FROM suggestions WHERE creation = ?1 AND state = 'open'", [c.id], |r| r.get(0))?;
        let state: String = db.query_row("SELECT state FROM versions WHERE creation = ?1 AND n = ?2", rusqlite::params![c.id, c.latest], |r| r.get(0)).unwrap_or_default();
        out.push(json!({
            "code": c.code, "name": c.name, "version": c.latest, "listing": c.listing, "state": state,
            "suggestions": open, "stars": c.stars, "plays": c.plays, "remixes": c.remixes, "url": app.url(&c),
        }));
    }
    Ok(Json(json!({ "creations": out })))
}

#[derive(Deserialize)]
struct LoadQ {
    v: Option<i64>,
}

async fn load(State(app): State<Shared>, h: HeaderMap, Path(code): Path<String>, Query(q): Query<LoadQ>) -> R<Json<Value>> {
    let a = maybe_who(&app, &h).await;
    let anon = h.get("x-forwarded-for").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
    let (c, v) = app.load(&code, q.v, a.as_ref(), &anon)?;
    Ok(Json(json!({
        "manifest": v.manifest, "signature": v.signature, "key": codes::public_key(&app.key),
        "listing": c.listing, "latest": c.latest, "state": v.state, "url": app.url(&c),
        "all_ages": c.all_ages && v.n == c.latest,
    })))
}

#[derive(Deserialize)]
struct ReportReq {
    reason: String,
    #[serde(default)]
    note: String,
}

async fn report(State(app): State<Shared>, h: HeaderMap, Path(code): Path<String>, Json(r): Json<ReportReq>) -> R<Json<Value>> {
    let a = who(&app, &h).await?;
    app.report(&a, &code, &r.reason, &r.note)?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct StarReq {
    on: bool,
}

async fn star(State(app): State<Shared>, h: HeaderMap, Path(code): Path<String>, Json(r): Json<StarReq>) -> R<Json<Value>> {
    let a = who(&app, &h).await?;
    let n = app.star(&a, &code, r.on)?;
    Ok(Json(json!({ "stars": n })))
}

#[derive(Deserialize)]
struct SuggestReq {
    #[serde(default)]
    title: String,
    #[serde(default)]
    body: String,
}

async fn suggest(State(app): State<Shared>, h: HeaderMap, Path(code): Path<String>, Json(r): Json<SuggestReq>) -> R<Json<Value>> {
    let a = who(&app, &h).await?;
    let id = app.suggest(&a, &code, &r.title, &r.body).await?;
    Ok(Json(json!({ "id": id })))
}

fn suggestion_json(app: &App, s: &crate::market::Suggestion) -> R<Value> {
    let db = app.db();
    let from = db::creation(&db, s.from_creation)?;
    let to = db::creation(&db, s.creation)?;
    let by = db::account(&db, &s.by_user)?;
    Ok(json!({
        "id": s.id, "title": s.title, "body": s.body, "state": s.state, "created": s.created,
        "by": by.map(|b| b.name).unwrap_or_default(),
        "code": to.map(|c| c.code).unwrap_or_default(),
        "from_code": from.as_ref().map(|c| c.code.clone()).unwrap_or_default(),
        "from_name": from.map(|c| c.name).unwrap_or_default(),
        "from_version": s.from_version, "merged_version": s.merged_version,
    }))
}

async fn suggestions(State(app): State<Shared>, Path(code): Path<String>) -> R<Json<Value>> {
    let c = db::creation_by_code(&app.db(), &codes::clean(&code))?.ok_or(Fail { status: 404, message: "no creation has that code".into() })?;
    let list = app.suggestions_for(c.id, true)?;
    let out: Vec<Value> = list.iter().filter_map(|s| suggestion_json(&app, s).ok()).collect();
    Ok(Json(json!({ "suggestions": out })))
}

async fn suggestion(State(app): State<Shared>, Path(id): Path<i64>) -> R<Json<Value>> {
    let s = app.suggestion(id)?;
    let diff = app.suggestion_diff(&s).await.unwrap_or_default();
    let (add, del) = count_diff(&diff);
    let mut v = suggestion_json(&app, &s)?;
    v["diff"] = json!(diff);
    v["added"] = json!(add);
    v["removed"] = json!(del);
    Ok(Json(v))
}

async fn merge(State(app): State<Shared>, h: HeaderMap, Path(id): Path<i64>) -> R<Json<Value>> {
    let a = who(&app, &h).await?;
    let s = app.merge(&a, id).await?;
    Ok(Json(serde_json::to_value(s).unwrap_or_default()))
}

async fn close(State(app): State<Shared>, h: HeaderMap, Path(id): Path<i64>) -> R<Json<Value>> {
    let a = who(&app, &h).await?;
    app.close_suggestion(&a, id).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize, Default)]
struct SigninReq {
    #[serde(default)]
    next: String,
}

/// A one-time link that signs a browser in: the game opens it, or shows it
/// as a QR code for a phone. `next` is the market page it opens (`/me/profile`).
async fn signin(State(app): State<Shared>, h: HeaderMap, body: axum::body::Bytes) -> R<Json<Value>> {
    let a = who(&app, &h).await?;
    let next = serde_json::from_slice::<SigninReq>(&body).unwrap_or_default().next;
    Ok(Json(json!({ "url": app.signin_link(&a, &next)?, "expires_in": 300 })))
}

/// A code for another device to type, so it plays as this account too.
async fn link_code(State(app): State<Shared>, h: HeaderMap) -> R<Json<Value>> {
    let a = who(&app, &h).await?;
    let code = app.link_code(&a)?;
    Ok(Json(json!({ "code": codes::pretty(&code), "expires_in": crate::devices::LINK_SECONDS })))
}

#[derive(Deserialize)]
struct LinkReq {
    code: String,
    /// What the device is, for the account's list: "Steam Deck", "Windows".
    #[serde(default)]
    device: String,
}

/// This device plays as the account a link or recovery code belongs to.
async fn link(State(app): State<Shared>, h: HeaderMap, Json(r): Json<LinkReq>) -> R<Json<Value>> {
    let Some(token) = bearer(&h) else { return Err(Fail { status: 401, message: "sign in to the game server first".into() }) };
    let l = app.link(&token, &r.code, &r.device).await?;
    Ok(Json(serde_json::to_value(l).unwrap_or_default()))
}

/// This device stops playing as the account it was linked to.
async fn leave(State(app): State<Shared>, h: HeaderMap) -> R<Json<Value>> {
    let Some(token) = bearer(&h) else { return Err(Fail { status: 401, message: "sign in to the game server first".into() }) };
    app.leave(&token).await?;
    Ok(Json(json!({ "left": true })))
}

#[derive(Deserialize)]
struct InviteReq {
    code: String,
}

async fn invite(State(app): State<Shared>, h: HeaderMap, Json(r): Json<InviteReq>) -> R<Json<Value>> {
    let a = who(&app, &h).await?;
    app.redeem_invite(&a, &r.code)?;
    Ok(Json(json!({ "beta": true })))
}

#[derive(Deserialize, Default)]
struct PassReq {
    #[serde(default)]
    server: String,
}

/// A pass for joining a server: who the market says this player is.
async fn pass(State(app): State<Shared>, h: HeaderMap, body: axum::body::Bytes) -> R<Json<Value>> {
    let a = who(&app, &h).await?;
    let server = serde_json::from_slice::<PassReq>(&body).unwrap_or_default().server;
    Ok(Json(json!({ "pass": app.pass(&a, &server)?, "expires_in": crate::servers::PASS_SECONDS })))
}

async fn servers(State(app): State<Shared>) -> R<Json<Value>> {
    let list: Vec<Value> = app
        .listed_servers()?
        .into_iter()
        .map(|s| {
            json!({
                "name": s.name, "about": s.about, "address": s.address, "players": s.players, "max_players": s.max_players,
                "join": s.join, "version": s.version, "all_ages": s.all_ages,
                "creations": s.creations.iter().filter_map(|c| c["name"].as_str()).collect::<Vec<_>>(),
            })
        })
        .collect();
    Ok(Json(json!({ "servers": list })))
}

fn server_token(h: &HeaderMap) -> R<String> {
    h.get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Server "))
        .map(|s| s.trim().to_string())
        .ok_or(Fail { status: 401, message: "a server token goes in Authorization: Server <token>".into() })
}

async fn heartbeat(State(app): State<Shared>, h: HeaderMap, Json(hb): Json<crate::servers::Heartbeat>) -> R<Json<Value>> {
    let t = server_token(&h)?;
    Ok(Json(app.heartbeat(&t, hb).await?))
}

#[derive(Deserialize)]
struct BoardsReq {
    boards: Vec<crate::servers::BoardPost>,
}

async fn boards_post(State(app): State<Shared>, h: HeaderMap, Json(b): Json<BoardsReq>) -> R<Json<Value>> {
    let t = server_token(&h)?;
    let n = app.post_boards(&t, b.boards)?;
    Ok(Json(json!({ "scores": n })))
}

async fn boards(State(app): State<Shared>, Path(code): Path<String>) -> R<Json<Value>> {
    Ok(Json(json!({ "boards": app.boards_for(&codes::clean(&code))? })))
}

/// A file by its hash. It never changes, so anything may keep it forever.
async fn file(State(app): State<Shared>, Path(sha): Path<String>) -> Response {
    if !is_hash(&sha) {
        return (StatusCode::NOT_FOUND, "no such file").into_response();
    }
    if db::is_banned_file(&app.db(), &sha) {
        return (StatusCode::GONE, "that file was taken down").into_response();
    }
    if let Some(url) = app.store.public_url(&sha) {
        return Redirect::temporary(&url).into_response();
    }
    match app.store.get(&sha).await {
        Ok(Some(bytes)) => (
            [(header::CONTENT_TYPE, "application/octet-stream"), (header::CACHE_CONTROL, "public, max-age=31536000, immutable"), (header::X_CONTENT_TYPE_OPTIONS, "nosniff")],
            bytes,
        )
            .into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, "no such file").into_response(),
        Err(e) => {
            eprintln!("store: {e}");
            (StatusCode::BAD_GATEWAY, "the store had a problem").into_response()
        }
    }
}

/// A suggestion's diff with counts, for the web too.
pub fn count_diff(diff: &str) -> (usize, usize) {
    let mut add = 0;
    let mut del = 0;
    for l in diff.lines() {
        if l.starts_with('+') && !l.starts_with("+++") {
            add += 1;
        } else if l.starts_with('-') && !l.starts_with("---") {
            del += 1;
        }
    }
    (add, del)
}
