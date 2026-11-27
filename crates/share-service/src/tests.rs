//! The market end to end against a real Forgejo: sharing, the closed beta,
//! the review queue, loading with signatures, a remix of a remix and its
//! family tree, a suggested change merged, reports, held versions, cloning
//! and signing in on the web. Set POTLUCK_TEST_FORGEJO (its address) and
//! POTLUCK_TEST_FORGEJO_TOKEN (an admin's token) to run it; without them it
//! says it was skipped. deploy/market/test.sh starts a Forgejo in a container.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::{json, Value};

use crate::checks::Words;
use crate::config::Config;
use crate::market::App;
use crate::{codes, db, forgejo, identity, store};

struct Market {
    url: String,
    http: reqwest::Client,
    app: Arc<App>,
    dir: std::path::PathBuf,
    run: String,
}

impl Drop for Market {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

async fn start() -> Option<Market> {
    let (Ok(fj), Ok(token)) = (std::env::var("POTLUCK_TEST_FORGEJO"), std::env::var("POTLUCK_TEST_FORGEJO_TOKEN")) else {
        eprintln!("skipped: set POTLUCK_TEST_FORGEJO and POTLUCK_TEST_FORGEJO_TOKEN (deploy/market/test.sh does)");
        return None;
    };
    // Forgejo outlives the test, so every run's makers have names of their own.
    let run = codes::new_code()[..5].to_lowercase();
    let dir = std::env::temp_dir().join(format!("potluck-share-test-{run}"));
    std::fs::create_dir_all(&dir).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let mut cfg = Config::default();
    cfg.public_url = url.clone();
    cfg.data = dir.clone();
    cfg.dev_accounts = true;
    cfg.forgejo.url = fj.trim_end_matches('/').to_string();
    cfg.forgejo.token = token;
    cfg.forgejo.email_domain = "test.invalid".into();
    cfg.store.kind = "folder".into();
    let http = reqwest::Client::new();
    let conn = db::open(&dir.join("market.db")).unwrap();
    let st = store::Store::new(&cfg, http.clone()).unwrap();
    let git = forgejo::Forgejo::new(&cfg.forgejo, http.clone());
    let ids = identity::Identity::new(&cfg.nakama, true, http.clone());
    let key = codes::signing_key(&dir).unwrap();
    let words = Words::parse("grumbleweed\nsnark*\n");
    let app = Arc::new(App::new(cfg, conn, git, st, ids, key, words));
    // The base game's creations belong to Potluck's own account: each run
    // gets one of its own in git hosting.
    app.db().execute("INSERT INTO accounts (user_id, handle, name, created, beta, trusted) VALUES ('potluck', ?1, 'Potluck', 0, 1, 1)", [format!("potluck-{run}")]).unwrap();
    let router = crate::router(app.clone());
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let http = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).build().unwrap();
    Some(Market { url, http, app, dir, run })
}

/// A player, as the game would be: a game server session (dev accounts
/// take `dev:ID:NAME`) and the name they chose.
struct Player {
    token: String,
    name: String,
    /// The game says its player is under 13.
    young: bool,
}

impl Market {
    fn player(&self, who: &str) -> Player {
        let name = format!("{who} {}", self.run);
        Player { token: format!("dev:{who}-{}:{name}", self.run), name, young: false }
    }

    async fn call(&self, p: Option<&Player>, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
        let mut r = self.http.request(method.parse().unwrap(), format!("{}{path}", self.url));
        if let Some(p) = p {
            r = r.bearer_auth(&p.token).header("x-potluck-name", &p.name);
            if p.young {
                r = r.header("x-potluck-young", "1");
            }
        }
        if let Some(b) = body {
            r = r.json(&b);
        }
        let res = r.send().await.unwrap();
        let status = res.status().as_u16();
        let text = res.text().await.unwrap();
        (status, serde_json::from_str(&text).unwrap_or(Value::String(text)))
    }

