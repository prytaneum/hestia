use std::path::Path;

use anyhow::{bail, Context, Result};
use client::proto::plugin::{Grant, Permission, PluginInspection, Tier};

use crate::ui::{self, Spinner, View};

pub async fn add(plugins: client::Plugin<'_>, path: &Path, yes: bool) -> Result<()> {
    let path = std::fs::canonicalize(path)
        .with_context(|| format!("cannot resolve {}", path.display()))?
        .to_string_lossy()
        .into_owned();
    let inspection = plugins.inspect(&path).await?;
    show_grant(&inspection)?;
    if !yes {
        let install = ui::confirm("install it?", "install", "cancel")
            .context("pass --yes to approve exactly what is shown")?;
        if !install {
            bail!("install cancelled");
        }
    }
    let id = &inspection.manifest.id;
    let plugin = {
        let _spinner = Spinner::start(format!("installing {id}"));
        plugins
            .install(&path, &inspection.grant_digest, |_| {})
            .await?
            .plugin
    };
    ui::show(View::line(format!(
        "'{id}' {} installed",
        plugin.manifest.version
    )))
}

fn show_grant(inspection: &PluginInspection) -> Result<()> {
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
    let rows = grant_rows(grant);
    if rows.is_empty() {
        return ui::show(View::note(match inspection.replaces {
            Some(_) => "asks for nothing new",
            None => "asks for no permissions",
        }));
    }
    if grant
        .permissions
        .iter()
        .any(|permission| permission.tier() == Tier::Unrestricted)
    {
        ui::show(View::warning("asks for full access to your computer"))?;
    }
    ui::show(View::detail(rows))
}

fn grant_rows(grant: &Grant) -> Vec<(&'static str, String)> {
    let (unrestricted, sandboxed): (Vec<&Permission>, Vec<&Permission>) = grant
        .permissions
        .iter()
        .partition(|permission| permission.tier() == Tier::Unrestricted);
    let line = |permission: &Permission| format!("{permission} — {}", permission.description());
    let mut rows: Vec<(&'static str, String)> = unrestricted
        .into_iter()
        .map(|permission| ("Full access", line(permission)))
        .chain(
            sandboxed
                .into_iter()
                .map(|permission| ("Permission", line(permission))),
        )
        .collect();
    rows.extend(grant.programs.iter().map(|program| {
        let mut line = std::iter::once(&program.command)
            .chain(&program.args)
            .cloned()
            .collect::<Vec<_>>()
            .join(" ");
        if !program.env.is_empty() {
            let env: Vec<&str> = program.env.iter().map(String::as_str).collect();
            line = format!("{line} (env: {})", env.join(", "));
        }
        ("Program", format!("{}: {line}", program.id))
    }));
    let lists = [
        ("Network", &grant.network),
        ("Launch properties", &grant.launch_properties),
        ("Hooks", &grant.hooks),
        ("Events", &grant.events),
        ("Secret settings", &grant.secret_settings),
        ("Themes", &grant.ui_css),
    ];
    for (label, items) in lists {
        if !items.is_empty() {
            let items: Vec<&str> = items.iter().map(String::as_str).collect();
            rows.push((label, items.join(", ")));
        }
    }
    rows
}
