//! Server backups: the archive/restore passes and the RCON save-off/save-on
//! dance around a live server. Backups are a server feature — instances have
//! none (import/export is the intended replacement, not yet built); the daemon
//! admits one job per entry at a time.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use proto::backup::{BackupInfo, BackupKind};
use proto::minecraft::{Edition, ProvisionPhase, ProvisionProgress};

use crate::backup;
use crate::edition;
use crate::engine::Engine;
use crate::hold::Truncation;
use crate::minecraft::bedrock;
use crate::minecraft::materialize::OnProgress;
use crate::minecraft::profile::ServerRuntime;
use crate::servers::ServerRecord;

impl Engine {
    /// Archive a server's `data/` into its `backups/`. With `live` (the
    /// caller observed the server running) world saving is paused around the
    /// archive — save-off / save-all flush over RCON for Java, the documented
    /// `save hold` / `save query` / `save resume` protocol for Bedrock — and
    /// always resumes, even when archiving fails (the docker-mc-backup dance).
    pub async fn backup_server(
        &self,
        reference: &str,
        kind: BackupKind,
        live: bool,
        on_progress: OnProgress<'_>,
    ) -> Result<BackupInfo> {
        let record = self.ready_server(reference)?;
        let edition = record.edition;
        let archive = |truncate| {
            run_backup(
                self.servers.server_dir(&record),
                self.servers.data_dir(&record),
                kind,
                server_backup_excludes(&record, &self.servers.data_dir(&record)),
                truncate,
                on_progress,
            )
        };
        if !live {
            return archive(Truncation::new()).await;
        }
        with_saves_paused(
            self.pause_world_saves(&record.id, edition),
            self.resume_world_saves(&record.id, edition),
            archive,
        )
        .await
    }

    /// Replace a stopped server's `data/` with a backup's content. The jar and
    /// libraries of the record's current version carry over — a backup holds
    /// the world and configuration, not the re-materialisable binaries.
    pub async fn restore_server_backup(
        &self,
        reference: &str,
        backup: &str,
        on_progress: OnProgress<'_>,
    ) -> Result<BackupInfo> {
        let record = self.ready_server(reference)?;
        run_restore(
            self.servers.server_dir(&record),
            self.servers.data_dir(&record),
            backup.to_string(),
            server_backup_excludes(&record, &self.servers.data_dir(&record)),
            on_progress,
        )
        .await
    }

    /// A server's stored backups, newest first.
    pub fn server_backups(&self, reference: &str) -> Result<Vec<BackupInfo>> {
        let record = self.servers.require(reference)?;
        Ok(backup::list(&self.servers.server_dir(&record)))
    }

    /// Delete one server backup. Returns false when no backup matches.
    pub fn remove_server_backup(&self, reference: &str, backup: &str) -> Result<bool> {
        let record = self.servers.require(reference)?;
        backup::remove(&self.servers.server_dir(&record), backup)
    }

    /// Prune a server's *scheduled* backups beyond `retention` (manual and
    /// pre-update backups are kept until removed explicitly). Returns what was
    /// removed.
    pub fn prune_server_backups(
        &self,
        reference: &str,
        retention: usize,
    ) -> Result<Vec<BackupInfo>> {
        let record = self.servers.require(reference)?;
        backup::prune(
            &self.servers.server_dir(&record),
            BackupKind::Scheduled,
            retention,
        )
    }

    /// Pause world writes before archiving a live server; the lengths to copy
    /// each file to, empty when the edition needs none.
    async fn pause_world_saves(&self, server: &str, edition: Edition) -> Result<Truncation> {
        edition::pause_saves(self, edition, server).await
    }

    /// Resuming must reach the server even when archiving failed, or the world
    /// stops persisting — retry like docker-mc-backup's exit trap does.
    async fn resume_world_saves(&self, server: &str, edition: Edition) -> Result<()> {
        let command = edition::resume_command(edition);
        let mut last = anyhow::anyhow!("the console is unreachable");
        for attempt in 0..5 {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
            match self.server_command(server, command).await {
                Ok(_) => {
                    tracing::info!(server, command, "world saving resumed");
                    return Ok(());
                }
                Err(e) => last = e,
            }
        }
        Err(last.context("cannot re-enable world saving"))
    }
}

/// Run `work` between a pause and its resume. The resume runs whatever
/// happened — a failed pause may have half-applied — and its failure is
/// reported even when the work succeeded.
async fn with_saves_paused<T, W, Fut>(
    pause: impl Future<Output = Result<Truncation>>,
    resume: impl Future<Output = Result<()>>,
    work: W,
) -> Result<T>
where
    W: FnOnce(Truncation) -> Fut,
    Fut: Future<Output = Result<T>>,
{
    let result = match pause.await {
        Ok(truncate) => work(truncate).await,
        Err(e) => Err(e),
    };
    if let Err(e) = resume.await {
        tracing::error!(error = format!("{e:#}"), "world saving is still disabled");
        if result.is_ok() {
            return Err(e.context(
                "the backup was created, but world saving could not be re-enabled \
                 (re-enable it from the server console)",
            ));
        }
    }
    result
}