    async fn page(&self, path: &str, cookie: Option<&str>) -> (u16, String) {
        let mut r = self.http.get(format!("{}{path}", self.url));
        if let Some(c) = cookie {
            r = r.header("cookie", c);
        }
        let res = r.send().await.unwrap();
        (res.status().as_u16(), res.text().await.unwrap())
    }

    async fn form(&self, path: &str, cookie: &str, fields: &[(&str, &str)]) -> reqwest::Response {
        let body = url::form_urlencoded::Serializer::new(String::new()).extend_pairs(fields).finish();
        self.http
            .post(format!("{}{path}", self.url))
            .header("cookie", cookie)
            .header("origin", &self.url)
            .header("content-type", "application/x-www-form-urlencoded")
            .body(body)
            .send()
            .await
            .unwrap()
    }

    /// Loads a code as the game does: the manifest must carry the market's
    /// signature, and every file must match its hash.
    async fn load(&self, p: Option<&Player>, code: &str) -> (u16, BTreeMap<String, String>, Value) {
        let (status, v) = self.call(p, "GET", &format!("/api/c/{code}"), None).await;
        if status != 200 {
            return (status, BTreeMap::new(), v);
        }
        let (_, key) = self.call(None, "GET", "/api/key", None).await;
        let text = v["manifest"].as_str().unwrap();
        assert!(codes::verify(key["key"].as_str().unwrap(), text, v["signature"].as_str().unwrap()), "the manifest is signed by the market");
        assert!(!codes::verify(key["key"].as_str().unwrap(), &text.replace("\"version\"", "\"Version\""), v["signature"].as_str().unwrap()), "a changed manifest fails");
        let manifest: Value = serde_json::from_str(text).unwrap();
        let mut files = BTreeMap::new();
        for (path, f) in manifest["files"].as_object().unwrap() {
            let sha = f["sha256"].as_str().unwrap();
            let bytes = self.http.get(format!("{}/f/{sha}", self.url)).send().await.unwrap().bytes().await.unwrap();
            assert_eq!(codes::sha256_hex(&bytes), sha, "{path} matches its hash");
            files.insert(path.clone(), String::from_utf8(bytes.to_vec()).unwrap());
        }
        (status, files, manifest)
    }

    async fn web_session(&self, p: &Player, next: &str) -> (String, String) {
        let (status, v) = self.call(Some(p), "POST", "/api/signin", Some(json!({ "next": next }))).await;
        assert_eq!(status, 200, "{v}");
        let link = v["url"].as_str().unwrap().to_string();
        let res = self.http.get(&link).send().await.unwrap();
        assert_eq!(res.status().as_u16(), 303, "the link signs in and moves on");
        let to = res.headers()["location"].to_str().unwrap().to_string();
        let cookie = res.headers()["set-cookie"].to_str().unwrap().split(';').next().unwrap().to_string();
        let again = self.http.get(&link).send().await.unwrap();
        assert_eq!(again.status().as_u16(), 410, "a link works once");
        (cookie, to)
    }

    fn queue_item(&self, code: &str) -> i64 {
        self.app.db().query_row("SELECT q.id FROM queue q JOIN creations c ON c.id = q.creation WHERE c.code = ?1 AND q.open = 1 AND q.why != 'checks'", [code], |r| r.get(0)).unwrap()
    }
}

fn mossback_with(name: &str, by: &str, main_extra: &str) -> BTreeMap<String, String> {
    let mut files: BTreeMap<String, String> = crate::market::BASE.iter().find(|(id, _)| *id == "mossback").unwrap().1.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    let mut toml: toml::Table = files["potluck.toml"].parse().unwrap();
    toml.insert("name".into(), name.into());
    toml.insert("by".into(), by.into());
    files.insert("potluck.toml".into(), toml::to_string_pretty(&toml).unwrap());
    let main = files.get_mut("main.luau").unwrap();
    main.push_str(main_extra);
    files
}

