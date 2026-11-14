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
mod sdk;
mod sign;
mod store;
mod test;
mod update;
mod watch;
mod world;

use anyhow::{bail, Context, Result};
use clap::Subcommand;
use client::proto::plugin::{PluginInfo, PluginSource, PluginState};
use serde_json::json;

use crate::commands::connect;
use crate::ui::{self, Cell, View};

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
        #[command(flatten)]
        sdk: sdk::SdkOption,
    },
    /// Run a plugin from its folder, reload it on every rebuild and stream its log
    Dev {
        /// A plugin folder of the repository, the current directory by default;
        /// several folders of one repository with --once
        dirs: Vec<std::path::PathBuf>,
        #[arg(short = 'y', long, help = "Approve exactly the permissions shown")]
        yes: bool,
        #[arg(
            long,
            help = "Build and install, then return instead of watching for changes"
        )]
        once: bool,
        #[command(flatten)]
        sdk: sdk::SdkOption,
    },
    /// Check that the repository follows the one shape plugin repositories have,
    /// report every plugin's result, and type-check the plugins' code
    Check {
        /// Plugins to check; all of the repository's by default
        ids: Vec<String>,
        /// A folder inside the repository; the current directory by default
        #[arg(long, value_name = "DIR")]
        dir: Option<std::path::PathBuf>,
        #[command(flatten)]
        sdk: sdk::SdkOption,
    },
    /// Run the plugins' own tests on this computer, against the SDK's mock host
    Test {
        /// Plugins to test; all of the repository's by default
        ids: Vec<String>,
        #[command(flatten)]
        sdk: sdk::SdkOption,
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
        #[command(flatten)]
        sdk: sdk::SdkOption,
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
        PluginCmd::Pack { ids, sign, sdk } => pack::run(&ids, sign.as_deref(), &sdk),
        PluginCmd::Dev {
            dirs,
            yes,
            once,
            sdk,
        } => dev::run(&dirs, dev::Mode { yes, once }, &sdk).await,
        PluginCmd::Check { ids, dir, sdk } => check::run(dir.as_deref(), &ids, &sdk),
        PluginCmd::Test { ids, sdk } => test::run(&ids, &sdk),
        PluginCmd::Publish {
            out,
            sign,
            repository,
            sdk,
        } => publish::run(&out, &sign, repository, &sdk),
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
        return ui::show(View::empty("no plugins installed"));
    }
    ui::show(View::table(
        "Plugins",
        ["ID", "NAME", "VERSION", "STATE", "SOURCE"],
        plugins
            .into_iter()
            .map(|p| {
                vec![
                    p.manifest.id.into(),
                    if p.official {
                        format!("{} (official)", p.manifest.name).into()
                    } else {
                        p.manifest.name.into()
                    },
                    p.manifest.version.into(),
                    Cell::token(state_label(p.state), &p.state),
                    Cell::token(
                        if p.obsolete {
                            format!("{} (obsolete)", source_label(p.source))
                        } else {
                            source_label(p.source).to_string()
                        },
                        &p.source,
                    ),
                ]
            })
            .collect(),
    ))
}

fn info(plugin: &PluginInfo) -> Result<()> {
    let manifest = &plugin.manifest;
    let mut rows: Vec<(&str, Cell)> = vec![
        ("ID", manifest.id.clone().into()),
        ("Name", manifest.name.clone().into()),
        ("Version", manifest.version.clone().into()),
        (
            "State",
            Cell::token(state_label(plugin.state), &plugin.state),
        ),
        (
            "Source",
            Cell::token(source_label(plugin.source), &plugin.source),
        ),
    ];
    if let Some(signer) = &plugin.signer {
        rows.push(("Signer", signer.clone().into()));
    }
    if plugin.official {
        rows.push((
            "Official",
            Cell::flag(true, "signed by the official plugins key"),
        ));
    }
    if let Some(repository) = &plugin.repository {
        rows.push(("Repository", repository.clone().into()));
    }
    if plugin.obsolete {
        rows.push((
            "Obsolete",
            Cell::flag(
                true,
                "no repository you added lists it any more; it gets no updates",
            ),
        ));
    }
    if !manifest.description.is_empty() {
        rows.push(("Description", manifest.description.clone().into()));
    }
    if !manifest.authors.is_empty() {
        rows.push((
            "Authors",
            Cell::list(manifest.authors.join(", "), &manifest.authors),
        ));
    }
    let permissions: Vec<String> = plugin
        .grant
        .permissions
        .keys()
        .map(ToString::to_string)
        .collect();
    rows.push((
        "Permissions",
        Cell::list(
            if permissions.is_empty() {
                "none".to_string()
            } else {
                permissions.join(", ")
            },
            &permissions,
        ),
    ));
    if let Some(error) = &plugin.last_error {
        rows.push(("Last error", error.clone().into()));
    }
    if let Some(bytes) = plugin.data_bytes {
        rows.push(("Data", Cell::bytes(bytes)));
    }
    if let Some(developer) = &plugin.developer {
        rows.push(("Grant digest", developer.grant_digest.clone().into()));
        rows.push(("Component cache", developer.cache_dir.clone().into()));
        if developer.error_chain.len() > 1 {
            rows.push((
                "Error chain",
                Cell::list(
                    developer.error_chain.join("\n  caused by: "),
                    &developer.error_chain,
                ),
            ));
        }
    }
    rows.push((
        "Calls",
        Cell::typed(
            format!(
                "{} ({} traps, {} timeouts)",
                plugin.calls, plugin.traps, plugin.timeouts
            ),
            json!({ "calls": plugin.calls, "traps": plugin.traps, "timeouts": plugin.timeouts }),
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
