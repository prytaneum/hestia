use std::path::{Path, PathBuf};

use cap_std::ambient_authority;
use cap_std::fs::{Dir, OpenOptions};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use super::bindings::hestia::kernel::download;
use super::bindings::hestia::kernel::types::PluginError;
use super::host::State;
use super::shared::Downloading;
use crate::vetting::{self, Failure, Limits, Refusal};

const PERMISSION: &str = "network:connect";
const PATH_BYTES: usize = 1024;

fn denied() -> PluginError {
    PluginError::Denied(PERMISSION.to_owned())
}

const FORBIDDEN: [char; 8] = ['<', '>', ':', '"', '\\', '|', '?', '*'];
const DEPTH_MAX: usize = 64;

/// `path` as a place under `/data`: relative, no `..`, and every name valid on
/// every platform the daemon runs on, so a plugin behaves the same everywhere.
/// Links the plugin plants inside `/data` are never followed out of it: every
/// step runs through a [`Dir`] handle on `/data`, which refuses a link that
/// leaves it.
fn relative(path: &str) -> Result<PathBuf, PluginError> {
    let invalid = || PluginError::Invalid("path".into());
    if path.is_empty() || path.len() > PATH_BYTES || path.starts_with('/') {
        return Err(invalid());
    }
    let mut clean = PathBuf::new();
    for name in path.split('/').filter(|name| !matches!(*name, "" | ".")) {
        if !portable(name) {
            return Err(invalid());
        }
        clean.push(name);
    }
    match clean.as_os_str().is_empty() {
        true => Err(invalid()),
        false => Ok(clean),
    }
}

/// A name Windows would also accept: no reserved characters, no device name
/// (with or without an extension), no trailing dot or space, which it drops.
fn portable(name: &str) -> bool {
    !name
        .chars()
        .any(|c| c.is_control() || FORBIDDEN.contains(&c))
        && !name.ends_with(['.', ' '])
        && !device(name)
}

fn device(name: &str) -> bool {
    let stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end_matches(' ')
        .to_lowercase();
    if matches!(
        stem.as_str(),
        "con" | "prn" | "aux" | "nul" | "conin$" | "conout$"
    ) {
        return true;
    }
    let numbered = stem
        .strip_prefix("com")
        .or_else(|| stem.strip_prefix("lpt"));
    numbered.is_some_and(|rest| {
        let mut digits = rest.chars();
        matches!(
            digits.next(),
            Some('0'..='9' | '\u{b9}' | '\u{b2}' | '\u{b3}')
        ) && digits.next().is_none()
    })
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
        let failed = |error: std::io::Error| match is_escape(&error) {
            true => {
                tracing::warn!(?target, "download destination leaves the plugin's data");
                PluginError::Invalid("path".into())
            }
            false => {
                tracing::warn!(?target, %error, "cannot stage a download");
                PluginError::Failed(error.to_string())
            }
        };
        let dir = Dir::open_ambient_dir(data, ambient_authority()).map_err(failed)?;
        if let Some(parent) = target.parent().filter(|p| !p.as_os_str().is_empty()) {
            dir.create_dir_all(parent).map_err(failed)?;
        }
        let part = common::atomic::staged(&target);
        match common::fsys::retry(|| dir.remove_file(&part)) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                return Err(failed(error))
            }
            _ => {}
        }
        let file = common::fsys::retry(|| {
            dir.open_with(&part, OpenOptions::new().write(true).create_new(true))
        })
        .map_err(failed)?;
        let staged = Staged {
            dir,
            part,
            target,
            committed: false,
        };
        Ok((staged, file.into_std()))
    }

    /// How many more bytes `/data` may take before [`Limits::DATA_QUOTA`],
    /// counting what the target now holds as freed. Blocking.
    fn headroom(&self) -> std::io::Result<u64> {
        let replaced = self.dir.symlink_metadata(&self.target).map_or(0, |found| {
            if found.is_file() {
                found.len()
            } else {
                0
            }
        });
        let held = stored(&self.dir, 0)?.saturating_sub(replaced);
        Ok(Limits::DATA_QUOTA.saturating_sub(held))
    }

    /// Renames the staging file into place. Blocking.
    pub(super) fn commit(mut self) -> std::io::Result<()> {
        common::fsys::retry(|| self.dir.rename(&self.part, &self.dir, &self.target))?;
        self.committed = true;
        Ok(())
    }
}

