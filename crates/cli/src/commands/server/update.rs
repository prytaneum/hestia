//! `hestia server update` — move a server to another version. A running server
//! confirms a stop-update-start; a downgrade confirms separately.

use std::sync::Arc;

use anyhow::{bail, Context, Result};
use client::proto::error::{EntryKind, EntryTarget};
use client::proto::modpack::ModpackMove;
use client::proto::server::ServerUpdateParams;
use client::Client;

use super::{entry, lifecycle};
use crate::commands::mc;
use crate::commands::modpack;
use crate::ui::{self, ProvisionReporter, Spinner, View};

pub(super) async fn run(
    client: &Client,
    server: String,
    version: Option<String>,
    loader: Option<String>,
    downgrade: bool,
    restart: bool,
    keep_pack: bool,
) -> Result<()> {
    let info = entry::pick_server(client.server().list().await?, Some(server))?;
    let was_running = entry::running_process(&info).is_some();
    if was_running && !restart {
        confirm_update_restart(&info.name)?;
    }
    let versions = {
        let _spinner = Spinner::start("fetching versions");
        client.server().versions(&info.flavor).await?
    };
    let version = mc::pick_version(versions.clone(), version)?;
    let is_downgrade =
        client::proto::minecraft::downgrade_between(&versions, &info.game_version, &version)
            == Some(true);
    if is_downgrade && !downgrade {
        mc::confirm_downgrade(
            &info.name,
            "a world",
            "the current data is backed up automatically first",
            &info.game_version,
            &version,
        )?;
    }
    let entry = EntryTarget::new(EntryKind::Server, &info.id);
    let pack_version = modpack::pack_move(client, &entry, &version, keep_pack).await?;

    if was_running {
        {
            let _spinner = Spinner::start(format!("stopping '{}'", info.name));
            client.server().stop(&info.id).await?;
            lifecycle::wait_until_stopped(client, &info.id).await?;
        }
        ui::show(View::note(format!(
            "server '{}' stopped for the update",
            info.name
        )))?;
    }

    let allow_downgrade = downgrade || is_downgrade;
    match pack_version {
        Some(pack_version) => {
            let to = ModpackMove {
                version: pack_version,
                allow_downgrade,
                ..ModpackMove::default()
            };
            modpack::update(client, &entry, to).await?;
        }
        None => change_version(client, &info.id, version, loader, allow_downgrade).await?,
    }
    if was_running {
        lifecycle::start(client, &info.id).await?;
    }
    Ok(())
}

async fn change_version(
    client: &Client,
    server: &str,
    version: String,
    loader: Option<String>,
    allow_downgrade: bool,
) -> Result<()> {
    let reporter = Arc::new(ProvisionReporter::new());
    let progress = reporter.clone();
    let params = ServerUpdateParams {
        server: server.to_string(),
        version,
        loader_version: loader,
        allow_downgrade,
        id: String::new(),
    };
    let result = client
        .server()
        .update(params, move |p| progress.update(p))
        .await;
    reporter.finish();
    let done = result?;
    ui::show(View::line(format!(
        "server '{}' updated to {}",
        done.server.name, done.server.game_version
    )))?;
    entry::show_status(&done.server, None)?;
    ui::show_warnings(&done.warnings)
}

/// Interactive fallback for a missing `--restart`; errors when stdin is not a
/// terminal so scripts must pass the flag explicitly.
fn confirm_update_restart(name: &str) -> Result<()> {
    let restart = ui::confirm(
        &format!("server '{name}' is running and must restart to update"),
        "stop, update, and start again",
        "cancel",
    )
    .context("pass --restart to update a running server")?;
    if !restart {
        bail!("update cancelled");
    }
    Ok(())
}
