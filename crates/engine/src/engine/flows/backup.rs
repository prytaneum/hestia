//! Server backups: the archive/restore passes and the RCON save-off/save-on
//! dance around a live server. Backups are a server feature — instances have
//! none (import/export is the intended replacement, not yet built); the daemon
//! admits one job per entry at a time.

use std::path::PathBuf;

use anyhow::{Context, Result};
use proto::backup::{BackupInfo, BackupKind};
use proto::minecraft::{ProvisionPhase, ProvisionProgress};

use crate::backup;
use crate::engine::Engine;
use crate::minecraft::materialize::OnProgress;
use crate::servers::ServerRecord;

impl Engine {
    /// Archive a server's `data/` into its `backups/`. With `live` (the
    /// caller observed the server running) world saving pauses over RCON
    /// around the archive — save-off, save-all flush, tar, save-on — and
    /// always resumes, even when archiving fails (the docker-mc-backup dance).
    pub async fn backup_server(
        &self,
        reference: &str,
        kind: BackupKind,
        live: bool,
        on_progress: OnProgress<'_>,
    ) -> Result<BackupInfo> {
        let record = self.ready_server(reference)?;
        if live {
            self.pause_world_saves(&record.id).await?;
        }
        let result = run_backup(
            self.servers.server_dir(&record),
            self.servers.data_dir(&record),
            kind,
            server_backup_excludes(&record),
            on_progress,
        )
        .await;
        if live {
            if let Err(e) = self.resume_world_saves(&record.id).await {
                tracing::error!(
                    server = %record.id,
                    error = format!("{e:#}"),
                    "world saving is still disabled"
                );
                if result.is_ok() {
                    return Err(e.context(
                        "the backup was created, but world saving could not be re-enabled \
                         (run `save-on` in the server console)",
                    ));
                }
            }
        }
        result
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
            server_backup_excludes(&record),
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

    /// Pause world writes before archiving a live server: `save-off` stops
    /// autosaves, `save-all flush` forces everything pending onto disk (the
    /// reply arrives once the flush completed).
    async fn pause_world_saves(&self, server: &str) -> Result<()> {
        self.server_command(server, "save-off").await?;
        self.server_command(server, "save-all flush").await?;
        Ok(())
    }

    /// `save-on` must reach the server even when archiving failed, or the world
    /// stops persisting — retry like docker-mc-backup's exit trap does.
    async fn resume_world_saves(&self, server: &str) -> Result<()> {
        let mut last = anyhow::anyhow!("rcon unreachable");
        for attempt in 0..5 {
            if attempt > 0 {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
            match self.server_command(server, "save-on").await {
                Ok(_) => return Ok(()),
                Err(e) => last = e,
            }
        }
        Err(last.context("cannot re-enable world saving"))
    }
}

/// What a server backup skips and a restore carries over: content the
/// launcher re-materialises for the record's *current* version (jar,
/// libraries) plus logs and cache — the docker-mc-backup default set, whose
/// `cache` entry also covers the vanilla jar a paperclip build unpacks on its
/// first run — and the managed content mirrors (`mods/`, `plugins/`), which the
/// sync pass re-creates from the entry root at the next start. Transient
/// `session.lock` files are omitted by the archive walker at every depth.
fn server_backup_excludes(record: &ServerRecord) -> Vec<String> {
    vec![
        record.profile.jar.filename().to_string(),
        "libraries".into(),
        "logs".into(),
        "cache".into(),
        "mods".into(),
        "plugins".into(),
    ]
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
    on_progress: OnProgress<'_>,
) -> Result<BackupInfo> {
    blocking_pass(
        ProvisionPhase::Backup,
        "the backup task panicked",
        on_progress,
        move |tick| backup::create(&entry_dir, &data_dir, kind, &exclude, tick),
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