#[tokio::test]
async fn the_market_end_to_end() {
    let Some(m) = start().await else { return };
    m.app.seed_base().await.expect("the base creations");
    let (_, base) = m.call(None, "GET", "/api/base", None).await;
    let moss = base["mossback"]["code"].as_str().expect("Mossback is on the market").to_string();
    let (status, _, manifest) = m.load(None, &moss).await;
    assert_eq!(status, 200);
    assert_eq!(manifest["name"], "Mossback");

    let ada = m.player("Ada");
    let bo = m.player("Bo");
    let mo = m.player("Mo");

    // Ada remixes Mossback and shares it to Everyone, but isn't in the beta yet.
    let files = mossback_with(&format!("Mossy {}", m.run), &ada.name, "\n-- Ada's mossback is a little quicker\n");
    let (status, s) = m.call(Some(&ada), "POST", "/api/share", Some(json!({ "files": files, "visibility": "everyone", "from": { "base": "mossback", "version": 1 } }))).await;
    assert_eq!(status, 200, "{s}");
    let ada_code = s["code"].as_str().unwrap().to_string();
    assert_eq!(s["listing"], "unlisted", "no invite: anyone with the code, not the market");
    assert!(s["message"].as_str().unwrap().contains("closed beta"), "{s}");
    assert_eq!(s["state"], "ok");

    // An invite lets Ada in; sharing again to Everyone waits for a moderator.
    let invite = m.app.make_invites("test", 1).unwrap().remove(0);
    let (status, _) = m.call(Some(&ada), "POST", "/api/invite", Some(json!({ "code": codes::pretty(&invite) }))).await;
    assert_eq!(status, 200);
    let (status, _) = m.call(Some(&bo), "POST", "/api/invite", Some(json!({ "code": invite }))).await;
    assert_eq!(status, 404, "an invite works once");
    let files = mossback_with(&format!("Mossy {}", m.run), &ada.name, "\n-- Ada's mossback is a little quicker\n-- and greener\n");
    let (status, s) = m.call(Some(&ada), "POST", "/api/share", Some(json!({ "code": ada_code, "files": files, "visibility": "everyone", "note": "Greener" }))).await;
    assert_eq!(status, 200, "{s}");
    assert_eq!(s["version"], 2);
    assert_eq!(s["listing"], "waiting");
    let (status, page) = m.page("/?q=Mossy", None).await;
    assert_eq!(status, 200);
    assert!(!page.contains(&ada_code), "not on Explore before a moderator looks");

    // Mo is a moderator and lists it.
    m.call(Some(&mo), "GET", "/api/me", None).await;
    let mo_id = format!("dev-Mo-{}", m.run);
    m.app.db().execute("UPDATE accounts SET moderator = 1 WHERE user_id = ?1", [&mo_id]).unwrap();
    let mo_acct = db::account(&m.app.db(), &mo_id).unwrap().expect("Mo's account");
    m.app.decide(&mo_acct, m.queue_item(&ada_code), "list", "lovely").unwrap();
    let (_, page) = m.page(&format!("/?q=Mossy+{}", m.run), None).await;
    assert!(page.contains(&codes::pretty(&ada_code)), "on Explore once listed");

    let (status, ada_files, manifest) = m.load(Some(&bo), &ada_code).await;
    assert_eq!(status, 200);
    assert_eq!(manifest["version"], 2);
    assert_eq!(manifest["from"]["code"], moss, "the manifest says what it's a remix of");
    assert!(ada_files["main.luau"].contains("and greener"));

    // Bo remixes Ada's remix and changes it.
    let mut bo_files = ada_files.clone();
    let mut toml: toml::Table = bo_files["potluck.toml"].parse().unwrap();
    toml.insert("name".into(), format!("Hummy {}", m.run).into());
    toml.insert("by".into(), bo.name.clone().into());
    toml.insert("remix_of".into(), format!("Mossy ({ada_code})").into());
    bo_files.insert("potluck.toml".into(), toml::to_string_pretty(&toml).unwrap());
    bo_files.get_mut("main.luau").unwrap().push_str("-- Bo's: they hum when you pat them\n");
    let (status, s) = m.call(Some(&bo), "POST", "/api/share", Some(json!({ "files": bo_files, "visibility": "code", "from": { "code": ada_code, "version": 2 } }))).await;
    assert_eq!(status, 200, "{s}");
    let bo_code = s["code"].as_str().unwrap().to_string();
    assert_eq!(s["listing"], "unlisted");

    // The family tree on the web: Mossback, then Ada's, then Bo's.
    let (status, page) = m.page(&format!("/c/{bo_code}?tab=remixes"), None).await;
    assert_eq!(status, 200);
    let line = &page[page.find("<div class=\"line\">").expect("the line back to the first")..];
    let line = &line[..line.find("</div>").unwrap()];
    let at = |s: &str| line.find(s).unwrap_or_else(|| panic!("{s} in {line}"));
    assert!(at("Mossback") < at("Mossy") && at("Mossy") < at("Hummy"), "the line runs from Mossback to Bo's remix: {line}");
    let tree = &page[page.find("<ul class=\"tree\">").expect("the tree")..];
    assert!(tree.contains(&codes::pretty(&moss)) && tree.contains(&codes::pretty(&ada_code)) && tree.contains(&codes::pretty(&bo_code)));
    let (_, page) = m.page(&format!("/c/{moss}?tab=remixes"), None).await;
    assert!(page.contains(&format!("/c/{ada_code}")), "Mossback's tree has Ada's listed remix");
    assert!(!page.contains(&format!("/c/{bo_code}")) && page.contains("One more remix isn"), "but not Bo's, shared only by code");

    // Bo suggests the change to Ada, who sees it and its diff.
    let (status, s) = m.call(Some(&bo), "POST", &format!("/api/c/{bo_code}/suggest"), Some(json!({ "title": "Humming", "body": "They hum when you pat them." }))).await;
    assert_eq!(status, 200, "{s}");
    let id = s["id"].as_i64().unwrap();
    let (_, list) = m.call(Some(&ada), "GET", &format!("/api/c/{ada_code}/suggestions"), None).await;
    assert_eq!(list["suggestions"].as_array().unwrap().len(), 1);
    let (_, sug) = m.call(Some(&ada), "GET", &format!("/api/suggestions/{id}"), None).await;
    assert!(sug["diff"].as_str().unwrap().contains("+-- Bo's: they hum when you pat them"), "{sug}");
    assert!(sug["added"].as_i64().unwrap() >= 1);
    assert!(!sug["diff"].as_str().unwrap().contains("+by = ") && !sug["diff"].as_str().unwrap().contains("Hummy"), "the change keeps Ada's name and maker: {}", sug["diff"]);
    let (status, _) = m.call(Some(&bo), "POST", &format!("/api/suggestions/{id}/merge"), None).await;
    assert_eq!(status, 403, "only the maker merges");

    // Ada merges it from the game: a new version with Bo's change, still Ada's.
    let (status, s) = m.call(Some(&ada), "POST", &format!("/api/suggestions/{id}/merge"), None).await;
    assert_eq!(status, 200, "{s}");
    assert_eq!(s["version"], 3);
    let (_, merged, manifest) = m.load(None, &ada_code).await;
    assert_eq!(manifest["version"], 3, "the code loads the merged version");
    assert!(merged["main.luau"].contains("they hum when you pat them"));
    let toml: toml::Table = merged["potluck.toml"].parse().unwrap();
    assert_eq!(toml["by"].as_str(), Some(ada.name.as_str()), "the merge keeps its maker");
    assert!(toml.get("remix_of").is_none(), "and doesn't turn it into a remix of Bo's");
    let (_, sug) = m.call(None, "GET", &format!("/api/suggestions/{id}"), None).await;
    assert_eq!(sug["state"], "merged");
    let (_, page) = m.page(&format!("/c/{ada_code}?tab=versions"), None).await;
    assert!(page.contains("Humming"), "the versions list says what was merged");

    // Anyone can clone it through the market.
    let clone = m.dir.join("clone");
    let handle: String = m.app.db().query_row("SELECT a.handle || '/' || c.slug FROM creations c JOIN accounts a ON a.user_id = c.owner WHERE c.code = ?1", [&ada_code], |r| r.get(0)).unwrap();
    let out = tokio::process::Command::new("git").args(["clone", "-q", &format!("{}/git/{handle}.git", m.url), clone.to_str().unwrap()]).env("GIT_TERMINAL_PROMPT", "0").output().await.unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(std::fs::read_to_string(clone.join("main.luau")).unwrap().contains("they hum when you pat them"));
    let push = tokio::process::Command::new("git").args(["-C", clone.to_str().unwrap(), "push", "-q", "origin", "HEAD:main"]).env("GIT_TERMINAL_PROMPT", "0").output().await.unwrap();
    assert!(!push.status.success(), "nobody pushes through the market");

    // Broken code is kept for its maker but nobody else loads it.
    let mut broken = mossback_with(&format!("Broken {}", m.run), &ada.name, "");
    broken.insert("main.luau".into(), "local function oops(\n".into());
    let (status, s) = m.call(Some(&ada), "POST", "/api/share", Some(json!({ "files": broken, "visibility": "code" }))).await;
    assert_eq!(status, 200, "{s}");
    assert_eq!(s["state"], "held");
    let broken_code = s["code"].as_str().unwrap().to_string();
    let (status, _, _) = m.load(None, &broken_code).await;
    assert_eq!(status, 404, "a held version doesn't load for others");
    let (status, _, _) = m.load(Some(&ada), &broken_code).await;
    assert_eq!(status, 200, "its maker can still load it");

    // Words the market doesn't allow, in a name, hold it too.
    let rude = mossback_with(&format!("Snarky {}", m.run), &ada.name, "");
    let (_, s) = m.call(Some(&ada), "POST", "/api/share", Some(json!({ "files": rude, "visibility": "code" }))).await;
    assert_eq!(s["state"], "held", "{s}");

    // Five reports from five players hide Bo's remix.
    for i in 0..5 {
        let r = m.player(&format!("Reporter{i}"));
        let (status, v) = m.call(Some(&r), "POST", &format!("/api/c/{bo_code}/report"), Some(json!({ "reason": "unkind", "note": "test" }))).await;
        assert_eq!(status, 200, "{v}");
        if i < 4 {
            assert_eq!(m.load(None, &bo_code).await.0, 200);
        }
    }
    assert_eq!(m.load(None, &bo_code).await.0, 451, "hidden while a moderator looks");
    let (status, _) = m.call(Some(&bo), "POST", &format!("/api/c/{bo_code}/report"), Some(json!({ "reason": "unkind" }))).await;
    assert_eq!(status, 400, "nobody reports their own");

    // Ada signs in on the web from the game (the QR code's link) and edits her profile.
    let (cookie, to) = m.web_session(&ada, "/me/profile").await;
    assert_eq!(to, "/me/profile");
    let (status, page) = m.page("/me/profile", Some(&cookie)).await;
    assert_eq!(status, 200);
    assert!(page.contains("Your profile"));
    let res = m.form("/me/profile", &cookie, &[("name", "Ada Lovelace"), ("about", "I make mossy things."), ("head", "23a88f"), ("eyes", "happy"), ("top", "leaf"), ("top_colour", "6dbb4c")]).await;
    assert_eq!(res.status().as_u16(), 303);
    let ada_handle = handle.split('/').next().unwrap().to_string();
    let (_, page) = m.page(&format!("/@{ada_handle}"), None).await;
    assert!(page.contains("Ada Lovelace") && page.contains("I make mossy things."));
    let svg = m.http.get(format!("{}/avatar/{ada_handle}.svg", m.url)).send().await.unwrap();
    assert_eq!(svg.headers()["content-type"], "image/svg+xml");
    assert!(svg.text().await.unwrap().contains("#23a88f"));
    let (_, me) = m.call(Some(&ada), "GET", "/api/me", None).await;
    assert_eq!(me["name"], "Ada Lovelace", "the game's name no longer changes a name set on the web");
    let res = m.form("/me/profile", &cookie, &[("name", "Grumbleweed"), ("head", "23a88f"), ("eyes", "happy"), ("top", "leaf"), ("top_colour", "6dbb4c")]).await;
    assert_eq!(res.status().as_u16(), 400, "words the market doesn't allow");
    let no_origin = m.http.post(format!("{}/me/profile", m.url)).header("cookie", &cookie).header("content-type", "application/x-www-form-urlencoded").body("name=Sneaky").send().await.unwrap();
    assert_eq!(no_origin.status().as_u16(), 403, "a form from another site");
    let (_, away) = m.web_session(&ada, "//elsewhere.example/").await;
    assert_eq!(away, "/me", "a sign-in link never sends anyone away");
    let (status, _) = m.page("/me/profile", None).await;
    assert_eq!(status, 303, "signed out: off to sign in");
}

