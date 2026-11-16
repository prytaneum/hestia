//! `hestia plugin …` — the plugin registry.

use anyhow::{bail, Context, Result};
use clap::Subcommand;
use proto::plugin::{Inspection, PermissionScope, PluginState, Tier};

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

/// `token`, in double quotes when it holds whitespace or a quote, so that the
/// prompt tells one argument from two and a path from its arguments.
fn quoted(token: &str) -> String {
    match token.contains(|c: char| c.is_whitespace() || c == '"') {
        true => format!("\"{}\"", token.replace('"', "\\\"")),
        false => token.to_owned(),
    }
}

/// Everything a scope lets through, so that approving it is informed: a host
/// that may serve unchecked files says so, a program shows its arguments.
fn scope_text(scope: &PermissionScope) -> String {
    let hosts = scope.values.iter().map(|value| {
        match scope
            .unhashed
            .iter()
            .any(|host| host.eq_ignore_ascii_case(value))
        {
            true => format!("{value} (no checksum)"),
            false => value.clone(),
        }
    });
    let programs = scope.programs.iter().map(|program| {
        [program.path.as_str()]
            .into_iter()
            .chain(program.args.iter().map(String::as_str))
            .map(quoted)
            .collect::<Vec<_>>()
            .join(" ")
    });
    hosts.chain(programs).collect::<Vec<_>>().join(", ")
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
                let scope = p.scope.as_ref().map(scope_text);
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

#[cfg(test)]
mod tests {
    use super::*;
    use proto::plugin::Program;

    #[test]
    fn the_scope_shows_unchecked_hosts_and_program_arguments() {
        let hosts = PermissionScope {
            values: vec!["api.example".into(), "files.example".into()],
            unhashed: vec!["files.example".into()],
            ..Default::default()
        };
        assert_eq!(
            scope_text(&hosts),
            "api.example, files.example (no checksum)"
        );
        let programs = PermissionScope {
            programs: vec![Program {
                path: "/usr/bin/tool".into(),
                args: vec!["--all".into(), "-x".into()],
                sha256: String::new(),
            }],
            ..Default::default()
        };
        assert_eq!(scope_text(&programs), "/usr/bin/tool --all -x");
        let spaced = PermissionScope {
            programs: vec![Program {
                path: r"C:\Program Files\x".into(),
                args: vec!["--flag".into(), "a b".into(), r#"say "hi""#.into()],
                sha256: String::new(),
            }],
            ..Default::default()
        };
        assert_eq!(
            scope_text(&spaced),
            r#""C:\Program Files\x" --flag "a b" "say \"hi\"""#
        );
    }
}
