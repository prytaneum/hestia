use std::collections::BTreeMap;

use anyhow::{bail, Context, Result};
use client::proto::catalogue::{Availability, CatalogueParams, CataloguePlugin};
use client::proto::plugin::{Grant, PermissionScope, PluginInspection, Tier};

use super::{browse, store};
use crate::ui::{self, Spinner, View};

/// What the user named, told apart without asking the daemon.
#[derive(Debug, PartialEq, Eq)]
enum Target {
    /// An https address that is not a package: a repository's index.
    Index,
    Package,
    Local,
    /// `<id>` or `<label>/<id>`, as a configured repository lists it.
    Listed,
}

fn classify(target: &str) -> Target {
    if target.contains("://") {
        let path = target.split(['?', '#']).next().unwrap_or(target);
        return if path.ends_with(".hplugin") {
            Target::Package
        } else {
            Target::Index
        };
    }
    if std::path::Path::new(target).exists() {
        Target::Local
    } else if target.starts_with("github.com/") {
        Target::Index
    } else {
        Target::Listed
    }
}

pub async fn add(plugins: client::Plugin<'_>, target: &str, yes: bool) -> Result<()> {
    match classify(target) {
        Target::Index => from_index(&plugins, target, yes).await,
        Target::Listed => from_listing(&plugins, target, yes).await,
        Target::Package => {
            let inspection = {
                let _spinner = Spinner::start("downloading");
                plugins.inspect_url(target).await?
            };
            install(&plugins, inspection, None, yes).await
        }
        Target::Local => {
            let path = resolve(target)?;
            let inspection = plugins.inspect(&path).await?;
            install(&plugins, inspection, Some(&path), yes).await
        }
    }
}

async fn install(
    plugins: &client::Plugin<'_>,
    inspection: PluginInspection,
    local: Option<&str>,
    yes: bool,
) -> Result<()> {
    show_grant(&inspection)?;
    if !yes {
        let install = ui::confirm("install it?", "install", "cancel")
            .context("pass --yes to approve exactly what is shown")?;
        if !install {
            bail!("install cancelled");
        }
    }
    let id = &inspection.manifest.id;
    let digest = &inspection.grant_digest;
    let done = {
        let _spinner = Spinner::start(format!("installing {id}"));
        match (&inspection.staged, local) {
            (Some(staged), _) => plugins.install_staged(staged, digest, |_| {}).await?,
            (None, Some(path)) => plugins.install(path, digest, |_| {}).await?,
            (None, None) => bail!("nothing to install"),
        }
    };
    ui::show(View::line(format!(
        "'{id}' {} installed",
        done.plugin.manifest.version
    )))?;
    ui::show_warnings(&done.warnings)
}

async fn from_listing(plugins: &client::Plugin<'_>, reference: &str, yes: bool) -> Result<()> {
    let (label, id) = match reference.split_once('/') {
        Some((label, id)) => (Some(label), id),
        None => (None, reference),
    };
    let catalogue = {
        let _spinner = Spinner::start("loading the catalogue");
        plugins
            .catalogue(CatalogueParams {
                query: Some(id.to_string()),
                ..CatalogueParams::default()
            })
            .await?
    };
    let offered = offers(&catalogue.plugins, label, id);
    let chosen = match offered.as_slice() {
        [] => bail!(
            "no configured repository lists '{reference}'; see `hestia plugin browse` and `hestia plugin repo add`"
        ),
        [one] => *one,
        many => {
            let choices: Vec<String> = many.iter().map(|plugin| describe(plugin)).collect();
            if !ui::is_interactive() {
                bail!(
                    "'{id}' is offered under several keys; name one as <label>/<id> or run it in a terminal:\n  {}",
                    choices.join("\n  ")
                );
            }
            many[ui::select(&format!("which '{id}'?"), &choices)?]
        }
    };
    from_entry(plugins, chosen, yes).await
}

/// The catalogue entries for `id`, narrowed to a store label when one was given.
fn offers<'a>(
    catalogue: &'a [CataloguePlugin],
    label: Option<&str>,
    id: &str,
) -> Vec<&'a CataloguePlugin> {
    catalogue
        .iter()
        .filter(|plugin| plugin.id == id)
        .filter(|plugin| {
            label.is_none_or(|label| {
                plugin
                    .listed_by
                    .iter()
                    .any(|lister| lister.label.eq_ignore_ascii_case(label))
            })
        })
        .collect()
}

fn describe(plugin: &CataloguePlugin) -> String {
    let stores: Vec<&str> = plugin.listed_by.iter().map(|l| l.label.as_str()).collect();
    format!(
        "{}/{} {} (key {}): {}",
        stores.join(","),
        plugin.id,
        plugin.version,
        plugin.key_id,
        browse::status(plugin)
    )
}

async fn from_entry(
    plugins: &client::Plugin<'_>,
    plugin: &CataloguePlugin,
    yes: bool,
) -> Result<()> {
    if plugin.build.is_none() {
        bail!(
            "'{}' cannot be installed: {}",
            plugin.id,
            browse::status(plugin)
        );
    }
    if plugin.availability != Availability::Prebuilt {
        ui::show(View::warning(format!(
            "{} is {}; installing the newest build, {}",
            plugin.version,
            browse::status(plugin),
            plugin.build.as_deref().unwrap_or_default()
        )))?;
    }
    let repository = &plugin.listed_by[0].url;
    let inspection = {
        let _spinner = Spinner::start("downloading");
        plugins.inspect_repo(repository, &plugin.id).await?
    };
    install(plugins, inspection, None, yes).await
}

