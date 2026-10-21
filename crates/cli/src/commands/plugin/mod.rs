//! `hestia plugin …` — launcher plugins. Server plugins are `hestia server-plugin`.

mod add;

use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use clap::Subcommand;
use client::proto::plugin::{PluginInfo, PluginSource, PluginState};

use crate::commands::connect;
use crate::ui::{self, View};

#[derive(Subcommand)]
pub enum PluginCmd {
    /// Installed plugins and their state
    #[command(alias = "ls")]
    List,
    /// Install or update a plugin from a .hplugin file or a folder
    Add {
        path: PathBuf,
        #[arg(short = 'y', long, help = "Approve exactly the permissions shown")]
        yes: bool,
    },
    /// One plugin's manifest, grant and health
    Info { id: String },
    /// Load a plugin
    Enable { id: String },
    /// Unload a plugin; it stays installed
    Disable { id: String },
    /// Uninstall a plugin and delete its data
    #[command(alias = "rm")]
    Remove {
        id: String,
        #[arg(short = 'y', long, help = "Remove without the confirmation prompt")]
        yes: bool,
    },
}

pub async fn run(cmd: PluginCmd) -> Result<()> {
    let client = connect().await?;
    let plugins = client.plugin();
    match cmd {
        PluginCmd::List => list(plugins.list().await?),
        PluginCmd::Add { path, yes } => add::add(plugins, &path, yes).await,
        PluginCmd::Info { id } => info(&plugins.info(&id).await?),
        PluginCmd::Enable { id } => {
            let plugin = plugins.enable(&id).await?;
            let reason = plugin.last_error.unwrap_or_default();
            match plugin.state {
                PluginState::Enabled => ui::show(View::line(format!("'{id}' enabled"))),
                PluginState::Failed => bail!("'{id}' failed to load: {reason}"),
                state => {
                    let label = state_label(state);
                    let note = if reason.is_empty() {
                        format!("'{id}' is {label}")
                    } else {
                        format!("'{id}' is {label}: {reason}")
                    };
                    ui::show(View::note(note))
                }
            }
        }
        PluginCmd::Disable { id } => {
            plugins.disable(&id).await?;
            ui::show(View::line(format!("'{id}' disabled")))
        }
        PluginCmd::Remove { id, yes } => {
            if !yes {
                confirm_remove(&id)?;
            }
            plugins.remove(&id).await?;
            ui::show(View::line(format!("'{id}' removed")))
        }
    }
}

fn confirm_remove(id: &str) -> Result<()> {
    let remove = ui::confirm(
        &format!("removing '{id}' also deletes its data"),
        "remove",
        "cancel",
    )
    .context("pass --yes to remove without confirming")?;
    if !remove {
        bail!("remove cancelled");
    }
    Ok(())
}

fn list(plugins: Vec<PluginInfo>) -> Result<()> {
    if plugins.is_empty() {
        return ui::show(View::note("no plugins installed"));
    }
    ui::show(View::table(
        "Plugins",
        ["ID", "NAME", "VERSION", "STATE", "SOURCE"],
        plugins
            .into_iter()
            .map(|p| {
                vec![
                    p.manifest.id,
                    p.manifest.name,
                    p.manifest.version,
                    state_label(p.state).to_string(),
                    source_label(p.source).to_string(),
                ]
            })
            .collect(),
    ))
}

fn info(plugin: &PluginInfo) -> Result<()> {
    let manifest = &plugin.manifest;
    let mut rows = vec![
        ("ID", manifest.id.clone()),
        ("Name", manifest.name.clone()),
        ("Version", manifest.version.clone()),
        ("State", state_label(plugin.state).to_string()),
        ("Source", source_label(plugin.source).to_string()),
    ];
    if !manifest.description.is_empty() {
        rows.push(("Description", manifest.description.clone()));
    }
    if !manifest.authors.is_empty() {
        rows.push(("Authors", manifest.authors.join(", ")));
    }
    let permissions: Vec<&str> = plugin
        .grant
        .permissions
        .iter()
        .map(|p| p.description())
        .collect();
    rows.push((
        "Permissions",
        if permissions.is_empty() {
            "none".to_string()
        } else {
            permissions.join("; ")
        },
    ));
    if let Some(error) = &plugin.last_error {
        rows.push(("Last error", error.clone()));
    }
    if let Some(bytes) = plugin.data_bytes {
        rows.push(("Data", ui::human_bytes(bytes)));
    }
    rows.push((
        "Calls",
        format!(
            "{} ({} traps, {} timeouts)",
            plugin.calls, plugin.traps, plugin.timeouts
        ),
    ));
    ui::show(View::detail(rows))
}

fn state_label(state: PluginState) -> &'static str {
    match state {
        PluginState::Enabled => "enabled",
        PluginState::Disabled => "disabled",
        PluginState::Failed => "failed",
        PluginState::NeedsApproval => "needs approval",
    }
}

fn source_label(source: PluginSource) -> &'static str {
    match source {
        PluginSource::File => "file",
        PluginSource::Folder => "folder",
        PluginSource::Url => "url",
        PluginSource::Git => "git",
        PluginSource::Dev => "dev",
    }
}
