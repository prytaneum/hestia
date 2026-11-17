//! `hestia plugin …` — the plugin registry.

use anyhow::{bail, Context, Result};
use clap::{Args, Subcommand};
use proto::error::ErrorInfo;
use proto::plugin::{
    Choice, EntryScope, Grant, GrantedPermission, Inspection, PermissionScope, PluginInfo,
    PluginReleaseParams, PluginSource, PluginState, Tier,
};

use client::Client;

mod author;

use crate::ui::{self, View};

/// The servers and instances a plugin may reach, as flags. A reference is an
/// id or a name; the daemon resolves it.
#[derive(Args, Default, Clone)]
pub struct EntryArgs {
    /// Let it reach this server or instance; repeat for more
    #[arg(long = "entry", value_name = "ENTRY", conflicts_with = "all_entries")]
    entries: Vec<String>,
    /// Let it reach every server and instance
    #[arg(long)]
    all_entries: bool,
}

impl EntryArgs {
    /// The scope the flags name; `None` when none was given.
    fn scope(self) -> Option<EntryScope> {
        match (self.all_entries, self.entries.is_empty()) {
            (false, true) => None,
            (all, _) => Some(EntryScope {
                all,
                ids: self.entries,
            }),
        }
    }
}

/// What you decide at approval beyond the permissions it lists.
#[derive(Args, Clone)]
pub struct ChoiceArgs {
    /// Grant an optional permission; none is granted unless named
    #[arg(long = "optional", value_name = "PERMISSION")]
    optional: Vec<String>,
    #[command(flatten)]
    entries: EntryArgs,
}

#[derive(Subcommand)]
pub enum PluginCmd {
    #[command(flatten)]
    Author(author::AuthorCmd),
    #[command(flatten)]
    Manage(ManageCmd),
}

#[derive(Subcommand)]
pub enum ManageCmd {
    /// Every plugin and its state
    List,
    /// Enable a plugin, and what it requires
    Enable { id: String },
    /// Disable a plugin; one that other enabled plugins need asks before
    /// disabling them too
    Disable {
        id: String,
        /// Also disable the plugins that need it, without prompting
        #[arg(long)]
        yes: bool,
    },
    /// Install a plugin from a directory holding plugin.toml and plugin.wasm;
    /// needs `plugins.developer`. A directory whose plugin is already a dev
    /// install replaces it, keeping its settings, secrets and schedules
    Add {
        path: String,
        #[command(flatten)]
        choice: ChoiceArgs,
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
        #[command(flatten)]
        choice: ChoiceArgs,
        /// Approve what it asks for without prompting
        #[arg(long)]
        yes: bool,
    },
    /// Check every repository for newer versions of what is installed
    Updates,
    /// Update a plugin; one that asks for more than you approved shows it first
    Update {
        id: String,
        #[command(flatten)]
        entries: EntryArgs,
        /// Approve a widened update without prompting
        #[arg(long)]
        yes: bool,
    },
    /// The repositories plugins come from
    Repo {
        #[command(subcommand)]
        cmd: RepoCmd,
    },
    /// Grant an optional permission of an installed plugin; for one that takes
    /// entries, shows what it may reach and accepts the entry flags
    Grant {
        id: String,
        permission: String,
        #[command(flatten)]
        entries: EntryArgs,
    },
    /// Choose the servers and instances an installed plugin may reach
    #[command(group(clap::ArgGroup::new("scope").required(true).multiple(false).args(["all", "entry", "none"])))]
    Entries {
        id: String,
        /// Let it reach every server and instance
        #[arg(long)]
        all: bool,
        /// Let it reach this server or instance; repeat for more
        #[arg(long, value_name = "ENTRY")]
        entry: Vec<String>,
        /// Let it reach none
        #[arg(long)]
        none: bool,
    },
    /// Revoke an optional permission of an installed plugin
    Revoke { id: String, permission: String },
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
        PluginState::MissingDependency => "missing dependency",
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

fn scope_text_of(scope: Option<&EntryScope>) -> String {
    match scope {
        None => String::new(),
        Some(scope) if scope.all => "all".to_owned(),
        Some(scope) => format!("{} chosen", scope.ids.len()),
    }
}

fn source_name(source: PluginSource) -> &'static str {
    match source {
        PluginSource::Builtin => "built in",
        PluginSource::Dev => "dev",
        PluginSource::Repository => "repository",
    }
}

