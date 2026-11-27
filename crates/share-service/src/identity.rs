//! Who's asking. Players never make an account here: the game signs in to
//! the LinuxGroove game server with its anonymous device id, and sends that
//! session with each request. The service asks the game server whose session
//! it is, once every few minutes, and keeps its own record (handle, beta,
//! moderator) under the same user id. Browsers get a session of their own
//! from a one-time link the game opens.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub struct Identity {
    http: reqwest::Client,
    nakama: String,
    dev: bool,
    cache: Mutex<HashMap<String, (String, Instant)>>,
}

const KEEP: Duration = Duration::from_secs(300);

impl Identity {
    pub fn new(nakama: &str, dev: bool, http: reqwest::Client) -> Identity {
        Identity { http, nakama: nakama.to_string(), dev, cache: Mutex::new(HashMap::new()) }
    }

    /// The game server's user id for a session token.
    pub async fn user_id(&self, token: &str) -> Result<String, String> {
        if self.dev {
            if let Some(rest) = token.strip_prefix("dev:") {
                let id = rest.split(':').next().unwrap_or("");
                if !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
                    return Ok(format!("dev-{id}"));
                }
            }
        }
        if token.len() > 4096 || token.is_empty() {
            return Err("that sign-in isn't one".into());
        }
        {
            let mut cache = self.cache.lock().unwrap();
            cache.retain(|_, (_, at)| at.elapsed() < KEEP);
            if let Some((id, _)) = cache.get(token) {
                return Ok(id.clone());
            }
        }
        let r = self
            .http
            .get(format!("{}/v2/account", self.nakama))
            .bearer_auth(token)
            .timeout(Duration::from_secs(8))
            .send()
            .await
            .map_err(|e| format!("couldn't reach the game server to check who you are ({e})"))?;
        if r.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err("your sign-in has run out; try again".into());
        }
        if !r.status().is_success() {
            return Err(format!("the game server said {} when checking who you are", r.status()));
        }
        let v: serde_json::Value = r.json().await.map_err(|e| e.to_string())?;
        let id = v.pointer("/user/id").and_then(|i| i.as_str()).ok_or("the game server didn't say who you are")?.to_string();
        self.cache.lock().unwrap().insert(token.to_string(), (id.clone(), Instant::now()));
        Ok(id)
    }
}
