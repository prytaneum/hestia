//! `hestia plugin browse` — everything the configured repositories offer.

use anyhow::Result;
use client::proto::catalogue::{Availability, CatalogueParams, CataloguePlugin};

use super::store;
use crate::ui::{self, Spinner, View};

pub async fn run(
    plugins: client::Plugin<'_>,
    query: Option<String>,
    repo: Option<&str>,
) -> Result<()> {
    let repo = match repo {
        Some(reference) => Some(store::resolve(&plugins.repo_list().await?, reference)?.url),
        None => None,
    };
    let catalogue = {
        let _spinner = Spinner::start("loading the catalogue");
        plugins.catalogue(CatalogueParams { repo, query }).await?
    };
    if catalogue.repositories.is_empty() {
        return ui::show(View::note(
            "no plugin repositories; add one with `hestia plugin repo add`",
        ));
    }
    if catalogue.plugins.is_empty() {
        ui::show(View::note("no plugins match"))?;
    } else {
        ui::show(View::table(
            "Plugins",
            ["ID", "NAME", "VERSION", "STORE", "KEY ID", "STATUS"],
            catalogue.plugins.iter().map(row).collect(),
        ))?;
    }
    ui::show_warnings(&catalogue.warnings)
}

fn row(plugin: &CataloguePlugin) -> Vec<String> {
    let stores: Vec<&str> = plugin.listed_by.iter().map(|l| l.label.as_str()).collect();
    vec![
        plugin.id.clone(),
        plugin.name.clone(),
        plugin.version.clone(),
        stores.join(", "),
        plugin.key_id.clone(),
        status(plugin),
    ]
}

pub fn status(plugin: &CataloguePlugin) -> String {
    if let Some(installed) = &plugin.installed {
        return format!("installed {installed}");
    }
    let older = plugin
        .build
        .as_ref()
        .map(|version| format!(", {version} installable"))
        .unwrap_or_default();
    match plugin.availability {
        Availability::Prebuilt => "available".to_string(),
        Availability::Unreleased => format!("unreleased{older}"),
        Availability::Incompatible => format!(
            "incompatible (needs {}){older}",
            plugin.requires.as_deref().unwrap_or("another API")
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_status_says_why_a_plugin_cannot_be_installed() {
        let plugin = |availability, build: Option<&str>| CataloguePlugin {
            availability,
            build: build.map(str::to_string),
            requires: Some("^2.0.0".to_string()),
            ..CataloguePlugin::default()
        };

        assert_eq!(
            status(&plugin(Availability::Prebuilt, Some("1.0.0"))),
            "available"
        );
        assert_eq!(
            status(&plugin(Availability::Unreleased, None)),
            "unreleased"
        );
        assert_eq!(
            status(&plugin(Availability::Incompatible, Some("0.9.0"))),
            "incompatible (needs ^2.0.0), 0.9.0 installable"
        );
        let installed = CataloguePlugin {
            installed: Some("0.1.0".to_string()),
            ..CataloguePlugin::default()
        };
        assert_eq!(status(&installed), "installed 0.1.0");
    }
}
