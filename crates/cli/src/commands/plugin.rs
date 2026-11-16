//! `hestia plugin …` — the plugin registry.

use anyhow::Result;
use clap::Subcommand;
use proto::plugin::PluginState;

use crate::ui::{self, View};

#[derive(Subcommand)]
pub enum PluginCmd {
    /// Every plugin and its state
    List,
    /// Enable a plugin, and what it requires
    Enable { id: String },
    /// Disable a plugin; refused while another enabled plugin requires it
    Disable { id: String },
}

fn state_name(state: PluginState) -> &'static str {
    match state {
        PluginState::Enabled => "enabled",
        PluginState::Disabled => "disabled",
        PluginState::Failed => "failed",
    }
}

pub async fn run(cmd: PluginCmd) -> Result<()> {
    let client = super::connect().await?;
    match cmd {
        PluginCmd::List => {
            let plugins = client.plugin().list().await?;
            if plugins.is_empty() {
                return ui::show(View::note("no plugins"));
            }
            let rows = plugins
                .into_iter()
                .map(|p| {
                    vec![
                        p.id,
                        p.name,
                        p.version,
                        state_name(p.state).to_string(),
                        p.requires.join(", "),
                    ]
                })
                .collect();
            ui::show(View::table(
                "plugins",
                ["ID", "NAME", "VERSION", "STATE", "REQUIRES"],
                rows,
            ))?;
        }
        PluginCmd::Enable { id } => {
            let plugin = client.plugin().enable(&id).await?;
            ui::show(View::line(format!("enabled {}", plugin.id)))?;
        }
        PluginCmd::Disable { id } => {
            let plugin = client.plugin().disable(&id).await?;
            ui::show(View::line(format!("disabled {}", plugin.id)))?;
        }
    }
    Ok(())
}
