//! `hestia job …` — the daemon's running jobs.

use anyhow::Result;
use clap::Subcommand;

use crate::commands::mc::age_label;
use crate::ui::{self, View};

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
                return ui::show(View::note("no running jobs"));
            }
            let rows = jobs
                .into_iter()
                .map(|job| {
                    let stage = job
                        .progress
                        .as_ref()
                        .and_then(|p| p.get("phase"))
                        .and_then(|p| p.as_str())
                        .unwrap_or("-")
                        .to_string();
                    let entry = job.subject.map_or_else(|| "-".to_string(), |s| s.id);
                    vec![
                        job.id,
                        job.channel,
                        entry,
                        age_label(job.started_unix as i64),
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
