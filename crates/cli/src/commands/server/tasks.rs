//! `hestia server <name> task …` — what the daemon runs on a schedule while
//! the server is up: a restart or a console command. A schedule is an interval
//! (`30m`, `6h`, `1d`) or a daily time (`04:30`, this machine's clock).

use anyhow::{bail, Result};
use clap::Subcommand;
use client::proto::tasks::{ServerTask, TaskAction};
use client::Client;

use crate::ui::{self, View};

#[derive(Subcommand)]
pub enum TaskCmd {
    /// The server's scheduled tasks
    #[command(visible_alias = "ls")]
    List,
    /// Restart the server on a schedule
    Restart {
        /// `30m`, `6h`, `1d`, or a daily time such as `04:30`
        schedule: String,
    },
    /// Send a console command on a schedule
    Command {
        /// `30m`, `6h`, `1d`, or a daily time such as `04:30`
        schedule: String,
        #[arg(required = true, trailing_var_arg = true)]
        command: Vec<String>,
    },
    /// Resume a paused task
    Enable { id: String },
    /// Pause a task without deleting it
    Disable { id: String },
    /// Delete a task
    #[command(visible_alias = "rm")]
    Remove { id: String },
}

pub(super) async fn run(client: &Client, server: &str, cmd: TaskCmd) -> Result<()> {
    let tasks = client.tasks();
    match cmd {
        TaskCmd::List => {
            let list = tasks.list(server).await?;
            if list.is_empty() {
                return ui::show(View::note("no scheduled tasks"));
            }
            ui::show(View::table(
                "Scheduled tasks",
                ["ID", "SCHEDULE", "DOES", "STATE"],
                list.into_iter()
                    .map(|task| {
                        vec![
                            short(&task.id).to_string(),
                            task.schedule,
                            match task.action {
                                TaskAction::Restart => "restart".to_string(),
                                TaskAction::Command => task.command,
                            },
                            match task.enabled {
                                true => "on".to_string(),
                                false => "paused".to_string(),
                            },
                        ]
                    })
                    .collect(),
            ))
        }
        TaskCmd::Restart { schedule } => {
            let task = new_task(TaskAction::Restart, schedule, String::new());
            created(tasks.save(server, task).await?)
        }
        TaskCmd::Command { schedule, command } => {
            let task = new_task(TaskAction::Command, schedule, command.join(" "));
            created(tasks.save(server, task).await?)
        }
        TaskCmd::Enable { id } => toggle(client, server, &id, true).await,
        TaskCmd::Disable { id } => toggle(client, server, &id, false).await,
        TaskCmd::Remove { id } => {
            let task = find(client, server, &id).await?;
            tasks.remove(server, &task.id).await?;
            ui::show(View::line(format!("removed task {}", short(&task.id))))
        }
    }
}

fn new_task(action: TaskAction, schedule: String, command: String) -> ServerTask {
    ServerTask {
        action,
        schedule,
        command,
        enabled: true,
        ..Default::default()
    }
}

fn created(task: ServerTask) -> Result<()> {
    ui::show(View::line(format!(
        "task {} scheduled ({})",
        short(&task.id),
        task.schedule
    )))
}

async fn toggle(client: &Client, server: &str, id: &str, enabled: bool) -> Result<()> {
    let mut task = find(client, server, id).await?;
    task.enabled = enabled;
    let task = client.tasks().save(server, task).await?;
    ui::show(View::line(format!(
        "task {} {}",
        short(&task.id),
        if enabled { "enabled" } else { "paused" }
    )))
}

/// A task by the trailing characters of its id, as `list` prints them.
async fn find(client: &Client, server: &str, id: &str) -> Result<ServerTask> {
    let mut matches = client
        .tasks()
        .list(server)
        .await?
        .into_iter()
        .filter(|task| task.id.ends_with(id));
    match (matches.next(), matches.next()) {
        (Some(task), None) => Ok(task),
        (None, _) => bail!("no scheduled task matches '{id}'"),
        (Some(_), Some(_)) => bail!("'{id}' matches more than one task"),
    }
}

/// Ids are time-ordered, so their tail is what tells two apart.
fn short(id: &str) -> &str {
    &id[id.len().saturating_sub(8)..]
}
