//! `hestia instance <name> export` and `hestia instance import` — moving an
//! instance out of the launcher as one file, and bringing one back.
//!
//! Paths are resolved against *this* process's working directory before they
//! are sent. The daemon is a separate process and refuses a relative path
//! outright, so `-o ./cozy.hestia` has to mean what the person typing it
//! expects rather than whatever the daemon's cwd happens to be.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use client::proto::transfer::{ArchiveInfo, ExportFormat, ImportFormat};
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
    let format = parse_format(format.as_deref())?;
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
        source_of(archive.format),
        archive.name,
        loader,
        archive.game_version
    )
}

fn source_of(format: ImportFormat) -> &'static str {
    match format {
        ImportFormat::Hestia => "a hestia",
        ImportFormat::Mrpack => "a Modrinth pack",
        ImportFormat::Prism => "a Prism/MultiMC",
        ImportFormat::CurseForge => "a CurseForge pack",
    }
}

fn parse_format(value: Option<&str>) -> Result<ExportFormat> {
    let Some(value) = value else {
        return Ok(ExportFormat::default());
    };
    ExportFormat::parse(&value.to_ascii_lowercase())
        .with_context(|| format!("unknown export format '{value}' (hestia, mrpack)"))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_parse_by_name_and_default_to_the_native_one() {
        assert_eq!(parse_format(None).unwrap(), ExportFormat::Hestia);
        assert_eq!(parse_format(Some("hestia")).unwrap(), ExportFormat::Hestia);
        assert_eq!(parse_format(Some("mrpack")).unwrap(), ExportFormat::Mrpack);
        assert_eq!(
            parse_format(Some("modrinth")).unwrap(),
            ExportFormat::Mrpack
        );
        assert!(parse_format(Some("curseforge")).is_err());
    }

    #[test]
    fn a_requested_name_wins_and_a_free_one_is_taken_as_is() {
        let archive = ArchiveInfo {
            name: "Cozy".to_string(),
            name_taken: false,
            ..ArchiveInfo::default()
        };
        assert_eq!(
            pick_name(&archive, Some("Mine".to_string())).unwrap(),
            "Mine"
        );
        assert_eq!(pick_name(&archive, None).unwrap(), "Cozy");
    }

    #[test]
    fn a_description_names_the_source_and_the_loader() {
        let prism = ArchiveInfo {
            format: ImportFormat::Prism,
            name: "Cozy".to_string(),
            game_version: "1.21.1".to_string(),
            loader: "fabric".to_string(),
            loader_version: "0.16.9".to_string(),
            name_taken: false,
        };
        assert_eq!(
            describe(&prism),
            "a Prism/MultiMC archive: 'Cozy' — fabric 0.16.9 1.21.1"
        );

        let vanilla = ArchiveInfo {
            format: ImportFormat::Hestia,
            loader: String::new(),
            ..prism
        };
        assert!(describe(&vanilla).contains("vanilla 1.21.1"));
    }
}
