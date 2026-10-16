//! `hestia sync …` — the settings shared across instances.

use anyhow::Result;
use clap::{Subcommand, ValueEnum};
use client::proto::sync::{InstanceSyncStatus, SyncConfig, SyncUnit, UnitState};
use client::Client;

use crate::ui::{self, View};

#[derive(Subcommand)]
pub enum SyncCmd {
    /// What is shared, and where each instance stands
    #[command(alias = "list", alias = "ls")]
    Status,
    /// Start sharing one thing, seeded from one instance's copy
    On {
        /// What to share
        unit: Unit,
        /// The instance whose copy the shared one starts from
        #[arg(long)]
        from: Option<String>,
    },
    /// Stop sharing one thing; every instance keeps the copy it has
    Off {
        /// What to stop sharing
        unit: Unit,
    },
    /// The shared game options
    Options {
        #[command(subcommand)]
        cmd: OptionsCmd,
    },
    /// The shared pack library
    Packs {
        #[command(subcommand)]
        cmd: PacksCmd,
    },
}

#[derive(Subcommand)]
pub enum PacksCmd {
    /// Every pack the instances share
    #[command(alias = "ls")]
    List,
    /// Load a pack in every instance that has it
    Enable { pack: String },
    /// Keep a pack installed but unloaded everywhere
    Disable { pack: String },
    /// Drop a pack from the library; every instance loses it at its next pass
    #[command(alias = "rm")]
    Remove { pack: String },
}

#[derive(Subcommand)]
pub enum OptionsCmd {
    /// Every shared setting and its value
    #[command(alias = "ls")]
    List,
    /// Change a shared setting
    Set { key: String, value: String },
    /// Stop sharing one setting; each instance keeps its own
    Local { key: String },
    /// Share one setting again
    Share { key: String },
}

#[derive(Clone, Copy, ValueEnum)]
pub enum Unit {
    Options,
    Servers,
    Commands,
    Hotbars,
    Screenshots,
    ResourcePacks,
}

impl Unit {
    pub(crate) fn proto(self) -> SyncUnit {
        match self {
            Unit::Options => SyncUnit::Options,
            Unit::Servers => SyncUnit::Servers,
            Unit::Commands => SyncUnit::Commands,
            Unit::Hotbars => SyncUnit::Hotbars,
            Unit::Screenshots => SyncUnit::Screenshots,
            Unit::ResourcePacks => SyncUnit::ResourcePacks,
        }
    }
}

pub async fn run(cmd: SyncCmd) -> Result<()> {
    let client = super::connect().await?;
    match cmd {
        SyncCmd::Status => status(&client).await,
        SyncCmd::On { unit, from } => enable(&client, unit.proto(), from).await,
        SyncCmd::Off { unit } => disable(&client, unit.proto()).await,
        SyncCmd::Options { cmd } => options(&client, cmd).await,
        SyncCmd::Packs { cmd } => packs(&client, cmd).await,
    }
}

async fn packs(client: &Client, cmd: PacksCmd) -> Result<()> {
    match cmd {
        PacksCmd::List => list_packs(client).await,
        PacksCmd::Enable { pack } => {
            client.sync().set_pack(&pack, true).await?;
            ui::show(View::line(format!("'{pack}' loads in every instance")))
        }
        PacksCmd::Disable { pack } => {
            client.sync().set_pack(&pack, false).await?;
            ui::show(View::line(format!(
                "'{pack}' stays installed but unloaded everywhere"
            )))
        }
        PacksCmd::Remove { pack } => {
            client.sync().remove_pack(&pack).await?;
            ui::show(View::line(format!(
                "'{pack}' is no longer shared; instances lose it at their next pass"
            )))
        }
    }
}

async fn list_packs(client: &Client) -> Result<()> {
    let packs = client.sync().packs().await?;
    if packs.is_empty() {
        return ui::show(View::note(
            "no packs shared yet — `hestia sync on resourcepacks` starts from an instance",
        ));
    }
    let rows = packs
        .into_iter()
        .map(|pack| {
            vec![
                pack.title,
                pack.kind.to_string(),
                pack.source,
                match pack.enabled {
                    true => "loaded".to_string(),
                    false => "not loaded".to_string(),
                },
            ]
        })
        .collect();
    ui::show(View::table(
        "Shared packs",
        ["PACK", "KIND", "FROM", ""],
        rows,
    ))
}

pub fn unit_name(unit: SyncUnit) -> &'static str {
    match unit {
        SyncUnit::Options => "options",
        SyncUnit::Servers => "servers",
        SyncUnit::Commands => "commands",
        SyncUnit::Hotbars => "hotbars",
        SyncUnit::Screenshots => "screenshots",
        SyncUnit::ResourcePacks => "resourcepacks",
    }
}

pub fn state_label(state: UnitState) -> &'static str {
    match state {
        UnitState::Synced => "synced",
        UnitState::Pending => "shares at next launch",
        UnitState::Overridden => "keeps its own",
        UnitState::Off => "not shared",
        UnitState::Unsupported => "version too old to share",
    }
}

