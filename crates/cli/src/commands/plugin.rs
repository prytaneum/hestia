//! `hestia plugin …` — the plugin registry.

use anyhow::{bail, Context, Result};
use clap::Subcommand;
use proto::plugin::{
    Inspection, PermissionScope, PluginReleaseParams, PluginSource, PluginState, Tier,
};

use crate::ui::{self, View};

#[derive(Subcommand)]
pub enum PluginCmd {
    /// Every plugin and its state
    List,
    /// Enable a plugin, and what it requires
    Enable { id: String },
    /// Disable a plugin; refused while another enabled plugin requires it
    Disable { id: String },
    /// Install a plugin from a directory holding plugin.toml and plugin.wasm;
    /// needs `plugins.developer`
    Add {
        path: String,
        /// Approve what it asks for without prompting
        #[arg(long)]
        yes: bool,
    },
    /// Install a plugin from a repository you added
    Get {
        repository: String,
        id: String,
        /// A version other than the newest
        #[arg(long)]
        version: Option<String>,
        /// Approve what it asks for without prompting
        #[arg(long)]
        yes: bool,
    },
    /// Check every repository for newer versions of what is installed
    Updates,
    /// Update a plugin; one that asks for more than you approved shows it first
    Update {
        id: String,
        /// Approve a widened update without prompting
        #[arg(long)]
        yes: bool,
    },
    /// The repositories plugins come from
    Repo {
        #[command(subcommand)]
        cmd: RepoCmd,
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
pub enum RepoCmd {
    /// Every repository and the key pinned for it
    List,
    /// Add a repository; its index must verify under the key you give
    Add {
        id: String,
        /// Where the signed index is; its signature is the same URL plus `.minisig`
        url: String,
        /// The minisign public key the repository signs with
        #[arg(long)]
        key: String,
    },
    /// Forget a repository; what it installed stays but no longer updates
    Remove { id: String },
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

fn source_name(source: PluginSource) -> &'static str {
    match source {
        PluginSource::Builtin => "built in",
        PluginSource::Dev => "dev",
        PluginSource::Repository => "repository",
    }
}

fn show_grant(inspection: &Inspection) -> Result<()> {
    let mut details = vec![
        ("Plugin", format!("{} ({})", inspection.name, inspection.id)),
        ("Version", inspection.version.clone()),
    ];
    if let Some(signer) = &inspection.signer {
        let standing = match signer.official {
            true => "Hestia (official)",
            false => "its author, not Hestia",
        };
        details.push(("Repository", signer.repository.clone()));
        details.push((
            "Signed by",
            format!("{standing}, key {}", signer.fingerprint),
        ));
    }
    ui::show(View::detail(details))?;
    for warning in &inspection.warnings {
        ui::show(View::note(format!("{warning}: {}", warning.hint())))?;
    }
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
    if !grant.schedules.is_empty() {
        let rows = grant
            .schedules
            .iter()
            .map(|schedule| vec![schedule.id.clone(), schedule.every.clone()])
            .collect();
        ui::show(View::table(
            "it wakes itself without being asked",
            ["SCHEDULE", "EVERY"],
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

fn approve(question: &str, yes: bool) -> Result<()> {
    if !yes
        && !ui::confirm(question, "approve", "cancel")
            .context("pass --yes to approve without a prompt")?
    {
        bail!("cancelled");
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
                        source_name(p.source).to_string(),
                        p.requires.join(", "),
                    ]
                })
                .collect();
            ui::show(View::table(
                "plugins",
                ["ID", "NAME", "VERSION", "STATE", "SOURCE", "REQUIRES"],
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
            approve("Install it with these permissions?", yes)?;
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
        PluginCmd::Get {
            repository,
            id,
            version,
            yes,
        } => {
            let release = PluginReleaseParams {
                repository,
                id,
                version,
            };
            let inspection = client.plugin().inspect_release(&release).await?;
            show_grant(&inspection)?;
            approve("Install it with these permissions?", yes)?;
            let plugin = client
                .plugin()
                .install_release(release, &inspection.approval_digest)
                .await?;
            ui::show(View::line(format!(
                "installed {} {} ({})",
                plugin.id,
                plugin.version,
                state_name(plugin.state)
            )))?;
        }
        PluginCmd::Updates => {
            let found = client.plugin().updates().await?;
            for failure in &found.failed {
                ui::show(View::note(format!(
                    "{}: not checked: {}",
                    failure.repository, failure.detail
                )))?;
            }
            for revoked in &found.revoked {
                ui::show(View::note(format!(
                    "{} {} is revoked by {} and disabled{}",
                    revoked.id,
                    revoked.version,
                    revoked.repository,
                    match revoked.reason.is_empty() {
                        true => String::new(),
                        false => format!(": {}", revoked.reason),
                    }
                )))?;
            }
            if found.updates.is_empty() {
                return ui::show(View::note("every plugin is up to date"));
            }
            let rows = found
                .updates
                .into_iter()
                .map(|u| vec![u.id, u.repository, u.installed, u.available])
                .collect();
            ui::show(View::table(
                "updates",
                ["ID", "REPOSITORY", "INSTALLED", "AVAILABLE"],
                rows,
            ))?;
        }
        PluginCmd::Update { id, yes } => {
            let update = client.plugin().inspect_update(&id).await?;
            if update.widened {
                ui::show(View::note(format!(
                    "{} {} asks for more than you approved for {}",
                    id, update.inspection.version, update.installed
                )))?;
                show_grant(&update.inspection)?;
                approve("Update with these permissions?", yes)?;
            }
            let plugin = client
                .plugin()
                .update(&id, &update.inspection.approval_digest)
                .await?;
            ui::show(View::line(format!(
                "updated {} to {} ({})",
                plugin.id,
                plugin.version,
                state_name(plugin.state)
            )))?;
        }
        PluginCmd::Repo { cmd } => match cmd {
            RepoCmd::List => {
                let rows = client
                    .plugin()
                    .repositories()
                    .await?
                    .into_iter()
                    .map(|r| vec![r.id, r.url, r.signer])
                    .collect();
                ui::show(View::table("repositories", ["ID", "URL", "SIGNER"], rows))?;
            }
            RepoCmd::Add { id, url, key } => {
                let repository = client.plugin().add_repository(&id, &url, &key).await?;
                ui::show(View::line(format!(
                    "added {} (signer {})",
                    repository.id, repository.signer
                )))?;
            }
            RepoCmd::Remove { id } => {
                client.plugin().remove_repository(&id).await?;
                ui::show(View::line(format!("removed {id}")))?;
            }
        },
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
