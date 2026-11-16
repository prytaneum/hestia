//! `hestia plugin …` — the plugin registry.

use anyhow::{bail, Context, Result};
use clap::Subcommand;
use proto::plugin::{Inspection, PluginState, Tier};

use crate::ui::{self, View};

#[derive(Subcommand)]
pub enum PluginCmd {
    /// Every plugin and its state
    List,
    /// Enable a plugin, and what it requires
    Enable { id: String },
    /// Disable a plugin; refused while another enabled plugin requires it
    Disable { id: String },
    /// Install a plugin from a directory holding plugin.toml and plugin.wasm
    Add {
        path: String,
        /// Approve what it asks for without prompting
        #[arg(long)]
        yes: bool,
    },
    /// Delete an installed plugin
    Remove { id: String },
    /// A plugin's secrets
    Secret {
        #[command(subcommand)]
        cmd: SecretCmd,
    },
}

#[derive(Subcommand)]
pub enum SecretCmd {
    /// Store a secret, typed without echo or read from stdin
    Set { id: String, key: String },
}

fn state_name(state: PluginState) -> &'static str {
    match state {
        PluginState::Enabled => "enabled",
        PluginState::Disabled => "disabled",
        PluginState::Failed => "failed",
    }
}

fn show_grant(inspection: &Inspection) -> Result<()> {
    ui::show(View::detail([
        ("Plugin", format!("{} ({})", inspection.name, inspection.id)),
        ("Version", inspection.version.clone()),
    ]))?;
    let grant = &inspection.grant;
    if grant.permissions.is_empty() {
        ui::show(View::note("it asks for no permissions"))?;
    } else {
        let rows = grant
            .permissions
            .iter()
            .map(|p| {
                let tier = match p.tier {
                    Tier::Sandboxed => "sandboxed",
                    Tier::Unrestricted => "UNRESTRICTED",
                };
                let scope = p.scope.as_ref().map(|s| s.values.join(", "));
                vec![
                    p.permission.clone(),
                    tier.into(),
                    scope.unwrap_or_default(),
                    p.prompt.clone(),
                ]
            })
            .collect();
        ui::show(View::table(
            "permissions",
            ["PERMISSION", "TIER", "SCOPE", "ALLOWS"],
            rows,
        ))?;
    }
    if !grant.requires.is_empty() {
        ui::show(View::line(format!(
            "it needs: {}",
            grant.requires.join(", ")
        )))?;
    }
    Ok(())
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
        PluginCmd::Add { path, yes } => {
            let path = std::fs::canonicalize(&path)
                .with_context(|| format!("cannot read {path}"))?
                .to_string_lossy()
                .into_owned();
            let inspection = client.plugin().inspect(&path).await?;
            show_grant(&inspection)?;
            if !yes
                && !ui::confirm("Install it with these permissions?", "install", "cancel")
                    .context("pass --yes to approve without a prompt")?
            {
                bail!("cancelled");
            }
            let plugin = client
                .plugin()
                .install(&path, &inspection.approval_digest)
                .await?;
            ui::show(View::line(format!(
                "installed {} ({})",
                plugin.id,
                state_name(plugin.state)
            )))?;
        }
        PluginCmd::Remove { id } => {
            client.plugin().remove(&id).await?;
            ui::show(View::line(format!("removed {id}")))?;
        }
        PluginCmd::Secret {
            cmd: SecretCmd::Set { id, key },
        } => {
            let value = ui::prompt_secret(&format!("{key} for {id}"))?;
            if value.is_empty() {
                bail!("no value given");
            }
            client.plugin().set_secret(&id, &key, &value).await?;
            ui::show(View::line(format!("stored {key} for {id}")))?;
        }
    }
    Ok(())
}
