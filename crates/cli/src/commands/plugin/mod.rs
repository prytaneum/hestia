//! `hestia plugin …` — launcher plugins. Server plugins are `hestia server-plugin`.

mod add;
mod browse;
mod cargo;
mod check;
mod config;
mod dev;
mod keygen;
mod new;
mod outdated;
mod pack;
mod page;
mod publish;
mod repo;
pub(crate) mod run;
mod sign;
mod store;
mod update;
mod world;

use anyhow::{bail, Context, Result};
use clap::Subcommand;
use client::proto::plugin::{PluginInfo, PluginSource, PluginState};

use crate::commands::connect;
use crate::ui::{self, View};

#[derive(Subcommand)]
pub enum PluginCmd {
    /// Create a plugin, and a plugin repository around it when there is none
    New {
        /// The plugin id, also its folder name
        id: String,
    },
    /// Build plugins into dist/<id>-<version>.hplugin
    Pack {
        /// Plugins to pack; all of the repository's by default
        ids: Vec<String>,
        /// Sign each package with this minisign secret key, writing <file>.minisig
        #[arg(long, value_name = "SECRET_KEY")]
        sign: Option<std::path::PathBuf>,
    },
    /// Run a plugin from its folder, reload it on every rebuild and stream its log
    Dev {
        /// A plugin folder of the repository; the current directory by default
        dir: Option<std::path::PathBuf>,
        #[arg(short = 'y', long, help = "Approve exactly the permissions shown")]
        yes: bool,
    },
    /// Check that the repository follows the one shape plugin repositories have
    Check {
        /// Inside the repository by default
        dir: Option<std::path::PathBuf>,
    },
    /// Build unpublished versions into <out>/packages and sign <out>/plugins.json
    Publish {
        /// A checkout of the repository's `repo` branch
        #[arg(long, value_name = "DIR")]
        out: std::path::PathBuf,
        /// The minisign secret key; it must be the repository's declared key
        #[arg(long, value_name = "SECRET_KEY")]
        sign: std::path::PathBuf,
        /// The repository's https URL; the `origin` remote by default
        #[arg(long)]
        repository: Option<String>,
    },
    /// Generate the repository's signing key pair and declare its public key
    Keygen {
        /// Where the key pair is written; the current directory by default
        #[arg(long, value_name = "DIR")]
        out: Option<std::path::PathBuf>,
        /// Leave the secret key unencrypted, for a CI secret
        #[arg(long)]
        unencrypted: bool,
    },
    #[command(flatten)]
    Manage(ManageCmd),
}

#[derive(Subcommand)]
pub enum ManageCmd {
    /// Installed plugins and their state
    #[command(alias = "ls")]
    List,
    /// Install or update a plugin from a repository, a .hplugin file, a folder or a link
    Add {
        /// A plugin id or <label>/<id> from a repository you added, a path, a link to a
        /// .hplugin file, or a repository's index link (offers to add it, then to pick plugins)
        #[arg(value_name = "PLUGIN_OR_PATH_OR_URL")]
        target: String,
        #[arg(short = 'y', long, help = "Approve exactly the permissions shown")]
        yes: bool,
    },
    /// Installed plugins with a newer version on offer
    Outdated {
        #[arg(
            long,
            help = "Also re-download unsigned link installs to read their version"
        )]
        unsigned: bool,
    },
    /// Update installed plugins to their newest build, asking before they gain permissions
    Update {
        /// The plugin to update
        #[arg(required_unless_present = "all")]
        id: Option<String>,
        /// Every plugin with a newer build on offer
        #[arg(long, conflicts_with = "id")]
        all: bool,
        #[arg(short = 'y', long, help = "Approve exactly the new permissions shown")]
        yes: bool,
    },
    /// Plugins the added repositories offer
    Browse {
        /// Matches id, name and description
        query: Option<String>,
        /// Only this repository (label, name or index URL)
        #[arg(long)]
        repo: Option<String>,
    },
    /// Plugin repositories: the stores whose signing keys you trust
    Repo {
        #[command(subcommand)]
        cmd: store::StoreCmd,
    },
    /// One plugin's manifest, grant and health
    Info { id: String },
    /// A plugin's settings: list them, show one, or set one
    Config {
        id: String,
        key: Option<String>,
        /// Prompted for, without echo, when a secret's value is left out
        value: Option<String>,
        /// Clear the setting back to its default
        #[arg(long, requires = "key", conflicts_with = "value")]
        unset: bool,
    },
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
    match cmd {
        PluginCmd::New { id } => new::run(&id),
        PluginCmd::Pack { ids, sign } => pack::run(&ids, sign.as_deref()),
        PluginCmd::Dev { dir, yes } => dev::run(dir.as_deref(), yes).await,
        PluginCmd::Check { dir } => check::run(dir.as_deref()),
        PluginCmd::Publish {
            out,
            sign,
            repository,
        } => publish::run(&out, &sign, repository),
        PluginCmd::Keygen { out, unencrypted } => keygen::run(out.as_deref(), unencrypted),
        PluginCmd::Manage(cmd) => manage(cmd).await,
    }
}

