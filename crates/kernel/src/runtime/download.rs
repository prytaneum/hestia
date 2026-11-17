use std::path::{Component, Path, PathBuf};

use cap_std::ambient_authority;
use cap_std::fs::{Dir, OpenOptions};
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

/// `path` as a place under `/data`: relative, no `..`. Links the plugin plants
/// inside `/data` are never followed out of it: every step runs through a
/// [`Dir`] handle on `/data`, which refuses a link that leaves it.
fn relative(path: &str) -> Result<PathBuf, PluginError> {
    let plain = Path::new(path)
        .components()
        .all(|part| matches!(part, Component::Normal(_)));
    match path.is_empty() || path.len() > PATH_BYTES || !plain {
        true => Err(PluginError::Invalid("path".into())),
        false => Ok(PathBuf::from(path)),
    }
}

/// The staged file of a download. Unless it is committed, dropping it unlinks
/// the staging file, however the download ended.
pub(super) struct Staged {
    dir: Dir,
    part: PathBuf,
    target: PathBuf,
    committed: bool,
}

impl Staged {
    /// Creates the parent directories of `target` and a fresh staging file
    /// beside it, all inside `data`. Blocking.
    pub(super) fn create(
        data: &Path,
        target: PathBuf,
    ) -> Result<(Self, std::fs::File), PluginError> {
        let failed = |error: std::io::Error| match error.kind() {
            std::io::ErrorKind::PermissionDenied => {
                tracing::warn!(?target, "download destination leaves the plugin's data");
                PluginError::Invalid("path".into())
            }
            _ => PluginError::Failed(error.to_string()),
        };
        let dir = Dir::open_ambient_dir(data, ambient_authority()).map_err(failed)?;
        if let Some(parent) = target.parent().filter(|p| !p.as_os_str().is_empty()) {
            dir.create_dir_all(parent).map_err(failed)?;
        }
        let part = common::atomic::staged(&target);
        match dir.remove_file(&part) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                return Err(failed(error))
            }
            _ => {}
        }
        let file = dir
            .open_with(&part, OpenOptions::new().write(true).create_new(true))
            .map_err(failed)?;
        let staged = Staged {
            dir,
            part,
            target,
            committed: false,
        };
        Ok((staged, file.into_std()))
    }

    /// Renames the staging file into place. Blocking.
    pub(super) fn commit(mut self) -> std::io::Result<()> {
        self.dir.rename(&self.part, &self.dir, &self.target)?;
        self.committed = true;
        Ok(())
    }
}

impl Drop for Staged {
    fn drop(&mut self) {
        if !self.committed {
            let _ = self.dir.remove_file(&self.part);
        }
    }
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
        if !self.online {
            tracing::info!(
                plugin = self.plugin,
                "plugin download refused while offline"
            );
            return Err(PluginError::Offline);
        }
        let wanted = sha256.as_deref().map(checksum).transpose()?;
        let target = relative(&path)?;
        let data = self
            .shared
            .services
            .data_dir(&self.plugin)
            .ok_or(PluginError::Unsupported)?;
        let plugin = self.plugin.as_str();
        let limits = Limits::DOWNLOAD;
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
            })
            .inspect_err(|error| {
                if matches!(error, PluginError::Denied(_)) {
                    self.log
                        .denied("hestia:kernel/download", "fetch", PERMISSION);
                }
            })?;
        if !response.status().is_success() {
            return Err(PluginError::Upstream(format!("HTTP {}", response.status())));
        }
        let written = write(response, data, target, limits, wanted.as_deref()).await;
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
async fn write(
    mut response: reqwest::Response,
    data: PathBuf,
    target: PathBuf,
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
    let (staged, file) = tokio::task::spawn_blocking(move || Staged::create(&data, target))
        .await
        .map_err(|error| PluginError::Failed(error.to_string()))??;
    let mut file = tokio::fs::File::from_std(file);
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
    drop(file);
    let digest: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if wanted.is_some_and(|wanted| wanted != digest) {
        return Err(PluginError::Failed("checksum mismatch".into()));
    }
    tokio::task::spawn_blocking(move || staged.commit())
        .await
        .map_err(|error| PluginError::Failed(error.to_string()))?
        .map_err(failed)?;
    Ok(size)
}
