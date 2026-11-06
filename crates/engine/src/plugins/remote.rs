//! A plugin package fetched from a URL. Inspect downloads it once and holds it
//! under `tmp/`; install takes it back by an opaque token, so what the user
//! approved is the very file that gets installed.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use anyhow::Result;
use proto::error::ErrorInfo;
use proto::warning::WarningInfo;
use uuid::Uuid;

use crate::download::Downloader;

/// A plugin is a component and a few assets; this matches the uncompressed
/// limit its extraction enforces.
const MAX_PACKAGE_BYTES: u64 = 64 * 1024 * 1024;
/// A package nobody installs is dropped this long after it was fetched; `tmp/`
/// is emptied at startup besides.
const HOLD: Duration = Duration::from_secs(15 * 60);

struct Held {
    path: PathBuf,
    url: String,
    fetched: Instant,
    origin: Option<Origin>,
    warnings: Vec<WarningInfo>,
}

/// A repository's attestation of a package: its signature verified against the
/// key of `repository`, whose id is `signer`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Origin {
    pub signer: String,
    pub repository: String,
}

/// A held package: where it is and where it came from.
#[derive(Debug, Clone)]
pub struct Staged {
    pub path: PathBuf,
    pub url: String,
    pub origin: Option<Origin>,
    /// Degraded outcomes found while fetching, reported once the install is done.
    pub warnings: Vec<WarningInfo>,
}

pub struct Remote {
    dir: PathBuf,
    held: Mutex<HashMap<String, Held>>,
}

impl Remote {
    pub fn new(dir: PathBuf) -> Self {
        Remote {
            dir,
            held: Mutex::new(HashMap::new()),
        }
    }

    /// Downloads `url` (https only) and returns the token that names the file.
    pub async fn fetch(&self, url: &str) -> Result<String> {
        let url = https_url(url)?;
        self.download(&url, MAX_PACKAGE_BYTES).await
    }

    /// A small document (an index, a signature) read into memory and not kept.
    pub async fn fetch_capped(&self, url: &str, max_bytes: u64) -> Result<Vec<u8>> {
        let url = https_url(url)?;
        let token = self.download(&url, max_bytes).await?;
        let path = self.staged(&token)?.path;
        let bytes = tokio::fs::read(&path).await;
        self.discard(&token);
        Ok(bytes?)
    }

    /// Marks a held package as verified by a repository's signature.
    pub fn attest(&self, token: &str, origin: Origin, warnings: Vec<WarningInfo>) {
        if let Some(held) = self.held.lock().unwrap().get_mut(token) {
            held.origin = Some(origin);
            held.warnings = warnings;
        }
    }

    async fn download(&self, url: &str, max_bytes: u64) -> Result<String> {
        self.expire();
        let token = Uuid::new_v4().simple().to_string();
        let path = self.dir.join(format!("{token}.hplugin"));
        tracing::debug!(url, "fetching a plugin package");
        Downloader::untrusted(max_bytes)
            .fetch(url, &path, None, &|_| Ok(()))
            .await?;
        self.held.lock().unwrap().insert(
            token.clone(),
            Held {
                path,
                url: url.to_string(),
                fetched: Instant::now(),
                origin: None,
                warnings: Vec::new(),
            },
        );
        Ok(token)
    }

    /// Fails for a token never issued, already discarded or expired.
    pub fn staged(&self, token: &str) -> Result<Staged> {
        self.expire();
        self.held
            .lock()
            .unwrap()
            .get(token)
            .map(|held| Staged {
                path: held.path.clone(),
                url: held.url.clone(),
                origin: held.origin.clone(),
                warnings: held.warnings.clone(),
            })
            .ok_or_else(|| {
                ErrorInfo::PluginInvalid {
                    detail: "the downloaded package is gone; inspect it again".to_string(),
                }
                .into()
            })
    }

    pub fn discard(&self, token: &str) {
        if let Some(held) = self.held.lock().unwrap().remove(token) {
            remove(&held.path);
        }
    }

    fn expire(&self) {
        self.held.lock().unwrap().retain(|_, held| {
            let live = held.fetched.elapsed() < HOLD;
            if !live {
                remove(&held.path);
            }
            live
        });
    }
}

fn remove(path: &Path) {
    if let Err(error) = std::fs::remove_file(path) {
        tracing::debug!(path = %path.display(), %error, "no held plugin package to remove");
    }
}

fn https_url(url: &str) -> Result<String> {
    let invalid = |detail: &str| {
        anyhow::Error::from(ErrorInfo::PluginInvalid {
            detail: detail.to_string(),
        })
    };
    let parsed = reqwest::Url::parse(url).map_err(|_| invalid("not a valid URL"))?;
    if parsed.scheme() != "https" || parsed.host_str().is_none() {
        return Err(invalid("only https links can be installed from"));
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(invalid("a link may not carry credentials"));
    }
    Ok(parsed.into())
}

#[cfg(test)]
mod tests;
