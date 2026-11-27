//! Where creations' files live, named by their SHA-256: a folder on this
//! machine, or an S3 bucket (Garage, MinIO, a cloud's) with a CDN in front.
//! A file is written once and never changes, so a file a thousand remixes
//! share is stored and downloaded once.

use std::path::PathBuf;
use std::time::Duration;

use rusty_s3::{Bucket, Credentials, S3Action, UrlStyle};

use crate::config;

pub enum Store {
    Folder(PathBuf),
    S3 { bucket: Bucket, creds: Credentials, http: reqwest::Client, public: Option<String> },
}

fn key(sha: &str) -> String {
    format!("files/{}/{}", &sha[..2], sha)
}

pub fn is_hash(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

impl Store {
    pub fn new(c: &config::Config, http: reqwest::Client) -> Result<Store, String> {
        match c.store.kind.as_str() {
            "folder" | "" => {
                let dir = c.data.join("files");
                std::fs::create_dir_all(&dir).map_err(|e| format!("couldn't make {}: {e}", dir.display()))?;
                Ok(Store::Folder(dir))
            }
            "s3" => {
                let s = &c.store;
                let endpoint = url::Url::parse(&s.endpoint).map_err(|e| format!("store.endpoint: {e}"))?;
                let bucket = Bucket::new(endpoint, UrlStyle::Path, s.bucket.clone(), s.region.clone()).map_err(|e| format!("store: {e}"))?;
                if s.access_key.is_empty() || s.secret_key.is_empty() {
                    return Err("store.kind is s3 but there's no access key (S3_ACCESS_KEY_ID and S3_SECRET_ACCESS_KEY)".into());
                }
                let creds = Credentials::new(s.access_key.clone(), s.secret_key.clone());
                let public = (!s.public_url.is_empty()).then(|| s.public_url.trim_end_matches('/').to_string());
                Ok(Store::S3 { bucket, creds, http, public })
            }
            other => Err(format!("store.kind {other:?} isn't folder or s3")),
        }
    }

    pub async fn put(&self, sha: &str, bytes: &[u8]) -> Result<(), String> {
        match self {
            Store::Folder(dir) => {
                let p = dir.join(&sha[..2]).join(sha);
                if p.exists() {
                    return Ok(());
                }
                let parent = p.parent().expect("a folder");
                tokio::fs::create_dir_all(parent).await.map_err(|e| e.to_string())?;
                let tmp = parent.join(format!(".{sha}.part"));
                tokio::fs::write(&tmp, bytes).await.map_err(|e| e.to_string())?;
                tokio::fs::rename(&tmp, &p).await.map_err(|e| e.to_string())
            }
            Store::S3 { bucket, creds, http, .. } => {
                let k = key(sha);
                let head = bucket.head_object(Some(creds), &k).sign(Duration::from_secs(60));
                if let Ok(r) = http.head(head.as_str()).send().await {
                    if r.status().is_success() {
                        return Ok(());
                    }
                }
                let put = bucket.put_object(Some(creds), &k).sign(Duration::from_secs(120));
                let r = http.put(put.as_str()).body(bytes.to_vec()).send().await.map_err(|e| format!("the store: {e}"))?;
                if !r.status().is_success() {
                    return Err(format!("the store said {} for {k}", r.status()));
                }
                Ok(())
            }
        }
    }

    pub async fn get(&self, sha: &str) -> Result<Option<Vec<u8>>, String> {
        match self {
            Store::Folder(dir) => match tokio::fs::read(dir.join(&sha[..2]).join(sha)).await {
                Ok(b) => Ok(Some(b)),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(e) => Err(e.to_string()),
            },
            Store::S3 { bucket, creds, http, .. } => {
                let get = bucket.get_object(Some(creds), &key(sha)).sign(Duration::from_secs(60));
                let r = http.get(get.as_str()).send().await.map_err(|e| format!("the store: {e}"))?;
                if r.status() == reqwest::StatusCode::NOT_FOUND {
                    return Ok(None);
                }
                if !r.status().is_success() {
                    return Err(format!("the store said {}", r.status()));
                }
                Ok(Some(r.bytes().await.map_err(|e| e.to_string())?.to_vec()))
            }
        }
    }

    /// Where a browser or game can fetch the file straight from a CDN, if the
    /// bucket has a public address.
    pub fn public_url(&self, sha: &str) -> Option<String> {
        match self {
            Store::S3 { public: Some(p), .. } => Some(format!("{p}/{}", key(sha))),
            _ => None,
        }
    }
}
