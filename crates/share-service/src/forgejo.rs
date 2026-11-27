//! Forgejo, where every creation is a git repository. The service talks to
//! it with an admin token and acts as each maker with `Sudo`, so commits,
//! forks and pull requests are theirs. Forgejo isn't reachable from outside:
//! the market shows what's in it and `git clone` goes through the service.

use std::collections::BTreeMap;

use base64::Engine;
use reqwest::{Method, StatusCode};
use serde_json::{json, Value};

use crate::codes::git_blob_sha;

#[derive(Clone)]
pub struct Forgejo {
    http: reqwest::Client,
    base: String,
    token: String,
    email_domain: String,
}

#[derive(Debug)]
pub struct Error {
    pub status: u16,
    pub message: String,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "git hosting said {}: {}", self.status, self.message)
    }
}

impl From<Error> for String {
    fn from(e: Error) -> String {
        e.to_string()
    }
}

type R<T> = Result<T, Error>;

fn err(status: u16, message: impl Into<String>) -> Error {
    Error { status, message: message.into() }
}

impl Forgejo {
    pub fn new(c: &crate::config::Forgejo, http: reqwest::Client) -> Forgejo {
        Forgejo { http, base: c.url.clone(), token: c.token.clone(), email_domain: c.email_domain.clone() }
    }

    async fn call(&self, method: Method, path: &str, sudo: Option<&str>, body: Option<Value>) -> R<Value> {
        let mut req = self.http.request(method, format!("{}/api/v1{path}", self.base)).header("Authorization", format!("token {}", self.token));
        if let Some(who) = sudo {
            req = req.header("Sudo", who);
        }
        if let Some(b) = body {
            req = req.json(&b);
        }
        let r = req.send().await.map_err(|e| err(0, format!("couldn't reach git hosting ({e})")))?;
        let status = r.status();
        let text = r.text().await.unwrap_or_default();
        if !status.is_success() {
            let msg = serde_json::from_str::<Value>(&text).ok().and_then(|v| v.get("message").and_then(|m| m.as_str()).map(String::from)).unwrap_or(text);
            return Err(err(status.as_u16(), msg));
        }
        Ok(if text.is_empty() { Value::Null } else { serde_json::from_str(&text).unwrap_or(Value::String(text)) })
    }

    pub async fn version(&self) -> R<String> {
        let v = self.call(Method::GET, "/version", None, None).await?;
        Ok(v.get("version").and_then(|v| v.as_str()).unwrap_or("?").to_string())
    }

    /// Makes the maker's Forgejo user the first time they share.
    pub async fn ensure_user(&self, handle: &str, name: &str) -> R<()> {
        match self.call(Method::GET, &format!("/users/{handle}"), None, None).await {
            // only one the service made: never a Forgejo account of someone else's
            Ok(u) if u["email"].as_str().is_some_and(|e| e.eq_ignore_ascii_case(&format!("{handle}@{}", self.email_domain))) => return Ok(()),
            Ok(_) => return Err(Error { status: 409, message: format!("the name {handle} is taken in git hosting") }),
            Err(e) if e.status == 404 => {}
            Err(e) => return Err(e),
        }
        // Nobody signs in to Forgejo as a maker, so the password is thrown away.
        let body = json!({
            "username": handle,
            "full_name": name,
            "email": format!("{handle}@{}", self.email_domain),
            "password": format!("{}Aa1!", crate::codes::token()),
            "must_change_password": false,
            "send_notify": false,
            "visibility": "public",
        });
        match self.call(Method::POST, "/admin/users", None, Some(body)).await {
            Ok(_) => Ok(()),
            // made by another request a moment ago
            Err(e) if e.status == 422 && self.call(Method::GET, &format!("/users/{handle}"), None, None).await.is_ok() => Ok(()),
            Err(e) => Err(e),
        }
    }

