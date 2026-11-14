//! `hestia screenshot …` — the shots every instance took, read where they are.

use anyhow::Result;
use clap::Subcommand;

use crate::commands::mc::age_cell;
use crate::ui::{self, Cell, View};

#[derive(Subcommand)]
pub enum ScreenshotCmd {
    /// Every screenshot the instances share
    #[command(alias = "ls")]
    List {
        /// One instance by name or id, whether or not it shares them
        #[arg(long)]
        instance: Option<String>,
    },
    /// Delete one screenshot from the instance that took it
    #[command(alias = "rm")]
    Remove {
        /// Instance name or id
        instance: String,
        /// The file, as `screenshot list` shows it
        file: String,
    },
}

pub async fn run(cmd: ScreenshotCmd) -> Result<()> {
    let client = super::connect().await?;
    match cmd {
        ScreenshotCmd::List { instance } => {
            let taken = client
                .screenshot()
                .list(instance.as_deref().unwrap_or_default())
                .await?;
            if taken.is_empty() {
                return ui::show(View::empty(
                    "no screenshots yet — `hestia sync on screenshots` reads every instance's",
                ));
            }
            let rows = taken
                .into_iter()
                .map(|shot| {
                    vec![
                        age_cell(shot.taken_unix),
                        shot.instance_name.into(),
                        shot.file.into(),
                        Cell::bytes(shot.bytes),
                    ]
                })
                .collect();
            ui::show(View::table(
                "Screenshots",
                ["TAKEN", "INSTANCE", "FILE", "SIZE"],
                rows,
            ))
        }
        ScreenshotCmd::Remove { instance, file } => {
            client.screenshot().delete(&instance, &file).await?;
            ui::show(View::line(format!("deleted '{file}' from '{instance}'")))
        }
    }
}
