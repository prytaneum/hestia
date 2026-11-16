use std::path::{Component, Path, PathBuf};

use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use super::bindings::hestia::kernel::download;
use super::bindings::hestia::kernel::types::PluginError;
use super::host::State;
use crate::vetting::{self, Failure, Limits, Refusal};

const PERMISSION: &str = "network:connect";
const PATH_BYTES: usize = 1024;

fn denied() -> PluginError {
    PluginError::Denied(PERMISSION.to_owned())
}

/// `path` as a place under `data`: relative, no `..`, and its directory, once
/// links are resolved, still inside `data`.
fn destination(data: &Path, path: &str) -> Result<PathBuf, PluginError> {
    let invalid = || PluginError::Invalid("path".into());
    let relative = Path::new(path);
    let plain = relative
        .components()
        .all(|part| matches!(part, Component::Normal(_)));
    if path.is_empty() || path.len() > PATH_BYTES || !plain {
        return Err(invalid());
    }
    let target = data.join(relative);
    let (Some(parent), Some(name)) = (target.parent(), target.file_name()) else {
        return Err(invalid());
    };
    std::fs::create_dir_all(parent).map_err(|error| PluginError::Failed(error.to_string()))?;
    let parent = parent
        .canonicalize()
        .map_err(|error| PluginError::Failed(error.to_string()))?;
    let root = data
        .canonicalize()
        .map_err(|error| PluginError::Failed(error.to_string()))?;
    if !parent.starts_with(&root) {
        tracing::warn!(path, "download destination leaves the plugin's data");
        return Err(invalid());
    }
    Ok(parent.join(name))
}

fn checksum(text: &str) -> Result<String, PluginError> {
    match text.len() == 64 && text.bytes().all(|b| b.is_ascii_hexdigit()) {
        true => Ok(text.to_ascii_lowercase()),
        false => Err(PluginError::Invalid("sha256".into())),
    }
}

fn refused(refusal: Refusal) -> PluginError {
    match refusal {
        Refusal::BadUrl | Refusal::BadRedirect => PluginError::Invalid("url".into()),
        Refusal::TooLarge(_) | Refusal::TooManyRedirects => {
            PluginError::Failed(refusal.to_string())
        }
        _ => denied(),
    }
}

impl download::Host for State {
    async fn fetch(
        &mut self,
        url: String,
        path: String,
        sha256: Option<String>,
    ) -> Result<u64, PluginError> {
        let wanted = sha256.as_deref().map(checksum).transpose()?;
        let data = self
            .shared
            .services
            .data_dir(&self.plugin)
            .ok_or(PluginError::Unsupported)?;
        let target = destination(&data, &path)?;
        let plugin = self.plugin.as_str();
        let limits = Limits {
            time: None,
            ..Limits::DOWNLOAD
        };
        let response = self
            .reach
            .policy
            .get(&url, wanted.is_none(), limits, |request| request.send())
            .await
            .map_err(|failure| match failure {
                Failure::Refused(refusal) => {
                    tracing::warn!(plugin, %refusal, "plugin download refused");
                    refused(refusal)
                }
                Failure::Send(error) if vetting::was_refused(&error) => {
                    tracing::warn!(plugin, "plugin download resolved to a non-public address");
                    denied()
                }
                Failure::Send(error) => PluginError::Upstream(error.without_url().to_string()),
            })?;
        if !response.status().is_success() {
            return Err(PluginError::Upstream(format!("HTTP {}", response.status())));
        }
        let written = write(response, &target, limits, wanted.as_deref()).await;
        if let Err(error) = &written {
            tracing::warn!(plugin, ?error, "plugin download failed");
        } else {
            tracing::info!(plugin, "plugin download saved");
        }
        written
    }
}

/// Streams the body to `<target>.part` and renames it into place once the size
/// and checksum hold, so a refused or failed body leaves nothing at `target`.
/// The staged file is created fresh, never through a link the plugin planted.
async fn write(
    mut response: reqwest::Response,
    target: &Path,
    limits: Limits,
    wanted: Option<&str>,
) -> Result<u64, PluginError> {
    let failed = |error: std::io::Error| PluginError::Failed(error.to_string());
    if response
        .content_length()
        .is_some_and(|length| length > limits.bytes)
    {
        return Err(refused(Refusal::TooLarge(limits.bytes)));
    }
    let part = common::atomic::staged(target);
    match std::fs::remove_file(&part) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(failed(error)),
        _ => {}
    }
    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&part)
        .await
        .map_err(failed)?;
    let outcome = async {
        let mut hash = Sha256::new();
        let mut size = 0u64;
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|error| PluginError::Upstream(error.without_url().to_string()))?
        {
            size += chunk.len() as u64;
            if size > limits.bytes {
                return Err(refused(Refusal::TooLarge(limits.bytes)));
            }
            hash.update(&chunk);
            file.write_all(&chunk).await.map_err(failed)?;
        }
        file.flush().await.map_err(failed)?;
        let digest: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
        match wanted {
            Some(wanted) if wanted != digest => {
                Err(PluginError::Failed("checksum mismatch".into()))
            }
            _ => Ok(size),
        }
    }
    .await;
    drop(file);
    match outcome {
        Ok(size) => tokio::fs::rename(&part, target)
            .await
            .map(|()| size)
            .map_err(failed),
        Err(error) => {
            let _ = tokio::fs::remove_file(&part).await;
            Err(error)
        }
    }
}