/// What a server backup skips and a restore carries over: content the
/// launcher re-materialises for the record's *current* version (jar,
/// libraries) plus logs and cache — the docker-mc-backup default set, whose
/// `cache` entry also covers the vanilla jar a paperclip build unpacks on its
/// first run — and the managed content mirrors (`mods/`, `plugins/`), which the
/// sync pass re-creates from the entry root at the next start. Transient
/// `session.lock` files are omitted by the archive walker at every depth.
fn server_backup_excludes(record: &ServerRecord, data: &Path) -> Vec<String> {
    let installed = match record.profile.runtime {
        ServerRuntime::Java { .. } => vec![record.profile.runtime.filename().to_string()],
        ServerRuntime::Native { .. } => bedrock::binaries(data),
    };
    installed
        .into_iter()
        .chain([
            "libraries".into(),
            "logs".into(),
            "cache".into(),
            "mods".into(),
            "plugins".into(),
        ])
        .collect()
}

/// Run a blocking archive pass off-thread, forwarding its per-file ticks to
/// `on_progress` as `phase` progress. Cancellation is checked per tick: a
/// backup discards its `.part` and a restore extracts into staging before the
/// swap, so stopping between files leaves nothing half-written.
async fn blocking_pass<T: Send + 'static>(
    phase: ProvisionPhase,
    panic_text: &'static str,
    on_progress: OnProgress<'_>,
    work: impl FnOnce(crate::backup::OnProgress<'_>) -> Result<T> + Send + 'static,
) -> Result<T> {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let cancel = on_progress.cancel().clone();
    let task = tokio::task::spawn_blocking(move || {
        work(&move |current, total| {
            if cancel.is_cancelled() {
                return Err(crate::cancel::Cancelled.into());
            }
            let _ = tx.send((current, total));
            Ok(())
        })
    });
    while let Some((current, total)) = rx.recv().await {
        on_progress.report(&progress_in(phase, current, total));
    }
    task.await.context(panic_text)?
}

async fn run_backup(
    entry_dir: PathBuf,
    data_dir: PathBuf,
    kind: BackupKind,
    exclude: Vec<String>,
    truncate: Truncation,
    on_progress: OnProgress<'_>,
) -> Result<BackupInfo> {
    blocking_pass(
        ProvisionPhase::Backup,
        "the backup task panicked",
        on_progress,
        move |tick| backup::create(&entry_dir, &data_dir, kind, &exclude, &truncate, tick),
    )
    .await
}

async fn run_restore(
    entry_dir: PathBuf,
    data_dir: PathBuf,
    backup: String,
    preserve: Vec<String>,
    on_progress: OnProgress<'_>,
) -> Result<BackupInfo> {
    blocking_pass(
        ProvisionPhase::Extract,
        "the restore task panicked",
        on_progress,
        move |tick| backup::restore(&entry_dir, &data_dir, &backup, &preserve, tick),
    )
    .await
}

fn progress_in(phase: ProvisionPhase, current: u64, total: u64) -> ProvisionProgress {
    ProvisionProgress {
        phase,
        current,
        total,
        ..ProvisionProgress::default()
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    #[tokio::test]
    async fn a_failing_archive_still_resumes() {
        let resumed = Cell::new(false);
        let result: Result<()> = with_saves_paused(
            async { Ok(Truncation::new()) },
            async {
                resumed.set(true);
                Ok(())
            },
            |_| async { Err(anyhow::anyhow!("disk full")) },
        )
        .await;
        assert_eq!(result.unwrap_err().to_string(), "disk full");
        assert!(resumed.get());
    }

    #[tokio::test]
    async fn a_failing_pause_resumes_without_archiving() {
        let resumed = Cell::new(false);
        let result: Result<()> = with_saves_paused(
            async { Err(crate::hold::HoldError::NotReady(30).into()) },
            async {
                resumed.set(true);
                Ok(())
            },
            |_| async { panic!("must not archive") },
        )
        .await;
        assert!(result.is_err());
        assert!(resumed.get());
    }

    #[tokio::test]
    async fn a_failed_resume_is_reported_after_a_good_backup() {
        let result = with_saves_paused(
            async { Ok(Truncation::new()) },
            async { Err(anyhow::anyhow!("console gone")) },
            |_| async { Ok(1) },
        )
        .await;
        assert!(result.is_err());
    }
}