#[tokio::test]
async fn keep_identity_puts_the_makers_name_back() {
    let old = "name = \"Mossy\"\nby = \"Ada\"\nversion = 2\n[needs]\nworld = \"0.1\"\n";
    let new = "name = \"Mossy\"\nby = \"Bo\"\nremix_of = \"Mossy (ABCD2345)\"\nversion = 1\n[needs]\nworld = \"0.1\"\ntweak = \"0.1\"\n";
    let kept: toml::Table = crate::market::keep_identity(old, new).unwrap().parse().unwrap();
    assert_eq!(kept["by"].as_str(), Some("Ada"));
    assert_eq!(kept["version"].as_integer(), Some(2));
    assert!(kept.get("remix_of").is_none());
    assert!(kept["needs"].get("tweak").is_some(), "the rest of the change stays");
    assert!(crate::market::keep_identity(old, old).is_none());
    let tidy = "name = \"Mossy\"\nby = \"Bo\"\ndescription = \"hums\"\nversion = 1\n\n[needs]\nworld = \"0.1\"\n";
    let kept = crate::market::keep_identity(old, tidy).unwrap();
    assert_eq!(kept, "name = \"Mossy\"\nby = \"Ada\"\ndescription = \"hums\"\nversion = 2\n\n[needs]\nworld = \"0.1\"\n", "only the maker's fields change, in place");
    assert_eq!(crate::market::local_path("/c/ABCD2345"), Some("/c/ABCD2345"));
    assert_eq!(crate::market::local_path("//evil.example"), None);
    assert_eq!(crate::market::local_path("https://evil.example"), None);
}