async fn from_index(plugins: &client::Plugin<'_>, url: &str, yes: bool) -> Result<()> {
    let repository = store::add(plugins, url, yes).await?;
    let catalogue = plugins
        .catalogue(CatalogueParams {
            repo: Some(repository.url.clone()),
            ..CatalogueParams::default()
        })
        .await?;
    let installable: Vec<&CataloguePlugin> = catalogue
        .plugins
        .iter()
        .filter(|plugin| plugin.listed_by.iter().any(|l| l.url == repository.url))
        .filter(|plugin| plugin.build.is_some())
        .collect();
    if installable.is_empty() {
        return ui::show(View::note("it has no installable plugins yet"));
    }
    let labels: Vec<String> = installable
        .iter()
        .map(|plugin| {
            format!(
                "{} {} ({})",
                plugin.id,
                plugin.version,
                browse::status(plugin)
            )
        })
        .collect();
    let picked = ui::multi_select("install which plugins?", &labels)
        .context("name a plugin instead: `hestia plugin add <label>/<id>`")?;
    for index in picked {
        from_entry(plugins, installable[index], yes).await?;
    }
    Ok(())
}

fn resolve(path: &str) -> Result<String> {
    Ok(std::fs::canonicalize(path)
        .with_context(|| format!("cannot resolve {path}"))?
        .to_string_lossy()
        .into_owned())
}

pub(super) fn show_grant(inspection: &PluginInspection) -> Result<()> {
    let manifest = &inspection.manifest;
    let (heading, grant) = match &inspection.replaces {
        Some(installed) => (
            format!(
                "update '{}' {} → {}",
                manifest.id, installed.manifest.version, manifest.version
            ),
            &inspection.widened,
        ),
        None => (
            format!("install '{}' {}", manifest.id, manifest.version),
            &inspection.grant,
        ),
    };
    ui::show(View::line(heading))?;
    let rows = grant_rows(grant, &inspection.program_paths);
    if rows.is_empty() {
        return ui::show(View::note(match inspection.replaces {
            Some(_) => "asks for nothing new",
            None => "asks for no permissions",
        }));
    }
    if !grant.unrestricted().is_empty() {
        ui::show(View::warning("asks for full access to your computer"))?;
    }
    ui::show(View::detail(rows))
}

fn grant_rows(
    grant: &Grant,
    program_paths: &BTreeMap<String, String>,
) -> Vec<(&'static str, String)> {
    let (unrestricted, sandboxed): (Vec<_>, Vec<_>) = grant
        .permissions
        .iter()
        .partition(|(permission, _)| permission.tier() == Tier::Unrestricted);
    let mut rows = Vec::new();
    for (heading, permissions) in [("Full access", unrestricted), ("Permission", sandboxed)] {
        for (permission, scope) in permissions {
            rows.push((
                heading,
                format!("{permission} — {}", permission.description()),
            ));
            rows.extend(scope_rows(scope));
        }
    }
    rows.extend(scope_rows(&grant.contributions));
    rows.extend(
        program_paths
            .iter()
            .map(|(id, path)| ("Pinned", format!("{id} → {path}"))),
    );
    rows
}

fn scope_rows(scope: &PermissionScope) -> impl Iterator<Item = (&'static str, String)> + '_ {
    scope.iter().map(|(kind, entries)| {
        let entries: Vec<&str> = entries.iter().map(String::as_str).collect();
        (kind.label(), entries.join(", "))
    })
}

#[cfg(test)]
mod tests {
    use client::proto::catalogue::Lister;

    use super::*;

    #[test]
    fn what_the_user_typed_is_told_apart() {
        assert_eq!(
            classify("https://example.com/aa-0.1.0.hplugin?x=1"),
            Target::Package
        );
        assert_eq!(
            classify("https://raw.githubusercontent.com/acme/tools/repo/plugins.json"),
            Target::Index
        );
        assert_eq!(classify("github.com/acme/tools"), Target::Index);
        assert_eq!(classify("SMP/aa"), Target::Listed);
        assert_eq!(classify("aa"), Target::Listed);
        assert_eq!(classify("."), Target::Local);
    }

    #[test]
    fn an_id_under_two_keys_has_two_offers_and_a_label_narrows_them() {
        let entry = |label: &str, key: &str| CataloguePlugin {
            id: "aa".to_string(),
            key_id: key.to_string(),
            listed_by: vec![Lister {
                label: label.to_string(),
                ..Lister::default()
            }],
            ..CataloguePlugin::default()
        };
        let catalogue = [entry("SMP", "K1"), entry("ACME", "K2")];

        assert_eq!(offers(&catalogue, None, "aa").len(), 2);
        let narrowed = offers(&catalogue, Some("acme"), "aa");
        assert_eq!(narrowed.len(), 1);
        assert_eq!(narrowed[0].key_id, "K2");
        assert!(offers(&catalogue, None, "bb").is_empty());
    }
}
