//! `hestia plugin outdated` — installed plugins with a newer version on offer.

use anyhow::Result;
use client::proto::plugin::OutdatedPlugin;

use crate::ui::{self, Cell, Spinner, View};

pub async fn run(plugins: client::Plugin<'_>, unsigned: bool) -> Result<()> {
    let outdated = {
        let _spinner = Spinner::start("checking for updates");
        plugins.outdated(unsigned).await?
    };
    if outdated.plugins.is_empty() {
        ui::show(View::empty("every plugin is up to date"))?;
    } else {
        ui::show(View::table(
            "Outdated plugins",
            ["ID", "INSTALLED", "AVAILABLE", "STATUS"],
            outdated.plugins.iter().map(row).collect(),
        ))?;
    }
    if outdated.offline {
        ui::show(View::note(
            "offline: answered from the last repository listings",
        ))?;
    }
    ui::show_warnings(&outdated.warnings)
}

fn row(plugin: &OutdatedPlugin) -> Vec<Cell> {
    let status = match (&plugin.requires, plugin.api_compatible) {
        (Some(api), false) => Cell::token(format!("needs plugin API {api}"), &"needs_api"),
        (None, false) => Cell::token("needs another plugin API", &"needs_api"),
        _ if plugin.widened.permissions.is_empty() => Cell::token("ready", &"ready"),
        _ => Cell::token("asks for more access", &"needs_approval"),
    };
    vec![
        plugin.id.clone().into(),
        plugin.installed.clone().into(),
        plugin.available.clone().into(),
        status,
    ]
}