async fn manage(cmd: ManageCmd) -> Result<()> {
    let client = connect().await?;
    let plugins = client.plugin();
    match cmd {
        ManageCmd::List => list(plugins.list().await?),
        ManageCmd::Add { target, yes } => add::add(plugins, &target, yes).await,
        ManageCmd::Outdated { unsigned } => outdated::run(plugins, unsigned).await,
        ManageCmd::Update { id, all: _, yes } => update::run(plugins, id.as_deref(), yes).await,
        ManageCmd::Browse { query, repo } => browse::run(plugins, query, repo.as_deref()).await,
        ManageCmd::Repo { cmd } => store::run(cmd, plugins).await,
        ManageCmd::Info { id } => info(&plugins.info(&id).await?),
        ManageCmd::Config {
            id,
            key,
            value,
            unset,
        } => config::run(plugins, &id, key.as_deref(), value.as_deref(), unset).await,
        ManageCmd::Enable { id } => {
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
        ManageCmd::Disable { id } => {
            plugins.disable(&id).await?;
            ui::show(View::line(format!("'{id}' disabled")))
        }
        ManageCmd::Remove { id, yes } => {
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
                    if p.official {
                        format!("{} (official)", p.manifest.name)
                    } else {
                        p.manifest.name
                    },
                    p.manifest.version,
                    state_label(p.state).to_string(),
                    if p.obsolete {
                        format!("{} (obsolete)", source_label(p.source))
                    } else {
                        source_label(p.source).to_string()
                    },
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
    if let Some(signer) = &plugin.signer {
        rows.push(("Signer", signer.clone()));
    }
    if plugin.official {
        rows.push(("Official", "signed by the official plugins key".to_string()));
    }
    if let Some(repository) = &plugin.repository {
        rows.push(("Repository", repository.clone()));
    }
    if plugin.obsolete {
        rows.push((
            "Obsolete",
            "no repository you added lists it any more; it gets no updates".to_string(),
        ));
    }
    if !manifest.description.is_empty() {
        rows.push(("Description", manifest.description.clone()));
    }
    if !manifest.authors.is_empty() {
        rows.push(("Authors", manifest.authors.join(", ")));
    }
    let permissions: Vec<String> = plugin
        .grant
        .permissions
        .iter()
        .map(ToString::to_string)
        .collect();
    rows.push((
        "Permissions",
        if permissions.is_empty() {
            "none".to_string()
        } else {
            permissions.join(", ")
        },
    ));
    if let Some(error) = &plugin.last_error {
        rows.push(("Last error", error.clone()));
    }
    if let Some(bytes) = plugin.data_bytes {
        rows.push(("Data", ui::human_bytes(bytes)));
    }
    if let Some(developer) = &plugin.developer {
        rows.push(("Grant digest", developer.grant_digest.clone()));
        rows.push(("Component cache", developer.cache_dir.clone()));
        if developer.error_chain.len() > 1 {
            rows.push(("Error chain", developer.error_chain.join("\n  caused by: ")));
        }
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
        PluginState::NeedsUpdate => "needs update",
    }
}

fn source_label(source: PluginSource) -> &'static str {
    match source {
        PluginSource::File => "file",
        PluginSource::Folder => "folder",
        PluginSource::Url => "url",
        PluginSource::Repo => "repository",
        PluginSource::Dev => "dev",
    }
}
