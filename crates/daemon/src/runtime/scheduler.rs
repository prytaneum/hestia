//! What the daemon does on a clock, checked once a minute: each running
//! server's scheduled tasks — a restart, a console command, a backup. A stopped
//! server's world cannot change, so nothing is scheduled against it; the
//! pre-update and on-demand backups cover it.

use std::sync::Arc;
use std::time::Duration;

use engine::CallChain;
use proto::backup::ServerBackupCreate;
use proto::plugin::OutdatedParams;
use proto::tasks::TaskAction;

use super::{server_process_id, BackupJob, JobKind, Runtime};

const TICK: Duration = Duration::from_secs(60);

const PLUGIN_UPDATE_EVERY: Duration = Duration::from_secs(6 * 60 * 60);
const PLUGIN_UPDATE_FIRST: Duration = Duration::from_secs(60);

/// Scheduled archives a backup task keeps when it names no retention.
const DEFAULT_RETENTION: usize = 7;

pub fn spawn_task_scheduler(runtime: Arc<Runtime>) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(TICK);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            run_due_tasks(&runtime).await;
        }
    });
}

/// Looks for newer plugin builds a minute after start and every six hours
/// after, and publishes the list whenever it changes. Unsigned installs are
/// never asked on this clock.
pub fn spawn_plugin_update_checker(runtime: Arc<Runtime>) {
    tokio::spawn(async move {
        let start = tokio::time::Instant::now() + PLUGIN_UPDATE_FIRST;
        let mut tick = tokio::time::interval_at(start, PLUGIN_UPDATE_EVERY);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut last = None;
        loop {
            tick.tick().await;
            let outdated = match runtime
                .engine()
                .outdated_plugins(&OutdatedParams::default())
                .await
            {
                Ok(outdated) => outdated,
                Err(error) => {
                    tracing::warn!(error = format!("{error:#}"), "plugin update check failed");
                    continue;
                }
            };
            runtime.plugins().enforce_revocations().await;
            tracing::info!(
                outdated = outdated.plugins.len(),
                offline = outdated.offline,
                "plugin update check done"
            );
            if last.as_ref() != Some(&outdated.plugins) {
                runtime.hub().publish_topic(&outdated);
                last = Some(outdated.plugins);
            }
        }
    });
}

/// Run each running server's scheduled tasks that have come due. A task is
/// stamped before it runs, so one that fails waits for its next slot instead
/// of retrying every tick.
async fn run_due_tasks(runtime: &Runtime) {
    let engine = runtime.engine();
    let now = common::now_unix();
    let midnight = common::local_midnight_unix();
    for record in engine.servers().list() {
        let process_id = server_process_id(&record.id);
        if !record.ready() {
            continue;
        }
        for task in record.tasks.iter().filter(|task| task.enabled) {
            let due = engine::Schedule::parse(&task.schedule)
                .is_ok_and(|schedule| schedule.due(task.last_run_unix, now, midnight));
            if !due || !runtime.process_running(&process_id) {
                continue;
            }
            // A backup or a version change owns the tree; a task under it would
            // corrupt one or the other, so it waits for a later tick.
            let kind = match task.action {
                TaskAction::Backup => JobKind::Backup,
                TaskAction::Restart | TaskAction::Command => JobKind::Launch,
            };
            let Ok(claim) = runtime.claims().claim(process_id.clone(), kind) else {
                continue;
            };
            if let Err(e) = engine.servers().mark_task_run(&record.id, &task.id, now) {
                tracing::warn!(server = %record.id, task = %task.id, error = format!("{e:#}"), "cannot stamp a task; skipping it");
                continue;
            }
            tracing::info!(server = %record.id, task = %task.id, action = ?task.action, "scheduled task starting");
            let outcome = match task.action {
                TaskAction::Command => engine
                    .server_command(&record.id, &task.command)
                    .await
                    .map(|_| ())
                    .map_err(|e| format!("{e:#}")),
                TaskAction::Restart => runtime
                    .restart_server(&record, &CallChain::default())
                    .await
                    .map(|_| ())
                    .map_err(|e| e.to_string()),
                TaskAction::Backup => {
                    let job = BackupJob::ServerBackup {
                        server_id: record.id.clone(),
                        live: true,
                        scheduled_retention: Some(
                            task.retention.map_or(DEFAULT_RETENTION, |n| n as usize),
                        ),
                    };
                    runtime.jobs().start_backup::<ServerBackupCreate>(
                        job,
                        String::new(),
                        claim,
                        CallChain::default(),
                    );
                    Ok(())
                }
            };
            if let Err(error) = outcome {
                tracing::warn!(server = %record.id, task = %task.id, error, "scheduled task failed");
            }
        }
    }
}