/// Younger players share by code, send no suggestions to strangers and have
/// no line about them on their profile. A moderator checks a creation for
/// all ages, Explore's filter finds it (and the base game), and a new
/// version starts without the mark.
#[tokio::test]
async fn younger_players_and_checked_for_all_ages() {
    let Some(m) = start().await else { return };
    m.app.seed_base().await.expect("the base creations");
    let (_, base) = m.call(None, "GET", "/api/base", None).await;
    let moss = base["mossback"]["code"].as_str().unwrap().to_string();
    let kid = Player { young: true, ..m.player("Kid") };
    m.app.db().execute("UPDATE accounts SET beta = 1 WHERE user_id = ?1", [format!("dev-Kid-{}", m.run)]).unwrap();
    let (_, me) = m.call(Some(&kid), "GET", "/api/me", None).await;
    assert_eq!(me["young"], true, "{me}");

    let files = mossback_with(&format!("Kid's mossy {}", m.run), &kid.name, "\n-- it naps a lot\n");
    let (status, s) = m.call(Some(&kid), "POST", "/api/share", Some(json!({ "files": files, "visibility": "everyone", "from": { "base": "mossback", "version": 1 } }))).await;
    assert_eq!(status, 200, "{s}");
    let kid_code = s["code"].as_str().unwrap().to_string();
    assert_eq!(s["listing"], "unlisted", "by code, for friends");
    assert!(s["message"].as_str().unwrap().contains("Younger players share by code"), "{s}");
    let (status, v) = m.call(Some(&kid), "POST", &format!("/api/c/{kid_code}/suggest"), Some(json!({ "title": "Naps" }))).await;
    assert_eq!(status, 403, "no suggestions to strangers: {v}");

    let (cookie, _) = m.web_session(&kid, "/me/profile").await;
    let res = m.form("/me/profile", &cookie, &[("name", "Kid"), ("about", "I live on Elm Street")]).await;
    assert_eq!(res.status().as_u16(), 303);
    let row = db::account(&m.app.db(), &format!("dev-Kid-{}", m.run)).unwrap().unwrap();
    assert_eq!(row.about, "", "no line about a younger player");

    // Bo's creation, checked for all ages by Mo
    let bo = m.player("Bo");
    let mo = m.player("Mo");
    m.call(Some(&bo), "GET", "/api/me", None).await;
    m.call(Some(&mo), "GET", "/api/me", None).await;
    m.app.db().execute("UPDATE accounts SET beta = 1 WHERE user_id = ?1", [format!("dev-Bo-{}", m.run)]).unwrap();
    m.app.db().execute("UPDATE accounts SET moderator = 1 WHERE user_id = ?1", [format!("dev-Mo-{}", m.run)]).unwrap();
    let mo_acct = db::account(&m.app.db(), &format!("dev-Mo-{}", m.run)).unwrap().unwrap();
    let name = format!("Bo's gentle mossy {}", m.run);
    let (_, s) = m.call(Some(&bo), "POST", "/api/share", Some(json!({ "files": mossback_with(&name, &bo.name, "\n-- gentle\n"), "visibility": "everyone", "from": { "base": "mossback", "version": 1 } }))).await;
    let bo_code = s["code"].as_str().unwrap().to_string();
    assert_eq!(s["listing"], "waiting", "{s}");
    m.app.decide(&mo_acct, m.queue_item(&bo_code), "list_all", "sweet").unwrap();
    let (_, v) = m.call(None, "GET", &format!("/api/c/{bo_code}"), None).await;
    assert_eq!(v["all_ages"], true, "{v}");
    let (_, page) = m.page(&format!("/?ages=all&q=gentle+{}", m.run), None).await;
    assert!(page.contains(&codes::pretty(&bo_code)), "Explore's all-ages filter finds it");
    let (_, page) = m.page("/?ages=all&kind=creature", None).await;
    assert!(page.contains(&codes::pretty(&moss)), "and the base game");
    let (_, page) = m.page(&format!("/?ages=all&q=Kid+{}", m.run), None).await;
    assert!(!page.contains(&codes::pretty(&kid_code)));

    // a new version starts without the mark until someone looks again
    let (_, s) = m.call(Some(&bo), "POST", "/api/share", Some(json!({ "code": bo_code, "files": mossback_with(&name, &bo.name, "\n-- gentle\n-- and new\n"), "visibility": "everyone" }))).await;
    assert_eq!(s["version"], 2, "{s}");
    assert!(!db::creation_by_code(&m.app.db(), &bo_code).unwrap().unwrap().all_ages);
    m.app.set_all_ages(&mo_acct, &bo_code, true).unwrap();
    assert!(db::creation_by_code(&m.app.db(), &bo_code).unwrap().unwrap().all_ages);
}
