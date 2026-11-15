//! What a plugin's `download` import does: the engine fetches a file to a path
//! the call may write, so the plugin never holds the body. The
//! transfer is the engine's own: the guarded [`Policy`] holds every redirect
//! hop to the grant, a checksum is verified before the file appears, the cache
//! serves a repeat, and progress reaches the call that asked.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use proto::download::{Checksum, DownloadProgress};
use proto::plugin::PluginRunProgress;
use reqwest::Url;
use tokio::time::Instant;

use super::error::GuestError;
use super::imports::CommandRun;
use super::vouch::Policy;
use crate::cache::Cache;
use crate::download::Downloader;

/// The most a single download may write, enforced while the body streams. It
/// is meant to cover a server jar or an installer and still bound what a
/// plugin can fill a disk with in one call.
pub const MAX_DOWNLOAD_BYTES: u64 = 4 << 30;

const CANCEL_POLL: Duration = Duration::from_millis(100);
const UNKNOWN_TOTAL_STEP: u64 = 1 << 20;
const PROGRESS_STEPS: u64 = 1000;

/// One plugin's way to have the engine fetch a file: its grant's [`Policy`] and
/// the engine's download cache, when the daemon lent one.
pub(crate) struct Downloads {
    policy: Arc<Policy>,
    cache: Option<Arc<Cache>>,
}

/// What bounds one download: the instant the call's budget ends and, for a
/// call that can be cancelled and shows progress, its [`CommandRun`].
pub(crate) struct DownloadScope<'a> {
    pub deadline: Instant,
    pub run: Option<&'a CommandRun>,
}

impl Downloads {
    pub(crate) fn new(policy: Policy, cache: Option<Arc<Cache>>) -> Self {
        Downloads {
            policy: Arc::new(policy),
            cache,
        }
    }

    /// Whether the grant lets `url` be fetched, for a plan checked before any
    /// of it runs; `hashed` is whether the body is verified against a checksum.
    pub(crate) fn vet(&self, url: &str, hashed: bool) -> Result<(), GuestError> {
        self.policy
            .vet(url, !hashed)
            .map_err(|refused| GuestError::Denied(refused.to_string()))
    }

    /// Fetches `url` to `destination` and returns the size written. `destination`
    /// is already resolved to a writable file of the call. With no checksum the
    /// body is unverified, so every hop must be on a host the grant declares as
    /// serving unhashed files, and there is nothing to key the cache on.
    pub(crate) async fn fetch(
        &self,
        url: &str,
        expect: Option<Checksum>,
        destination: &Path,
        scope: &DownloadScope<'_>,
    ) -> Result<u64, GuestError> {
        let expect = expect.map(normalised).transpose()?;
        self.policy
            .vet(url, expect.is_none())
            .map_err(|refused| GuestError::Denied(refused.to_string()))?;
        let name = destination
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let place = place(url);
        tracing::debug!(%place, hashed = expect.is_some(), "plugin download");

        let reporter = Reporter::new(scope.run, &name);
        let downloader = Downloader::new(self.cache.as_deref())
            .vouched(Some(self.policy.clone()))
            .limited(MAX_DOWNLOAD_BYTES);
        let on_progress = |progress: &DownloadProgress| {
            reporter.report(progress);
            Ok(())
        };
        let transfer = downloader.fetch(url, destination, expect.as_ref(), &on_progress);
        let outcome = tokio::select! {
            outcome = transfer => outcome.map_err(|error| {
                let failure = GuestError::fetch(&error);
                tracing::warn!(%place, kind = failure.kind(), "plugin download failed");
                failure
            }),
            stopped = stopped(scope) => {
                tracing::info!(%place, "plugin download stopped");
                Err(stopped)
            }
        };
        if outcome.is_err() {
            let _ = tokio::fs::remove_file(common::atomic::staged(destination)).await;
        }
        outcome?;
        let size = tokio::fs::metadata(destination)
            .await
            .map_err(|error| {
                tracing::warn!(%place, %error, "downloaded file is unreadable");
                GuestError::Failed("the downloaded file could not be read back".into())
            })?
            .len();
        Ok(size)
    }
}

fn normalised(checksum: Checksum) -> Result<Checksum, GuestError> {
    match checksum.is_valid() {
        true => Ok(Checksum {
            hex: checksum.hex.to_ascii_lowercase(),
            ..checksum
        }),
        false => Err(GuestError::Invalid(format!(
            "a {} checksum is {} hex characters",
            checksum.algorithm.as_str(),
            checksum.algorithm.hex_digest_length()
        ))),
    }
}

async fn stopped(scope: &DownloadScope<'_>) -> GuestError {
    loop {
        if scope.run.is_some_and(|run| run.cancel.is_cancelled()) {
            return GuestError::Failed("the call was cancelled".into());
        }
        if Instant::now() >= scope.deadline {
            return GuestError::Failed("the call ran out of time".into());
        }
        tokio::time::sleep(CANCEL_POLL.min(scope.deadline - Instant::now())).await;
    }
}

/// Host and path, never the query: a URL may carry a token there.
fn place(url: &str) -> String {
    Url::parse(url)
        .map(|url| format!("{}{}", url.host_str().unwrap_or_default(), url.path()))
        .unwrap_or_default()
}

/// Turns the downloader's per-chunk callbacks into a few reports into the
/// call's progress: one per thousandth of a known size, or per MiB of an
/// unknown one.
struct Reporter<'a> {
    run: Option<&'a CommandRun>,
    message: String,
    last: AtomicU64,
}

impl<'a> Reporter<'a> {
    fn new(run: Option<&'a CommandRun>, name: &str) -> Self {
        Reporter {
            run,
            message: format!("downloading {name}"),
            last: AtomicU64::new(u64::MAX),
        }
    }

    fn report(&self, progress: &DownloadProgress) {
        let Some(run) = self.run else { return };
        let step = match progress.total {
            0 => progress.downloaded / UNKNOWN_TOTAL_STEP,
            total => progress.downloaded.saturating_mul(PROGRESS_STEPS) / total,
        };
        if self.last.swap(step, Ordering::Relaxed) == step {
            return;
        }
        run.progress.send_replace(Some(PluginRunProgress {
            fraction: (progress.total > 0)
                .then(|| (progress.downloaded as f64 / progress.total as f64).min(1.0) as f32),
            message: self.message.clone(),
        }));
    }
}

#[cfg(test)]
mod tests;
