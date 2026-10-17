use proto::backup::{BackupDone, BackupKind};
use proto::job::Job;
use proto::minecraft::ProvisionProgress;

use super::job::{Jobs, Spec};
use crate::runtime::Claim;

/// One backup or restore job for one server — what `Jobs::start_backup` runs
/// off-thread. Backups are a server feature; instances have none.
pub enum BackupJob {
    /// `scheduled_retention` is set for a scheduled backup: it is archived as
    /// one, then the scheduled archives beyond that many are pruned.
    ServerBackup {
        server_id: String,
        live: bool,
        scheduled_retention: Option<usize>,
    },
    ServerRestore {
        server_id: String,
        backup: String,
    },
}

impl BackupJob {
    fn id_prefix(&self) -> &'static str {
        match self {
            BackupJob::ServerBackup { .. } => "server-backup",
            BackupJob::ServerRestore { .. } => "server-restore",
        }
    }
}

impl Jobs {
    /// Start a backup/restore job off-thread, holding the server's claim until
    /// it settles.
    pub fn start_backup<C>(&self, job: BackupJob, id: String, claim: Claim) -> String
    where
        C: Job<Progress = ProvisionProgress, Done = BackupDone>,
    {
        let spec = Spec {
            id,
            prefix: job.id_prefix(),
            claim: Some(claim),
        };

        self.run::<C, _>(spec, move |engine, reporter| {
            Box::pin(async move {
                let running = reporter.job();
                let backup = match job {
                    BackupJob::ServerBackup {
                        server_id,
                        live,
                        scheduled_retention,
                    } => {
                        let kind = match scheduled_retention {
                            Some(_) => BackupKind::Scheduled,
                            None => BackupKind::Manual,
                        };
                        let backup = engine
                            .backup_server(&server_id, kind, live, &running)
                            .await?;
                        if let Some(retention) = scheduled_retention {
                            match engine.prune_server_backups(&server_id, retention) {
                                Ok(pruned) if !pruned.is_empty() => {
                                    tracing::info!(server = %server_id, pruned = pruned.len(), "old scheduled backups pruned");
                                }
                                Ok(_) => {}
                                Err(e) => {
                                    tracing::warn!(server = %server_id, error = format!("{e:#}"), "backup prune failed");
                                }
                            }
                        }
                        backup
                    }
                    BackupJob::ServerRestore { server_id, backup } => {
                        engine
                            .restore_server_backup(&server_id, &backup, &running)
                            .await?
                    }
                };
                Ok(BackupDone { backup })
            })
        })
    }
}