    pub async fn create_repo(&self, owner: &str, name: &str, about: &str) -> R<()> {
        let body = json!({ "name": name, "description": short(about), "private": false, "default_branch": "main", "auto_init": false });
        self.call(Method::POST, &format!("/admin/users/{owner}/repos"), None, Some(body)).await.map(|_| ())
    }

    /// Copies one commit (a tag or a branch of `from`, with all the history
    /// behind it) to a branch of `to`, with git itself: Forgejo's API can't,
    /// and its forks allow one per person per repository, where a player can
    /// remix the same thing as often as they like.
    pub async fn copy_commit(&self, from: &str, rev: &str, to: &str, branch: &str, scratch: &std::path::Path) -> R<String> {
        let dir = scratch.join(format!("git-{}", crate::codes::token()));
        let run = |args: Vec<String>| {
            let mut cmd = tokio::process::Command::new("git");
            cmd.args(&args)
                .env("GIT_TERMINAL_PROMPT", "0")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                // the token goes in the environment, not the command line others can see
                .env("GIT_CONFIG_COUNT", "2")
                .env("GIT_CONFIG_KEY_0", "http.extraHeader")
                .env("GIT_CONFIG_VALUE_0", format!("Authorization: token {}", self.token))
                .env("GIT_CONFIG_KEY_1", "push.negotiate")
                .env("GIT_CONFIG_VALUE_1", "false")
                .kill_on_drop(true);
            cmd
        };
        let d = dir.to_string_lossy().to_string();
        let url = |repo: &str| format!("{}/{repo}.git", self.base);
        let result = async {
            let steps: Vec<Vec<String>> = vec![
                vec!["init".into(), "-q".into(), "--bare".into(), d.clone()],
                vec!["-C".into(), d.clone(), "fetch".into(), "-q".into(), "--no-tags".into(), url(from), rev.into()],
            ];
            for args in steps {
                let out = run(args).output().await.map_err(|e| err(0, format!("couldn't run git ({e})")))?;
                if !out.status.success() {
                    return Err(err(500, format!("git: {}", String::from_utf8_lossy(&out.stderr).trim())));
                }
            }
            let out = run(vec!["-C".into(), d.clone(), "rev-parse".into(), "FETCH_HEAD^{commit}".into()]).output().await.map_err(|e| err(0, e.to_string()))?;
            let sha = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if sha.len() != 40 {
                return Err(err(500, "git found no commit to copy"));
            }
            let out = run(vec!["-C".into(), d.clone(), "push".into(), "-q".into(), "--force".into(), url(to), format!("{sha}:refs/heads/{branch}")]).output().await.map_err(|e| err(0, e.to_string()))?;
            if !out.status.success() {
                return Err(err(500, format!("git: {}", String::from_utf8_lossy(&out.stderr).trim())));
            }
            Ok(sha)
        }
        .await;
        let _ = tokio::fs::remove_dir_all(&dir).await;
        let sha = result?;
        // Forgejo takes a moment to notice a pushed branch.
        for _ in 0..100 {
            if let Ok(v) = self.call(Method::GET, &format!("/repos/{to}/branches/{branch}"), None, None).await {
                if v.pointer("/commit/id").and_then(|c| c.as_str()) == Some(sha.as_str()) {
                    return Ok(sha);
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        Err(err(500, format!("{to} didn't show the copied branch {branch}")))
    }

    pub async fn delete_branch(&self, repo: &str, branch: &str) -> R<()> {
        match self.call(Method::DELETE, &format!("/repos/{repo}/branches/{branch}"), None, None).await {
            Ok(_) => Ok(()),
            Err(e) if e.status == 404 => Ok(()),
            Err(e) => Err(e),
        }
    }

    /// Every file on a branch, by path, with its git blob name. An empty
    /// repository has none.
    pub async fn tree(&self, repo: &str, rev: &str) -> R<BTreeMap<String, String>> {
        let v = match self.call(Method::GET, &format!("/repos/{repo}/git/trees/{rev}?recursive=true&per_page=1000"), None, None).await {
            Ok(v) => v,
            Err(e) if e.status == 404 || e.status == 409 => return Ok(BTreeMap::new()),
            Err(e) => return Err(e),
        };
        let mut out = BTreeMap::new();
        for e in v.get("tree").and_then(|t| t.as_array()).into_iter().flatten() {
            if e.get("type").and_then(|t| t.as_str()) == Some("blob") {
                if let (Some(p), Some(s)) = (e.get("path").and_then(|p| p.as_str()), e.get("sha").and_then(|s| s.as_str())) {
                    out.insert(p.to_string(), s.to_string());
                }
            }
        }
        Ok(out)
    }

    pub async fn head(&self, repo: &str) -> R<String> {
        let v = self.call(Method::GET, &format!("/repos/{repo}/branches/main"), None, None).await?;
        Ok(v.pointer("/commit/id").and_then(|v| v.as_str()).unwrap_or("").to_string())
    }

    /// The newest commit on main, or None while the repository is empty.
    pub async fn head_opt(&self, repo: &str) -> R<Option<String>> {
        match self.head(repo).await {
            Ok(sha) => Ok(Some(sha).filter(|s| !s.is_empty())),
            Err(e) if e.status == 404 => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// One commit on main by the maker that makes the files exactly these.
    /// Returns the commit, or None when nothing changed.
    pub async fn commit(&self, repo: &str, handle: &str, name: &str, message: &str, files: &BTreeMap<String, String>) -> R<Option<String>> {
        let now = match self.head_opt(repo).await? {
            Some(sha) => self.tree(repo, &sha).await?,
            None => BTreeMap::new(),
        };
        let b64 = base64::engine::general_purpose::STANDARD;
        let mut ops = Vec::new();
        for (path, text) in files {
            match now.get(path) {
                Some(sha) if *sha == git_blob_sha(text.as_bytes()) => {}
                Some(sha) => ops.push(json!({ "operation": "update", "path": path, "content": b64.encode(text), "sha": sha })),
                None => ops.push(json!({ "operation": "create", "path": path, "content": b64.encode(text) })),
            }
        }
        for (path, sha) in &now {
            if !files.contains_key(path) {
                ops.push(json!({ "operation": "delete", "path": path, "sha": sha }));
            }
        }
        if ops.is_empty() {
            return Ok(None);
        }
        let who = json!({ "name": name, "email": format!("{handle}@{}", self.email_domain) });
        let body = json!({ "branch": "main", "message": message, "author": who, "committer": who, "files": ops });
        let v = self.call(Method::POST, &format!("/repos/{repo}/contents"), Some(handle), Some(body)).await?;
        let sha = v.pointer("/commit/sha").and_then(|s| s.as_str()).ok_or_else(|| err(500, "no commit came back"))?;
        Ok(Some(sha.to_string()))
    }

    /// Changes one file on a branch, as the service, written down as `name`.
    pub async fn update_file(&self, repo: &str, branch: &str, path: &str, old_sha: &str, text: &str, handle: &str, name: &str, message: &str) -> R<String> {
        let who = json!({ "name": name, "email": format!("{handle}@{}", self.email_domain) });
        let body = json!({
            "branch": branch, "message": message, "author": who, "committer": who,
            "files": [{ "operation": "update", "path": path, "sha": old_sha, "content": base64::engine::general_purpose::STANDARD.encode(text) }],
        });
        let v = self.call(Method::POST, &format!("/repos/{repo}/contents"), None, Some(body)).await?;
        v.pointer("/commit/sha").and_then(|s| s.as_str()).map(String::from).ok_or_else(|| err(500, "no commit came back"))
    }

    pub async fn tag(&self, repo: &str, handle: &str, tag: &str, target: &str, message: &str) -> R<()> {
        let body = json!({ "tag_name": tag, "target": target, "message": message });
        match self.call(Method::POST, &format!("/repos/{repo}/tags"), Some(handle), Some(body)).await {
            Ok(_) => Ok(()),
            Err(e) if e.status == 409 || e.status == 422 => Ok(()),
            Err(e) => Err(e),
        }
    }

    pub async fn read(&self, repo: &str, path: &str, rev: &str) -> R<Vec<u8>> {
        let url = format!("{}/api/v1/repos/{repo}/raw/{path}", self.base);
        let r = self
            .http
            .get(url)
            .query(&[("ref", rev)])
            .header("Authorization", format!("token {}", self.token))
            .send()
            .await
            .map_err(|e| err(0, e.to_string()))?;
        if !r.status().is_success() {
            return Err(err(r.status().as_u16(), format!("couldn't read {path}")));
        }
        Ok(r.bytes().await.map_err(|e| err(0, e.to_string()))?.to_vec())
    }

    /// Suggest a change: a pull request into the original's main from a
    /// branch holding the remix's commits, opened by the remixer.
    pub async fn open_pull(&self, base: &str, branch: &str, by: &str, title: &str, body: &str) -> R<u64> {
        let b = json!({ "head": branch, "base": "main", "title": title, "body": body });
        let v = self.call(Method::POST, &format!("/repos/{base}/pulls"), Some(by), Some(b)).await?;
        v.get("number").and_then(|n| n.as_u64()).ok_or_else(|| err(500, "no pull request number came back"))
    }

    pub async fn pull_diff(&self, base: &str, n: u64) -> R<String> {
        let r = self
            .http
            .get(format!("{}/api/v1/repos/{base}/pulls/{n}.diff", self.base))
            .header("Authorization", format!("token {}", self.token))
            .send()
            .await
            .map_err(|e| err(0, e.to_string()))?;
        if !r.status().is_success() {
            return Err(err(r.status().as_u16(), "couldn't read the change"));
        }
        r.text().await.map_err(|e| err(0, e.to_string()))
    }

    pub async fn merge_pull(&self, base: &str, n: u64, by: &str, message: &str) -> R<()> {
        let b = json!({ "Do": "merge", "merge_title_field": message, "merge_message_field": "", "delete_branch_after_merge": true });
        match self.call(Method::POST, &format!("/repos/{base}/pulls/{n}/merge"), Some(by), Some(b)).await {
            Ok(_) => Ok(()),
            Err(e) if e.status == 405 => Err(err(405, "it can't be merged as it is: the two have changed the same lines")),
            Err(e) => Err(e),
        }
    }

    pub async fn close_pull(&self, base: &str, n: u64, by: &str) -> R<()> {
        self.call(Method::PATCH, &format!("/repos/{base}/pulls/{n}"), Some(by), Some(json!({ "state": "closed" }))).await.map(|_| ())
    }

    /// Passes a `git clone` (or fetch) through to Forgejo, read-only.
    /// Passes a git client's clone or fetch through to the repository.
    /// `headers` are the client's Content-Type, Content-Encoding (git
    /// gzips big requests) and Git-Protocol.
    pub async fn git(&self, repo: &str, tail: &str, query: Option<&str>, method: Method, headers: &[(&str, &str)], body: bytes_body::Body) -> R<reqwest::Response> {
        let mut url = format!("{}/{repo}.git/{tail}", self.base);
        if let Some(q) = query {
            url = format!("{url}?{q}");
        }
        let mut req = self.http.request(method, url).header("Authorization", format!("token {}", self.token));
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        let req = match body {
            bytes_body::Body::Empty => req,
            bytes_body::Body::Bytes(b) => req.body(b),
        };
        let r = req.send().await.map_err(|e| err(0, e.to_string()))?;
        if r.status() == StatusCode::NOT_FOUND {
            return Err(err(404, "no such creation"));
        }
        Ok(r)
    }
}

pub mod bytes_body {
    pub enum Body {
        Empty,
        Bytes(Vec<u8>),
    }
}

fn short(s: &str) -> String {
    s.chars().take(250).collect()
}
