use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use tempfile::NamedTempFile;
use zip::write::SimpleFileOptions;

use super::cargo::{self, Metadata};
use super::check;
use super::repo::{PluginDir, MANIFEST, PLUGINS_DIR};
use super::sign::Signer;
use crate::ui::{self, View};

const DIST: &str = "dist";
const BUNDLED_DIRS: [&str; 2] = ["assets", "themes"];

/// Packs the named plugins of the repository around the current directory, or
/// all of them.
pub fn run(ids: &[String], sign: Option<&Path>) -> Result<()> {
    let repo = check::find(None)?;
    let checked = check::check(&repo)?;
    let signer = sign
        .map(|path| Signer::load(path, &checked.meta.key))
        .transpose()?;
    let plugins = select(checked.plugins, ids)?;

    let components: Vec<&PluginDir> = plugins
        .iter()
        .filter(|plugin| plugin.manifest.wasm.is_some())
        .collect();
    let metadata = if components.is_empty() {
        None
    } else {
        let metadata = Metadata::read(repo.root())?;
        let packages = components
            .iter()
            .map(|plugin| metadata.package_name(&plugin.path))
            .collect::<Result<Vec<_>>>()?;
        cargo::build(repo.root(), &packages)?;
        Some(metadata)
    };

    let dist = repo.root().join(DIST);
    std::fs::create_dir_all(&dist)?;
    for plugin in &plugins {
        let component = match (&plugin.manifest.wasm, &metadata) {
            (Some(_), Some(metadata)) => Some(metadata.component(&plugin.path)?),
            _ => None,
        };
        let package = pack_plugin(plugin, component.as_ref(), &dist)
            .with_context(|| format!("cannot pack '{}'", plugin.manifest.id))?;
        if let Some(signer) = &signer {
            sign_package(signer, &plugin.manifest, &package)?;
        }
        ui::show(View::line(format!(
            "packed {}",
            package
                .strip_prefix(repo.root())
                .unwrap_or(&package)
                .display()
        )))?;
    }
    Ok(())
}

fn sign_package(signer: &Signer, manifest: &proto::plugin::Manifest, package: &Path) -> Result<()> {
    let file = package
        .file_name()
        .and_then(|name| name.to_str())
        .context("the package has no file name")?;
    let bytes = std::fs::read(package)?;
    let comment = proto::repository::build_comment(&manifest.id, &manifest.version, file);
    signer.write(&bytes, &comment, package)?;
    Ok(())
}

fn select(all: Vec<PluginDir>, ids: &[String]) -> Result<Vec<PluginDir>> {
    if all.is_empty() {
        bail!("no plugins under {PLUGINS_DIR}/; add one with `hestia plugin new <id>`");
    }
    if ids.is_empty() {
        return Ok(all);
    }
    let known: Vec<String> = all.iter().map(|plugin| plugin.dir_name.clone()).collect();
    if let Some(unknown) = ids.iter().find(|id| !known.contains(id)) {
        bail!(
            "no plugin '{unknown}' under {PLUGINS_DIR}/ (found: {})",
            known.join(", ")
        );
    }
    Ok(all
        .into_iter()
        .filter(|plugin| ids.contains(&plugin.dir_name))
        .collect())
}

/// A package's files by package-relative name.
type Files = Vec<(String, Vec<u8>)>;

/// The package's manifest and files by package-relative name. The `api`
/// requirement comes from the SDK the component was built against; the author
/// never writes it.
pub(super) fn package_files(
    plugin: &PluginDir,
    component: Option<&cargo::Component>,
) -> Result<(proto::plugin::Manifest, Files)> {
    let mut manifest = plugin.manifest.clone();
    let mut manifest_text = std::fs::read_to_string(plugin.path.join(MANIFEST))?;
    if let Some(component) = component {
        if manifest.api.is_some() {
            bail!("{MANIFEST} must not set `api`; pack derives it from hestia-sdk");
        }
        let api = cargo::api_requirement(&component.sdk_version);
        manifest_text = format!("api = \"{api}\"\n{manifest_text}");
        manifest.api = Some(api);
    }
    manifest.validate()?;

    let mut files = vec![(MANIFEST.to_string(), manifest_text.into_bytes())];
    if let (Some(wasm_path), Some(component)) = (&manifest.wasm, component) {
        let wasm = std::fs::read(&component.wasm)
            .with_context(|| format!("cannot read {}", component.wasm.display()))?;
        files.push((wasm_path.clone(), wasm));
    }
    for bundled in BUNDLED_DIRS {
        collect(&plugin.path, bundled, &mut files)?;
    }
    check_themes(&manifest, &files)?;
    Ok((manifest, files))
}

