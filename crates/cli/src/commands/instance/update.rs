//! `hestia instance update` — move a stopped instance to another version. The
//! new files download at the next launch; a downgrade confirms first.

use std::sync::Arc;

use anyhow::Result;
use client::proto::instance::InstanceUpdateParams;
use client::Client;

use super::entry;
use crate::commands::mc;
use crate::commands::modpack;
use crate::ui::{self, ProvisionReporter, Spinner, View};

pub(super) async fn run(
    client: &Client,
    instance: String,
    version: Option<String>,
    loader: Option<String>,
    downgrade: bool,
    keep_pack: bool,
) -> Result<()> {
    let info = entry::pick_instance(client.instance().list().await?, Some(instance))?;
    let versions = {
        let _spinner = Spinner::start("fetching versions");
        client.instance().versions(&info.flavor).await?
    };
    let version = mc::pick_version(versions.clone(), version)?;
    let is_downgrade =
        client::proto::minecraft::downgrade_between(&versions, &info.game_version, &version)
            == Some(true);
    if is_downgrade && !downgrade {
        mc::confirm_downgrade(
            &info.name,
            "saves",
            "nothing is backed up first (instances have no backups)",
            &info.game_version,
            &version,
        )?;
    }

    let reporter = Arc::new(ProvisionReporter::new());
    let progress = reporter.clone();
    let params = InstanceUpdateParams {
        instance: info.id,
        version,
        loader_version: loader,
        allow_downgrade: downgrade || is_downgrade,
        pack: modpack::pack_policy(keep_pack),
        id: String::new(),
    };
    let result = client
        .instance()
        .update(params, move |p| progress.update(p))
        .await;
    reporter.finish();
    let done = result?;
    ui::show(View::line(format!(
        "instance '{}' updated to {} (files download at the next launch)",
        done.instance.name, done.instance.game_version
    )))?;
    entry::show_info(&done.instance)?;
    modpack::show_failures(&done.failures)?;
    ui::show_warnings(&done.warnings)
}
