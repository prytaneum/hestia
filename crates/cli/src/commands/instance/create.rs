//! `hestia instance create` — the wizard for whatever is missing, or the fully
//! specified flag path, and the create-time JVM settings. The record is cheap;
//! files materialise at the first launch.

use anyhow::{bail, Result};
use client::proto::error::EntryKind;
use client::proto::minecraft::ConfigEntry;
use client::Client;

use super::entry;
use crate::commands::mc;
use crate::commands::wizard::{self, Field, WizardOutcome, WizardSeed};
use crate::ui::{self, Spinner, View};

pub(super) async fn run(
    client: &Client,
    flavor: Option<String>,
    version: Option<String>,
    loader: Option<String>,
    name: Option<String>,
    memory: Option<String>,
) -> Result<()> {
    let fully_specified = flavor.is_some() && version.is_some() && name.is_some();
    if ui::is_interactive() && !fully_specified {
        return run_wizard(client, flavor, version, loader, name, memory).await;
    }
    let flavors = {
        let _spinner = Spinner::start("fetching flavors");
        client.entry().flavors(EntryKind::Instance).await?
    };
    let flavor = mc::pick_flavor(flavors, flavor)?;
    let versions = {
        let _spinner = Spinner::start("fetching versions");
        client
            .entry()
            .versions(EntryKind::Instance, &flavor)
            .await?
    };
    let version = mc::pick_version(versions, version)?;
    let name = name.unwrap_or_else(|| format!("{flavor}-{version}"));
    let config = memory
        .map(|memory| ConfigEntry {
            key: "memory".into(),
            value: memory,
        })
        .into_iter()
        .collect();

    let instance = {
        let _spinner = Spinner::start("resolving profile");
        client
            .instance()
            .create(&name, &flavor, &version, loader, config)
            .await?
    };
    ui::show(View::line(format!("instance '{}' created", instance.name)))?;
    entry::show_info(&instance)
}

/// The interactive path: the fullscreen step wizard, prefilled from whatever
/// flags were given.
async fn run_wizard(
    client: &Client,
    flavor: Option<String>,
    version: Option<String>,
    loader: Option<String>,
    name: Option<String>,
    memory: Option<String>,
) -> Result<()> {
    let flavors = {
        let _spinner = Spinner::start("fetching flavors");
        client.entry().flavors(EntryKind::Instance).await?
    };
    if let Some(flavor) = &flavor {
        if !flavors.iter().any(|f| &f.id == flavor) {
            let ids: Vec<&str> = flavors.iter().map(|f| f.id.as_str()).collect();
            bail!("unknown flavor '{flavor}' (available: {})", ids.join(", "));
        }
    }
    let seed = WizardSeed {
        kind: EntryKind::Instance,
        flavors,
        flavor,
        version,
        name,
        loader,
        eula: true,
        fields: vec![Field::text("memory", "memory", "JVM default", memory)],
        extra: Vec::new(),
    };
    match wizard::run(client, seed).await? {
        None => ui::show(View::note("cancelled")),
        Some(WizardOutcome::Instance(instance)) => {
            ui::show(View::line(format!("instance '{}' created", instance.name)))?;
            entry::show_info(&instance)
        }
        Some(WizardOutcome::Server(_)) => unreachable!("instance wizard created a server"),
    }
}