/// Writes `dist/<id>-<version>.hplugin`.
fn pack_plugin(
    plugin: &PluginDir,
    component: Option<&cargo::Component>,
    dist: &Path,
) -> Result<PathBuf> {
    let (manifest, files) = package_files(plugin, component)?;
    let archive = dist.join(format!("{}-{}.hplugin", manifest.id, manifest.version));
    let mut temp = NamedTempFile::new_in(dist)?;
    write_zip(temp.as_file_mut(), &files)?;
    temp.persist(&archive)?;
    tracing::info!(plugin = %manifest.id, version = %manifest.version, "plugin packed");
    Ok(archive)
}

fn collect(plugin: &Path, relative: &str, files: &mut Vec<(String, Vec<u8>)>) -> Result<()> {
    let dir = plugin.join(relative);
    if !dir.is_dir() {
        return Ok(());
    }
    let mut entries = std::fs::read_dir(&dir)?.collect::<std::io::Result<Vec<_>>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let child = format!("{relative}/{}", entry.file_name().to_string_lossy());
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            collect(plugin, &child, files)?;
        } else if file_type.is_file() {
            files.push((child, std::fs::read(entry.path())?));
        }
    }
    Ok(())
}

fn check_themes(manifest: &proto::plugin::Manifest, files: &[(String, Vec<u8>)]) -> Result<()> {
    let packed: BTreeSet<&str> = files.iter().map(|(name, _)| name.as_str()).collect();
    match manifest
        .contributes
        .themes
        .iter()
        .find(|theme| !packed.contains(theme.file.as_str()))
    {
        Some(theme) => bail!(
            "theme '{}' points at {}, which is not under {}",
            theme.id,
            theme.file,
            BUNDLED_DIRS.join("/ or ")
        ),
        None => Ok(()),
    }
}

fn write_zip(out: &mut std::fs::File, files: &[(String, Vec<u8>)]) -> Result<()> {
    let mut zip = zip::ZipWriter::new(out);
    for (name, bytes) in files {
        zip.start_file(name, SimpleFileOptions::default())?;
        zip.write_all(bytes)?;
    }
    zip.finish()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use super::*;

    fn plugin_on_disk(root: &Path, manifest: &str) -> PluginDir {
        let path = root.join(PLUGINS_DIR).join("tunnel");
        std::fs::create_dir_all(path.join("assets")).unwrap();
        std::fs::create_dir_all(path.join("themes")).unwrap();
        std::fs::write(path.join(MANIFEST), manifest).unwrap();
        std::fs::write(path.join("assets/icon.txt"), "icon").unwrap();
        std::fs::write(path.join("themes/dark.css"), "body{}").unwrap();
        PluginDir {
            dir_name: "tunnel".to_string(),
            manifest: toml::from_str(manifest).unwrap(),
            path,
            has_crate: true,
        }
    }

    fn entry(archive: &Path, name: &str) -> Option<String> {
        let mut zip = zip::ZipArchive::new(std::fs::File::open(archive).unwrap()).unwrap();
        let mut text = String::new();
        zip.by_name(name).ok()?.read_to_string(&mut text).ok()?;
        Some(text)
    }

    #[test]
    fn a_packed_component_declares_the_api_of_its_sdk_and_bundles_its_folders() {
        let repo = tempfile::tempdir().unwrap();
        let plugin = plugin_on_disk(
            repo.path(),
            "id = \"tunnel\"\nname = \"Tunnel\"\nversion = \"0.2.0\"\nwasm = \"plugin.wasm\"\npermissions = [\"ui:css\"]\n\n[[contributes.themes]]\nid = \"dark\"\nname = \"Dark\"\nfile = \"themes/dark.css\"\n",
        );
        let wasm = repo.path().join("built.wasm");
        std::fs::write(&wasm, "component").unwrap();
        let component = cargo::Component {
            wasm,
            sdk_version: "1.0.0-beta.3".to_string(),
        };

        let archive = pack_plugin(&plugin, Some(&component), repo.path()).unwrap();

        assert_eq!(archive.file_name().unwrap(), "tunnel-0.2.0.hplugin");
        let manifest: proto::plugin::Manifest =
            toml::from_str(&entry(&archive, MANIFEST).unwrap()).unwrap();
        assert_eq!(manifest.api.as_deref(), Some("^1.0.0"));
        manifest.validate().unwrap();
        assert_eq!(entry(&archive, "plugin.wasm").unwrap(), "component");
        assert_eq!(entry(&archive, "assets/icon.txt").unwrap(), "icon");
        assert_eq!(entry(&archive, "themes/dark.css").unwrap(), "body{}");
    }

    #[test]
    fn a_theme_that_is_not_shipped_is_refused() {
        let repo = tempfile::tempdir().unwrap();
        let plugin = plugin_on_disk(
            repo.path(),
            "id = \"tunnel\"\nname = \"Tunnel\"\nversion = \"0.2.0\"\n\n[[contributes.themes]]\nid = \"dark\"\nfile = \"css/dark.css\"\n",
        );

        assert!(pack_plugin(&plugin, None, repo.path()).is_err());
    }
}