fn permission_rows(permissions: &[&GrantedPermission]) -> Vec<Vec<String>> {
    permissions
        .iter()
        .map(|p| {
            let tier = match p.tier {
                Tier::Sandboxed => "sandboxed",
                Tier::Unrestricted => "UNRESTRICTED",
            };
            let scope = match (&p.scope, p.entries) {
                (Some(scope), _) => scope_text(scope),
                (None, true) => "the entries you choose".to_owned(),
                (None, false) => String::new(),
            };
            vec![p.permission.clone(), tier.into(), scope, p.prompt.clone()]
        })
        .collect()
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
    let (optional, required): (Vec<_>, Vec<_>) = grant.permissions.iter().partition(|p| p.optional);
    if required.is_empty() {
        ui::show(View::note("it asks for no required permissions"))?;
    } else {
        ui::show(View::table(
            "permissions",
            ["PERMISSION", "TIER", "SCOPE", "ALLOWS"],
            permission_rows(&required),
        ))?;
    }
    if !optional.is_empty() {
        ui::show(View::table(
            "optional permissions, declined unless you grant them",
            ["PERMISSION", "TIER", "SCOPE", "ALLOWS"],
            permission_rows(&optional),
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
    if !grant.dependencies.is_empty() {
        let needs: Vec<String> = grant
            .dependencies
            .iter()
            .map(|dependency| format!("{} {}", dependency.id, dependency.range))
            .collect();
        ui::show(View::line(format!(
            "it needs plugins: {}",
            needs.join(", ")
        )))?;
    }
    Ok(())
}

/// The plugin and, before it, each plugin the install brings with it, all
/// shown for one approval.
fn show_plan(inspection: &Inspection) -> Result<()> {
    for planned in &inspection.plan {
        ui::show(View::note(format!(
            "it also installs {}, which it needs, with no optional permissions and no entries; \
             grant them later with `hestia plugin grant` and `hestia plugin entries`",
            planned.id
        )))?;
        show_grant(planned)?;
    }
    show_grant(inspection)
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

/// Which servers and instances to let a plugin reach, picked on a terminal.
async fn pick_entries(client: &Client) -> Result<EntryScope> {
    let options = [
        "none yet".to_owned(),
        "every server and instance".to_owned(),
        "choose which".to_owned(),
    ];
    match ui::prompt_select("Which servers and instances may it reach", &options)? {
        0 => Ok(EntryScope::default()),
        1 => Ok(EntryScope {
            all: true,
            ids: Vec::new(),
        }),
        _ => {
            let mut labels = Vec::new();
            let mut ids = Vec::new();
            for server in client.server().list().await? {
                labels.push(format!("server {}", server.name));
                ids.push(server.id);
            }
            for instance in client.instance().list().await? {
                labels.push(format!("instance {}", instance.name));
                ids.push(instance.id);
            }
            if labels.is_empty() {
                bail!("there is no server or instance to choose from");
            }
            let chosen = ui::multi_select("Entries it may reach", &labels)?;
            Ok(EntryScope {
                all: false,
                ids: chosen.into_iter().map(|index| ids[index].clone()).collect(),
            })
        }
    }
}

/// What the user decides at an install: the optional permissions named by flag
/// (or, on a terminal, each one agreed to), and the entries it may reach (the
/// flags, or picked on a terminal when it holds a permission that takes them).
/// Without a terminal, or with `--yes`, nothing optional is granted and no entry
/// is reachable unless a flag says so.
async fn choose(client: &Client, grant: &Grant, args: ChoiceArgs, yes: bool) -> Result<Choice> {
    let ask = ui::is_interactive() && !yes;
    let mut optional = args.optional;
    if ask {
        let named = optional.clone();
        for p in grant
            .permissions
            .iter()
            .filter(|p| p.optional && !named.contains(&p.permission))
        {
            if ui::prompt_confirm(&format!(
                "Grant the optional {}: {}?",
                p.permission, p.prompt
            ))? {
                optional.push(p.permission.clone());
            }
        }
    }
    let takes_entries = grant
        .permissions
        .iter()
        .any(|p| p.entries && (!p.optional || optional.contains(&p.permission)));
    let entries = match args.entries.scope() {
        Some(scope) => scope,
        None if takes_entries && ask => pick_entries(client).await?,
        None => EntryScope::default(),
    };
    Ok(Choice { optional, entries })
}

/// What the user granted within the plugin's grant: its optional permissions and
/// the entries it may reach.
fn show_granted(plugin: &PluginInfo) -> Result<()> {
    let optional = match plugin.optional.is_empty() {
        true => "none".to_owned(),
        false => plugin.optional.join(", "),
    };
    ui::show(View::note(format!(
        "optional permissions granted: {optional}"
    )))?;
    if let Some(scope) = &plugin.entries {
        ui::show(View::note(format!("it may reach {}", entries_text(scope))))?;
    }
    Ok(())
}

fn entries_text(scope: &EntryScope) -> String {
    match (scope.all, scope.ids.len()) {
        (true, _) => "every server and instance".to_owned(),
        (false, 0) => "no server or instance".to_owned(),
        (false, count) => format!("{count} server or instance entries"),
    }
}

pub async fn run(cmd: PluginCmd) -> Result<()> {
    match cmd {
        PluginCmd::Author(cmd) => author::run(cmd).await,
        PluginCmd::Manage(cmd) => manage(cmd).await,
    }
}

async fn manage(cmd: ManageCmd) -> Result<()> {
    let client = super::connect().await?;
    match cmd {
        ManageCmd::List => {
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
                        p.optional.join(", "),
                        scope_text_of(p.entries.as_ref()),
                    ]
                })
                .collect();
            ui::show(View::table(
                "plugins",
                [
                    "ID", "NAME", "VERSION", "STATE", "SOURCE", "REQUIRES", "OPTIONAL", "ENTRIES",
                ],
                rows,
            ))?;
        }
        ManageCmd::Enable { id } => {
            let plugin = client.plugin().enable(&id).await?;
            ui::show(View::line(format!("enabled {}", plugin.id)))?;
        }
        ManageCmd::Disable { id, yes } => {
            let plugin = match client.plugin().disable(&id, yes).await {
                Err(error) => match client::error_info(&error) {
                    ErrorInfo::PluginRequired { dependents, .. } => {
                        ui::show(View::note(format!(
                            "{id} is needed by {}, which would be disabled too",
                            dependents.join(", ")
                        )))?;
                        approve("Disable them all?", false)?;
                        client.plugin().disable(&id, true).await?
                    }
                    _ => return Err(error.into()),
                },
                Ok(plugin) => plugin,
            };
            ui::show(View::line(format!("disabled {}", plugin.id)))?;
        }
        ManageCmd::Add { path, choice, yes } => {
            let path = std::fs::canonicalize(&path)
                .with_context(|| format!("cannot read {path}"))?
                .to_string_lossy()
                .into_owned();
            let inspection = client.plugin().inspect(&path).await?;
            show_plan(&inspection)?;
            approve("Install it with these permissions?", yes)?;
            let choice = choose(&client, &inspection.grant, choice, yes).await?;
            let plugin = client
                .plugin()
                .install(&path, &inspection.approval_digest, choice)
                .await?;
            ui::show(View::line(format!(
                "installed {} ({})",
                plugin.id,
                state_name(plugin.state)
            )))?;
            show_granted(&plugin)?;
        }
        ManageCmd::Get {
            repository,
            id,
            version,
            choice,
            yes,
        } => {
            let release = PluginReleaseParams {
                repository,
                id,
                version,
            };
            let inspection = client.plugin().inspect_release(&release).await?;
            show_plan(&inspection)?;
            approve("Install it with these permissions?", yes)?;
            let choice = choose(&client, &inspection.grant, choice, yes).await?;
            let plugin = client
                .plugin()
                .install_release(release, &inspection.approval_digest, choice)
                .await?;
            ui::show(View::line(format!(
                "installed {} {} ({})",
                plugin.id,
                plugin.version,
                state_name(plugin.state)
            )))?;
            show_granted(&plugin)?;
        }
        ManageCmd::Grant {
            id,
            permission,
            entries,
        } => {
            let mut plugin = client
                .plugin()
                .set_permission(&id, &permission, true)
                .await?;
            if let Some(scope) = entries.scope() {
                plugin = client.plugin().set_entries(&id, scope).await?;
            }
            ui::show(View::line(format!("granted {permission} to {id}")))?;
            if let Some(scope) = &plugin.entries {
                ui::show(View::note(format!("it may reach {}", entries_text(scope))))?;
            }
        }
        ManageCmd::Entries {
            id,
            all,
            entry,
            none: _,
        } => {
            let scope = EntryScope { all, ids: entry };
            let plugin = client.plugin().set_entries(&id, scope).await?;
            let reach = plugin.entries.unwrap_or_default();
            ui::show(View::line(format!(
                "{id} may reach {}",
                entries_text(&reach)
            )))?;
        }
        ManageCmd::Revoke { id, permission } => {
            client
                .plugin()
                .set_permission(&id, &permission, false)
                .await?;
            ui::show(View::line(format!("revoked {permission} from {id}")))?;
        }
        ManageCmd::Updates => {
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
        ManageCmd::Update { id, entries, yes } => {
            let update = client.plugin().inspect_update(&id).await?;
            if update.widened {
                ui::show(View::note(format!(
                    "{} {} asks for more than you approved for {}",
                    id, update.inspection.version, update.installed
                )))?;
                show_plan(&update.inspection)?;
                approve("Update with these permissions?", yes)?;
            }
            let held = client
                .plugin()
                .list()
                .await?
                .into_iter()
                .find(|p| p.id == id)
                .and_then(|p| p.entries);
            let adds_entries = held.is_none()
                && update
                    .inspection
                    .grant
                    .permissions
                    .iter()
                    .any(|p| p.entries && !p.optional);
            let scope = match entries.scope() {
                Some(scope) => Some(scope),
                None if adds_entries && ui::is_interactive() && !yes => {
                    Some(pick_entries(&client).await?)
                }
                None => None,
            };
            let plugin = client
                .plugin()
                .update(&id, &update.inspection.approval_digest, scope)
                .await?;
            ui::show(View::line(format!(
                "updated {} to {} ({})",
                plugin.id,
                plugin.version,
                state_name(plugin.state)
            )))?;
        }
        ManageCmd::Repo { cmd } => match cmd {
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
        ManageCmd::Remove { id } => {
            client.plugin().remove(&id).await?;
            ui::show(View::line(format!("removed {id}")))?;
        }
        ManageCmd::Secret {
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
