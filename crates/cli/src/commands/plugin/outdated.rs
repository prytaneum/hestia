//! `hestia plugin outdated` — installed plugins with a newer version on offer.

use anyhow::Result;
use client::proto::plugin::OutdatedPlugin;

use crate::ui::{self, Spinner, View};

pub async fn run(plugins: client::Plugin<'_>, unsigned: bool) -> Result<()> {
    let outdated = {
        let _spinner = Spinner::start("checking for updates");
        plugins.outdated(unsigned).await?
    };
    if outdated.plugins.is_empty() {
        ui::show(View::note("every plugin is up to date"))?;
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

fn row(plugin: &OutdatedPlugin) -> Vec<String> {
    let status = match (&plugin.requires, plugin.api_compatible) {
        (Some(api), false) => format!("needs plugin API {api}"),
        (None, false) => "needs another plugin API".to_string(),
        _ if plugin.widened.permissions.is_empty() => "ready".to_string(),
        _ => format!(
            "asks for {} more permissions",
            plugin.widened.permissions.len()
        ),
    };
    vec![
        plugin.id.clone(),
        plugin.installed.clone(),
        plugin.available.clone(),
        status,
    ]
}
