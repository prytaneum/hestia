//! `hestia instance update` — move a stopped instance to another version. The
//! new files download at the next launch; a downgrade confirms first.

use anyhow::Result;
use client::proto::error::EntryKind;
use client::proto::instance::InstanceUpdateParams;
use client::Client;

use super::entry;
use crate::commands::mc;
use crate::commands::modpack;
use crate::ui::{self, ProvisionReporter, View};

pub(super) async fn run(
    client: &Client,
    instance: String,
    version: Option<String>,
    loader: Option<String>,
    downgrade: bool,
    keep_pack: bool,
) -> Result<()> {
    let info = entry::pick_instance(client.instance().list().await?, Some(instance))?;
    let versions = mc::fetch_versions(client, EntryKind::Instance, &info.flavor).await?;
    let version = mc::pick_version(versions.clone(), version)?;
    let is_downgrade =
        client::proto::minecraft::downgrade_between(&versions, &info.game_version, &version)
            == Some(true);
    if is_downgrade && !downgrade {
        mc::confirm_downgrade(
            client::proto::error::EntryKind::Instance,
            &info.name,
            &info.game_version,
            &version,
        )?;
    }

    let params = InstanceUpdateParams {
        instance: info.id,
        version,
        loader_version: loader,
        allow_downgrade: downgrade || is_downgrade,
        pack: modpack::pack_policy(keep_pack),
        id: String::new(),
    };
    let done = ProvisionReporter::run(None, async |on_progress| {
        client.instance().update(params, on_progress).await
    })
    .await?;
    ui::show(View::line(format!(
        "instance '{}' updated to {} (files download at the next launch)",
        done.instance.name, done.instance.game_version
    )))?;
    entry::show_info(&done.instance)?;
    modpack::show_failures(&done.failures)?;
    ui::show_warnings(&done.warnings)
}
