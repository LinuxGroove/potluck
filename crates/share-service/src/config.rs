//! The service's settings: a TOML file (`--config`, or `POTLUCK_SHARE_CONFIG`)
//! with secrets that may come from the environment instead, so they never sit
//! in a file that gets copied around.

use std::path::PathBuf;

use serde::Deserialize;

#[derive(Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Config {
    /// Where the service listens: `127.0.0.1:47180` behind a proxy.
    pub listen: String,
    /// The address players and browsers use, without a trailing slash.
    pub public_url: String,
    /// The index (SQLite), the signing key, and files when `store.kind` is "folder".
    pub data: PathBuf,
    /// The LinuxGroove game server; game sign-ins are checked against it.
    pub nakama: String,
    pub forgejo: Forgejo,
    pub store: Store,
    /// Sharing to Everyone needs an invite while this is on.
    pub invite_only: bool,
    /// Different players' reports that hide a creation until a moderator looks.
    pub hide_after_reports: u32,
    /// Words the text check looks for, one a line (`word*` matches the start of words).
    pub words: Option<PathBuf>,
    /// Accept `dev:<id>:<name>` instead of a game sign-in: for tests and trying
    /// the service on one machine only. Refused when `public_url` is https.
    pub dev_accounts: bool,
    /// Let the server list check servers on this machine and private
    /// networks: for tests and a market on a LAN only.
    pub private_servers: bool,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Forgejo {
    /// Forgejo's address as the service reaches it; it isn't public.
    pub url: String,
    /// An admin's access token (all scopes), or `FORGEJO_TOKEN`.
    pub token: String,
    /// Commits carry `<handle>@<email_domain>` as the maker's address.
    pub email_domain: String,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Store {
    /// "folder" (files under `data/files`) or "s3".
    pub kind: String,
    pub endpoint: String,
    pub bucket: String,
    pub region: String,
    /// Or `S3_ACCESS_KEY_ID` and `S3_SECRET_ACCESS_KEY`.
    pub access_key: String,
    pub secret_key: String,
    /// A public address for the bucket (a CDN in front of it): files are sent
    /// there instead of through the service.
    pub public_url: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            listen: "127.0.0.1:47180".into(),
            public_url: "http://localhost:47180".into(),
            data: PathBuf::from("market-data"),
            nakama: "http://127.0.0.1:7350".into(),
            forgejo: Forgejo::default(),
            store: Store::default(),
            invite_only: true,
            hide_after_reports: 5,
            words: None,
            dev_accounts: false,
            private_servers: false,
        }
    }
}

impl Default for Forgejo {
    fn default() -> Self {
        Self { url: "http://127.0.0.1:3000".into(), token: String::new(), email_domain: "users.potluck.linuxgroove.com".into() }
    }
}

impl Default for Store {
    fn default() -> Self {
        Self { kind: "folder".into(), endpoint: String::new(), bucket: "potluck".into(), region: "garage".into(), access_key: String::new(), secret_key: String::new(), public_url: String::new() }
    }
}

impl Config {
    pub fn load(path: Option<&str>) -> Result<Config, String> {
        let path = path.map(|s| s.to_string()).or_else(|| std::env::var("POTLUCK_SHARE_CONFIG").ok());
        let mut c: Config = match &path {
            Some(p) => {
                let text = std::fs::read_to_string(p).map_err(|e| format!("couldn't read {p}: {e}"))?;
                toml::from_str(&text).map_err(|e| format!("{p}: {e}"))?
            }
            None => Config::default(),
        };
        let env = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        if let Some(v) = env("FORGEJO_TOKEN") {
            c.forgejo.token = v;
        }
        if let Some(v) = env("S3_ACCESS_KEY_ID") {
            c.store.access_key = v;
        }
        if let Some(v) = env("S3_SECRET_ACCESS_KEY") {
            c.store.secret_key = v;
        }
        if let Some(v) = env("POTLUCK_SHARE_LISTEN") {
            c.listen = v;
        }
        if let Some(v) = env("POTLUCK_SHARE_DATA") {
            c.data = PathBuf::from(v);
        }
        c.public_url = c.public_url.trim_end_matches('/').to_string();
        c.forgejo.url = c.forgejo.url.trim_end_matches('/').to_string();
        c.nakama = c.nakama.trim_end_matches('/').to_string();
        if c.dev_accounts && c.public_url.starts_with("https://") {
            return Err("dev_accounts lets anyone sign in as anyone: it's only for tests on one machine, not a public address".into());
        }
        Ok(c)
    }
}
