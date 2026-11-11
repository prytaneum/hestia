//! `hestia plugin update [id|--all]` — move installed plugins to their newest build.

use anyhow::{bail, Context, Result};
use client::proto::error::ErrorInfo;
use client::proto::plugin::{OutdatedPlugin, PluginUpdateDone};
use client::proto::warning::WarningInfo;

use super::add::show_grant;
use crate::ui::{self, Spinner, View};

pub async fn run(plugins: client::Plugin<'_>, id: Option<&str>, yes: bool) -> Result<()> {
    let outdated = {
        let _spinner = Spinner::start("checking for updates");
        plugins.outdated(id.is_some()).await?
    };
    let offers: Vec<&OutdatedPlugin> = outdated
        .plugins
        .iter()
        .filter(|offer| id.is_none_or(|id| offer.id == id))
        .collect();
    if offers.is_empty() {
        return ui::show(View::note(match id {
            Some(id) => format!("'{id}' is up to date"),
            None => "every plugin is up to date".to_string(),
        }));
    }
    let mut failed = 0;
    for offer in offers {
        if let Err(error) = update_one(&plugins, offer, yes).await {
            failed += 1;
            ui::show(View::warning(format!("{}: {error:#}", offer.id)))?;
        }
    }
    if failed > 0 {
        bail!("{failed} update(s) did not apply");
    }
    Ok(())
}

async fn update_one(plugins: &client::Plugin<'_>, offer: &OutdatedPlugin, yes: bool) -> Result<()> {
    if !offer.api_compatible {
        match &offer.requires {
            Some(api) => bail!("{} needs plugin API {api}", offer.available),
            None => bail!("{} needs another plugin API", offer.available),
        }
    }
    let id = &offer.id;
    let first = {
        let _spinner = Spinner::start(format!("updating {id}"));
        plugins.update(id, "", |_| {}).await
    };
    let done = match first {
        Ok(done) => done,
        Err(error)
            if matches!(
                client::error_info(&error),
                ErrorInfo::PluginUpdateNeedsApproval { .. }
            ) =>
        {
            update_with_approval(plugins, offer, yes).await?
        }
        Err(error) => return Err(error.into()),
    };
    report(id, &done)
}

async fn update_with_approval(
    plugins: &client::Plugin<'_>,
    offer: &OutdatedPlugin,
    yes: bool,
) -> Result<PluginUpdateDone> {
    let inspection = {
        let _spinner = Spinner::start("downloading");
        if offer.repository.is_empty() {
            let url = plugins.info(&offer.id).await?.url.unwrap_or_default();
            plugins.inspect_url(&url).await?
        } else {
            plugins.inspect_repo(&offer.repository, &offer.id).await?
        }
    };
    show_grant(&inspection)?;
    if !yes {
        let apply = ui::confirm("approve and update?", "update", "cancel")
            .context("pass --yes to approve exactly what is shown")?;
        if !apply {
            bail!("update cancelled");
        }
    }
    let _spinner = Spinner::start(format!("updating {}", offer.id));
    Ok(plugins
        .update(&offer.id, &inspection.grant_digest, |_| {})
        .await?)
}

fn report(id: &str, done: &PluginUpdateDone) -> Result<()> {
    let rolled_back = done
        .warnings
        .iter()
        .any(|warning| matches!(warning, WarningInfo::PluginUpdateRolledBack { .. }));
    ui::show_warnings(&done.warnings)?;
    if rolled_back {
        bail!(
            "the new version failed to load; '{id}' is still {}",
            done.plugin.manifest.version
        );
    }
    ui::show(View::line(format!(
        "'{id}' {} → {}",
        done.from, done.plugin.manifest.version
    )))
}
