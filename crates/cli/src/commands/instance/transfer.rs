//! `hestia instance <name> export` and `hestia instance import` — moving an
//! instance out of the launcher as one file, and bringing one back.
//!
//! Paths are resolved against *this* process's working directory before they
//! are sent. The daemon is a separate process and refuses a relative path
//! outright, so `-o ./cozy.hestia` has to mean what the person typing it
//! expects rather than whatever the daemon's cwd happens to be.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use client::proto::transfer::{ArchiveInfo, ExportFormat, ExportFormatInfo, ImportFormat};
use client::Client;

use super::entry;
use crate::ui::{self, ProvisionReporter, View};

/// Export one instance. An empty destination lets the daemon file it under its
/// own `exports/`, which is the only directory it is sure it may write to.
pub(super) async fn export(
    client: &Client,
    instance: String,
    format: Option<String>,
    output: Option<PathBuf>,
    exclude: Vec<String>,
) -> Result<()> {
    let info = entry::pick_instance(client.instance().list().await?, Some(instance))?;
    let format = pick_format(client, format.as_deref()).await?;
    let destination = match output {
        Some(path) => absolute(&path)?.to_string_lossy().into_owned(),
        None => String::new(),
    };

    let exported = ProvisionReporter::run(None, async |on_progress| {
        client
            .transfer()
            .export(&info.id, format, &destination, exclude, on_progress)
            .await
    })
    .await?;

    ui::show(View::line(format!(
        "exported '{}' to {} ({} file(s), {})",
        info.name,
        exported.path,
        exported.files,
        ui::human_bytes(exported.size_bytes)
    )))?;
    ui::show_warnings(&exported.warnings)
}

/// Import an archive as a new instance. The format is detected, so this takes a
/// file and nothing else; what it turned out to be is reported before the work
/// starts, since an archive from elsewhere is often a surprise.
pub(super) async fn import(client: &Client, path: PathBuf, name: Option<String>) -> Result<()> {
    let path = absolute(&path)?;
    if !path.is_file() {
        bail!("no file at {}", path.display());
    }
    let path = path.to_string_lossy().into_owned();

    let archive = client.transfer().inspect(&path).await?;
    ui::show(View::line(describe(&archive)))?;
    let name = pick_name(&archive, name)?;

    let imported = ProvisionReporter::run(None, async |on_progress| {
        client.transfer().import(&path, &name, on_progress).await
    })
    .await?;

    ui::show(View::line(format!(
        "imported '{}' ({} {})",
        imported.instance.name, imported.instance.flavor, imported.instance.game_version
    )))?;
    if !imported.failures.is_empty() {
        ui::show(View::table(
            "Not installed",
            ["ITEM", "REASON"],
            imported
                .failures
                .iter()
                .map(|f| vec![f.title.clone(), f.error.to_string()])
                .collect(),
        ))?;
    }
    ui::show_warnings(&imported.warnings)
}

/// The name the new instance takes: the one asked for, else the archive's own
/// — prompted for when that is already taken, so a collision is answered
/// rather than merely reported.
fn pick_name(archive: &ArchiveInfo, requested: Option<String>) -> Result<String> {
    if let Some(name) = requested {
        return Ok(name);
    }
    if !archive.name_taken {
        return Ok(archive.name.clone());
    }
    let suggestion = format!("{} (imported)", archive.name);
    let answer = ui::input(
        &format!("'{}' already exists — name the import", archive.name),
        &suggestion,
    )?;
    match answer.trim().is_empty() {
        true => Ok(suggestion),
        false => Ok(answer.trim().to_string()),
    }
}

fn describe(archive: &ArchiveInfo) -> String {
    let loader = match archive.loader.is_empty() {
        true => "vanilla".to_string(),
        false => format!("{} {}", archive.loader, archive.loader_version),
    };
    format!(
        "{} archive: '{}' — {} {}",
        source_of(&archive.format),
        archive.name,
        loader,
        archive.game_version
    )
}

fn source_of(format: &ImportFormat) -> &str {
    match format {
        ImportFormat::Hestia => "hestia",
        ImportFormat::Provider(provider) => &provider.name,
    }
}

/// The format `value` names by id, extension or name, among those the daemon
/// offers — a plugin's formats are known to the daemon alone.
async fn pick_format(client: &Client, value: Option<&str>) -> Result<ExportFormat> {
    let Some(value) = value else {
        return Ok(ExportFormat::default());
    };
    let wanted = value.to_ascii_lowercase();
    let offered = client.transfer().export_formats().await?;
    let named = |info: &&ExportFormatInfo| {
        let name = match &info.format {
            ExportFormat::Hestia => "hestia",
            ExportFormat::Provider(provider) => provider.name.as_str(),
        };
        [info.format.id(), info.extension.as_str(), name]
            .iter()
            .any(|candidate| candidate.eq_ignore_ascii_case(&wanted))
    };
    match offered.iter().find(named) {
        Some(info) => Ok(info.format.clone()),
        None => {
            let ids: Vec<&str> = offered.iter().map(|info| info.format.id()).collect();
            anyhow::bail!("unknown export format '{value}' ({})", ids.join(", "))
        }
    }
}

/// Resolve against this process's working directory, since the daemon's own is
/// unrelated to the caller's.
fn absolute(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    let cwd = std::env::current_dir().context("cannot read the working directory")?;
    Ok(cwd.join(path))
}
