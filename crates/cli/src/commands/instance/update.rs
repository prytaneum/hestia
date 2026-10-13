//! `hestia instance update` — move a stopped instance to another version. The
//! new files download at the next launch; a downgrade confirms first.

use anyhow::Result;
use client::proto::modpack::ModpackMove;
use client::Client;

use super::entry;
use crate::commands::content::EntryKind;
use crate::commands::mc;
use crate::commands::modpack;
use crate::ui::{self, Spinner, View};

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
    let pack_version =
        modpack::pack_move(client, EntryKind::Instance, &info.id, &version, keep_pack).await?;
    if let Some(pack_version) = pack_version {
        let to = ModpackMove {
            version: pack_version,
            allow_downgrade: downgrade || is_downgrade,
            ..ModpackMove::default()
        };
        return modpack::update(client, EntryKind::Instance, &info.id, to).await;
    }

    let updated = {
        let _spinner = Spinner::start(format!("updating '{}' to {version}", info.name));
        client
            .instance()
            .update(&info.id, &version, loader, downgrade || is_downgrade)
            .await?
    };
    ui::show(View::line(format!(
        "instance '{}' updated to {} (files download at the next launch)",
        updated.name, updated.game_version
    )))?;
    entry::show_info(&updated)
}
