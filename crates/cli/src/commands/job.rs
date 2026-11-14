//! `hestia job …` — the daemon's running jobs.

use anyhow::Result;
use clap::Subcommand;

use crate::commands::mc::age_cell;
use crate::ui::{self, Cell, View};

#[derive(Subcommand)]
pub enum JobCmd {
    /// Running jobs
    List,
    /// Cancel a running job by id
    Cancel {
        /// The job id shown by `job list`
        id: String,
    },
}

pub async fn run(cmd: JobCmd) -> Result<()> {
    let client = super::connect().await?;
    match cmd {
        JobCmd::List => {
            let jobs = client.jobs().list().await?;
            if jobs.is_empty() {
                return ui::show(View::empty("no running jobs"));
            }
            let rows = jobs
                .into_iter()
                .map(|job| {
                    let stage = job
                        .progress
                        .as_ref()
                        .and_then(|p| p.get("phase"))
                        .and_then(|p| p.as_str())
                        .map_or_else(|| Cell::absent("-"), Cell::from);
                    let entry = job
                        .subject
                        .map_or_else(|| Cell::absent("-"), |s| Cell::from(s.id));
                    vec![
                        job.id.into(),
                        job.channel.into(),
                        entry,
                        age_cell(job.started_unix as i64),
                        stage,
                    ]
                })
                .collect();
            ui::show(View::table(
                "jobs",
                ["ID", "CHANNEL", "ENTRY", "STARTED", "STAGE"],
                rows,
            ))
        }
        JobCmd::Cancel { id } => {
            let message = if client.jobs().cancel(&id).await? {
                format!("cancelled {id}")
            } else {
                format!("{id} already finished")
            };
            ui::show(View::line(message))
        }
    }
}
