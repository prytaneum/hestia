//! `hestia server backup create|list|restore|remove` — the per-server backup
//! surface. Create and restore render live progress; restore confirms before
//! replacing the current data. Scheduled backups are `server task backup`.

use anyhow::Result;
use clap::Subcommand;
use client::Client;

use super::entry;
use crate::commands::mc;
use crate::ui::{self, ProvisionReporter, View};

#[derive(Subcommand)]
pub enum BackupCmd {
    /// Archive the server's data (a running server keeps running; its world
    /// saving pauses during the archive)
    Create,
    /// Stored backups, newest first
    #[command(visible_alias = "ls")]
    List,
    /// Replace a stopped server's data with a backup's content (the current
    /// jar and libraries are kept)
    Restore {
        /// Backup id (prompts when omitted)
        backup: Option<String>,
        #[arg(long, help = "Replace the current data without confirming")]
        force: bool,
    },
    /// Delete a backup
    #[command(visible_alias = "rm")]
    Remove {
        /// Backup id (prompts when omitted)
        backup: Option<String>,
    },
}

pub(super) async fn run(client: &Client, server: &str, cmd: BackupCmd) -> Result<()> {
    match cmd {
        BackupCmd::Create => {
            let info = entry::pick_server(client.server().list().await?, Some(server.to_string()))?;
            let backup = ProvisionReporter::run(Some(mc::backup_phase()), async |on_progress| {
                client.server().backup_create(&info.id, on_progress).await
            })
            .await?;
            ui::show(View::line(format!(
                "backup '{}' of '{}' created ({})",
                backup.id,
                info.name,
                ui::human_bytes(backup.size)
            )))
        }
        BackupCmd::List => {
            let info = entry::pick_server(client.server().list().await?, Some(server.to_string()))?;
            let backups = client.server().backup_list(&info.id).await?;
            if backups.is_empty() {
                return ui::show(View::note(format!(
                    "no backups yet (hestia server {server} backup create)"
                )));
            }
            mc::show_backups(format!("{} backups", info.name), backups)
        }
        BackupCmd::Restore { backup, force } => {
            let info = entry::pick_server(client.server().list().await?, Some(server.to_string()))?;
            let backups = client.server().backup_list(&info.id).await?;
            let backup = mc::pick_backup(backups, backup)?;
            if !force {
                mc::confirm_restore(&info.name, "world and settings", &backup)?;
            }
            ProvisionReporter::run(Some(mc::restore_phase()), async |on_progress| {
                client
                    .server()
                    .backup_restore(&info.id, &backup.id, on_progress)
                    .await
            })
            .await?;
            ui::show(View::line(format!(
                "backup '{}' restored onto '{}'",
                backup.id, info.name
            )))
        }
        BackupCmd::Remove { backup } => {
            let info = entry::pick_server(client.server().list().await?, Some(server.to_string()))?;
            let backups = client.server().backup_list(&info.id).await?;
            let backup = mc::pick_backup(backups, backup)?;
            client.server().backup_remove(&info.id, &backup.id).await?;
            ui::show(View::line(format!("backup '{}' removed", backup.id)))
        }
    }
}