async fn status(client: &Client) -> Result<()> {
    let config = client.sync().get().await?;
    ui::show(View::detail([(
        "shared store",
        config.shared_dir.display().to_string(),
    )]))?;
    render_units(&config)?;
    if config.units.iter().all(|unit| !unit.enabled) {
        return ui::show(View::note(
            "nothing is shared yet — `hestia sync on options` starts with your game settings",
        ));
    }
    if !config.unsynced.is_empty() {
        ui::show(View::note(format!(
            "settings kept per-instance: {}",
            config
                .unsynced
                .iter()
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        )))?;
    }
    render_instances(client.sync().status().await?)
}

fn render_units(config: &SyncConfig) -> Result<()> {
    let rows = config
        .units
        .iter()
        .map(|unit| {
            vec![
                unit_name(unit.unit).to_string(),
                match unit.enabled {
                    true => "shared".to_string(),
                    false => "not shared".to_string(),
                },
                unit.seeded_from.clone(),
            ]
        })
        .collect();
    ui::show(View::table(
        "Shared",
        ["WHAT", "SHARING", "STARTED FROM"],
        rows,
    ))
}

fn render_instances(instances: Vec<InstanceSyncStatus>) -> Result<()> {
    if instances.is_empty() {
        return Ok(());
    }
    let mut rows: Vec<Vec<String>> = Vec::new();
    for instance in instances {
        for unit in instance.units.iter().filter(|u| u.state != UnitState::Off) {
            rows.push(vec![
                instance.name.clone(),
                unit_name(unit.unit).to_string(),
                state_label(unit.state).to_string(),
            ]);
        }
    }
    if rows.is_empty() {
        return Ok(());
    }
    ui::show(View::table(
        "Instances",
        ["INSTANCE", "WHAT", "STATE"],
        rows,
    ))
}

/// The shared copy has to start as someone's, so a source is picked before the
/// call rather than letting the first launch decide.
async fn enable(client: &Client, unit: SyncUnit, from: Option<String>) -> Result<()> {
    let source = match from {
        Some(name) => name,
        None => pick_source(client, unit).await?,
    };
    let config = client.sync().enable(unit, &source).await?;
    let seeded = config
        .units
        .iter()
        .find(|entry| entry.unit == unit)
        .map(|entry| entry.seeded_from.clone())
        .unwrap_or_default();
    ui::show(View::line(match seeded.is_empty() {
        true => format!("your instances now share {unit}"),
        false => format!("your instances now share {unit}, starting from '{seeded}'"),
    }))
}

async fn pick_source(client: &Client, unit: SyncUnit) -> Result<String> {
    let candidates: Vec<_> = client
        .sync()
        .sources(unit)
        .await?
        .into_iter()
        .filter(|source| source.present)
        .collect();
    if candidates.len() < 2 {
        return Ok(String::new());
    }
    let names: Vec<String> = candidates.iter().map(|s| s.name.clone()).collect();
    let chosen = ui::select(&format!("Start {unit} from which instance?"), &names)?;
    Ok(names[chosen].clone())
}

async fn disable(client: &Client, unit: SyncUnit) -> Result<()> {
    client.sync().disable(unit).await?;
    ui::show(View::line(format!(
        "{unit} is no longer shared; every instance keeps the copy it has"
    )))
}

async fn options(client: &Client, cmd: OptionsCmd) -> Result<()> {
    match cmd {
        OptionsCmd::List => list_options(client).await,
        OptionsCmd::Set { key, value } => {
            client.sync().set_option(&key, &value).await?;
            ui::show(View::line(format!("shared '{key}' is now {value}")))
        }
        OptionsCmd::Local { key } => set_shared(client, &key, false).await,
        OptionsCmd::Share { key } => set_shared(client, &key, true).await,
    }
}

async fn list_options(client: &Client) -> Result<()> {
    let options = client.sync().options().await?;
    if options.is_empty() {
        return ui::show(View::note(
            "no shared settings yet — they arrive the first time an instance launches",
        ));
    }
    let rows = options
        .into_iter()
        .map(|option| {
            vec![
                option.key,
                option.value,
                match option.synced {
                    true => "shared".to_string(),
                    false => "per-instance".to_string(),
                },
            ]
        })
        .collect();
    ui::show(View::table("Shared options", ["KEY", "VALUE", ""], rows))
}

async fn set_shared(client: &Client, key: &str, shared: bool) -> Result<()> {
    let mut unsynced = client.sync().get().await?.unsynced;
    let changed = match shared {
        true => unsynced.remove(key),
        false => unsynced.insert(key.to_string()),
    };
    if !changed {
        return ui::show(View::note(match shared {
            true => format!("'{key}' is already shared"),
            false => format!("'{key}' is already per-instance"),
        }));
    }
    client.sync().set_unsynced(unsynced).await?;
    ui::show(View::line(match shared {
        true => format!("'{key}' is shared between your instances again"),
        false => format!("'{key}' is each instance's own from now on"),
    }))
}
