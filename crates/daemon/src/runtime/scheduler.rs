//! What the daemon does on a clock, checked once a minute: scheduled backups,
//! then each server's scheduled tasks (a restart, a console command).
//!
//! The scheduled-backup pass: archive each *running* server
//! whose `backup-interval` has elapsed since its newest backup (of any kind —
//! a fresh manual or pre-update archive resets the clock), then prune its
//! scheduled archives beyond `backup-retention`. A stopped server's world
//! cannot change, so it is never re-archived on schedule; the pre-update and
//! on-demand backups cover it.

use std::sync::Arc;
use std::time::Duration;

use proto::backup::BackupKind;
use proto::process::ProcessState;
use proto::tasks::TaskAction;

use super::{now_unix, server_process_id, JobKind, Runtime};

const TICK: Duration = Duration::from_secs(60);

pub fn spawn_backup_scheduler(runtime: Arc<Runtime>) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(TICK);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            run_due_backups(&runtime).await;
            run_due_tasks(&runtime).await;
        }
    });
}

/// Run each running server's scheduled tasks that have come due. A task is
/// stamped before it runs, so one that fails waits for its next slot instead
/// of retrying every tick.
async fn run_due_tasks(runtime: &Runtime) {
    let engine = runtime.engine();
    let now = now_unix();
    let midnight = common::local_midnight_unix();
    for record in engine.servers().list() {
        let process_id = server_process_id(&record.id);
        if !record.ready() {
            continue;
        }
        for task in record.tasks.iter().filter(|task| task.enabled) {
            let due = engine::Schedule::parse(&task.schedule)
                .is_ok_and(|schedule| schedule.due(task.last_run_unix, now, midnight));
            if !due || !is_running(runtime, &process_id) {
                continue;
            }
            // A backup or a version change owns the tree; a restart under it
            // would corrupt one or the other, so the task waits for a later tick.
            let Ok(_claim) = runtime.claims().claim(process_id.clone(), JobKind::Launch) else {
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
                    .restart_server(&record)
                    .await
                    .map(|_| ())
                    .map_err(|e| e.to_string()),
            };
            if let Err(error) = outcome {
                tracing::warn!(server = %record.id, task = %task.id, error, "scheduled task failed");
            }
        }
    }
}

fn is_running(runtime: &Runtime, process_id: &str) -> bool {
    runtime
        .processes()
        .status(process_id)
        .is_some_and(|p| p.state == ProcessState::Running)
}

async fn run_due_backups(runtime: &Runtime) {
    let engine = runtime.engine();
    for record in engine.servers().list() {
        let Some(interval) = record.backup.interval() else {
            continue;
        };
        if !record.ready() {
            continue;
        }
        if !is_running(runtime, &server_process_id(&record.id)) {
            continue;
        }
        let Ok(_claim) = runtime
            .claims()
            .claim(server_process_id(&record.id), JobKind::Backup)
        else {
            continue;
        };
        let newest = engine
            .server_backups(&record.id)
            .ok()
            .and_then(|backups| backups.first().map(|b| b.created_unix));
        let due = newest.is_none_or(|t| now_unix().saturating_sub(t) >= interval.as_secs() as i64);
        if !due {
            continue;
        }

        tracing::info!(server = %record.id, "scheduled backup starting");
        match engine
            .backup_server(
                &record.id,
                BackupKind::Scheduled,
                true,
                // A scheduled backup has no caller to cancel it.
                &engine::Job::new(&|_| {}, &engine::Cancel::new()),
            )
            .await
        {
            Ok(backup) => {
                tracing::info!(
                    server = %record.id,
                    backup = %backup.id,
                    size = backup.size,
                    "scheduled backup done"
                );
                match engine.prune_server_backups(&record.id) {
                    Ok(pruned) if !pruned.is_empty() => {
                        tracing::info!(server = %record.id, pruned = pruned.len(), "old scheduled backups pruned");
                    }
                    Ok(_) => {}
                    Err(e) => {
                        tracing::warn!(server = %record.id, error = format!("{e:#}"), "backup prune failed");
                    }
                }
            }
            Err(e) => {
                tracing::warn!(server = %record.id, error = format!("{e:#}"), "scheduled backup failed");
            }
        }
    }
}