impl Drop for Staged {
    fn drop(&mut self) {
        if !self.committed {
            let _ = common::fsys::retry(|| self.dir.remove_file(&self.part));
        }
    }
}

fn stored(dir: &Dir, depth: usize) -> std::io::Result<u64> {
    if depth > DEPTH_MAX {
        return Err(std::io::Error::other(
            "the data directory is nested too deeply",
        ));
    }
    let mut total = 0u64;
    for entry in dir.entries()? {
        let entry = entry?;
        let counted = match entry.file_type()? {
            kind if kind.is_file() => entry.metadata().map(|found| found.len()),
            kind if kind.is_dir() => entry
                .open_dir()
                .and_then(|nested| stored(&nested, depth + 1)),
            _ => Ok(0),
        };
        match counted {
            Ok(bytes) => total = total.saturating_add(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(total)
}

/// cap-std refuses a path that leaves the directory with a bare
/// `PermissionDenied`; an operating-system refusal always carries its code.
fn is_escape(error: &std::io::Error) -> bool {
    error.kind() == std::io::ErrorKind::PermissionDenied && error.raw_os_error().is_none()
}

/// What one download has written, counted against the plugin's `/data` quota
/// together with its other downloads in flight until it ends.
struct Reservation {
    downloading: Downloading,
    plugin: String,
    room: u64,
    taken: u64,
}

impl Reservation {
    fn take(&mut self, bytes: u64) -> bool {
        let mut downloading = self
            .downloading
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let total = downloading.entry(self.plugin.clone()).or_default();
        *total = total.saturating_add(bytes);
        self.taken = self.taken.saturating_add(bytes);
        *total <= self.room
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        let mut downloading = self
            .downloading
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(total) = downloading.get_mut(&self.plugin) {
            *total = total.saturating_sub(self.taken);
            if *total == 0 {
                downloading.remove(&self.plugin);
            }
        }
    }
}

fn over_quota() -> PluginError {
    PluginError::Failed("the plugin's data quota is used up".into())
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
        let tally = (self.shared.downloading.clone(), self.plugin.clone());
        let written = write(response, data, target, limits, wanted.as_deref(), tally).await;
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
    (downloading, plugin): (Downloading, String),
) -> Result<u64, PluginError> {
    let failed = |error: std::io::Error| PluginError::Failed(error.to_string());
    if response
        .content_length()
        .is_some_and(|length| length > limits.bytes)
    {
        return Err(refused(Refusal::TooLarge(limits.bytes)));
    }
    let (staged, file, room) = tokio::task::spawn_blocking(move || {
        let (staged, file) = Staged::create(&data, target)?;
        let room = staged.headroom().map_err(failed)?;
        Ok::<_, PluginError>((staged, file, room))
    })
    .await
    .map_err(|error| PluginError::Failed(error.to_string()))??;
    if response
        .content_length()
        .is_some_and(|length| length > room)
    {
        return Err(over_quota());
    }
    let mut reservation = Reservation {
        downloading,
        plugin,
        room,
        taken: 0,
    };
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
        if !reservation.take(chunk.len() as u64) {
            return Err(over_quota());
        }
        hash.update(&chunk);
        file.write_all(&chunk).await.map_err(failed)?;
    }
    file.flush().await.map_err(failed)?;
    drop(file);
    let digest: String = hex::encode(hash.finalize());
    if wanted.is_some_and(|wanted| wanted != digest) {
        return Err(PluginError::Failed("checksum mismatch".into()));
    }
    tokio::task::spawn_blocking(move || staged.commit())
        .await
        .map_err(|error| PluginError::Failed(error.to_string()))?
        .map_err(failed)?;
    Ok(size)
}
