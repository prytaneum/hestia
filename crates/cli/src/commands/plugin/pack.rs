use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use tempfile::NamedTempFile;
use zip::write::SimpleFileOptions;

use super::cargo::{self, Metadata};
use super::check;
use super::repo::{PluginDir, MANIFEST, PLUGINS_DIR};
use super::sdk::{Sdk, SdkOption};
use super::sign::Signer;
use super::world;
use crate::ui::{self, View};

const DIST: &str = "dist";
const BUNDLED_DIRS: [&str; 2] = ["assets", "themes"];

/// Packs the named plugins of the repository around the current directory, or
/// all of them. Invalid plugins are skipped and reported at the end.
pub fn run(ids: &[String], sign: Option<&Path>, sdk: &SdkOption) -> Result<()> {
    let sdk = Sdk::resolve(sdk)?;
    let repo = check::find(None)?;
    let inspection = check::inspect(&repo)?;
    inspection.require_shape()?;
    let inspection = inspection.select(ids)?;
    if inspection.plugins.is_empty() {
        bail!("no plugins under {PLUGINS_DIR}/; add one with `hestia plugin new <id>`");
    }
    let checked = inspection.checked();
    let signer = sign
        .map(|path| Signer::load(path, &checked.meta.key))
        .transpose()?;

    let dist = repo.root().join(DIST);
    std::fs::create_dir_all(&dist)?;
    let packages = build(repo.root(), &checked.plugins, &dist, sdk.as_ref())?;
    for (plugin, package) in checked.plugins.iter().zip(packages) {
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
    check::fail_invalid(&inspection.invalid(), "packed")
}

/// Builds the plugins' components in one cargo run and writes each package into
/// `out`, returning the package paths in the order given.
pub(super) fn build(
    root: &Path,
    plugins: &[PluginDir],
    out: &Path,
    sdk: Option<&Sdk>,
) -> Result<Vec<PathBuf>> {
    let components: Vec<&PluginDir> = plugins
        .iter()
        .filter(|plugin| plugin.manifest.wasm.is_some())
        .collect();
    let metadata = if components.is_empty() {
        None
    } else {
        let metadata = Metadata::read(root, sdk)?;
        let packages = components
            .iter()
            .map(|plugin| metadata.package_name(&plugin.path))
            .collect::<Result<Vec<_>>>()?;
        cargo::build(root, &packages, sdk)?;
        Some(metadata)
    };

    plugins
        .iter()
        .map(|plugin| {
            let component = match (&plugin.manifest.wasm, &metadata) {
                (Some(_), Some(metadata)) => Some(metadata.component(&plugin.path)?),
                _ => None,
            };
            pack_plugin(plugin, component.as_ref(), out)
                .with_context(|| format!("cannot pack '{}'", plugin.manifest.id))
        })
        .collect()
}

/// Writes `<package>.minisig`, bound to the manifest's id and version.
pub(super) fn sign_package(
    signer: &Signer,
    manifest: &proto::plugin::Manifest,
    package: &Path,
) -> Result<()> {
    let file = package
        .file_name()
        .and_then(|name| name.to_str())
        .context("the package has no file name")?;
    let bytes = std::fs::read(package)?;
    let comment = proto::repository::build_comment(&manifest.id, &manifest.version, file);
    signer.write(&bytes, &comment, package)?;
    Ok(())
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
        world::check_component(&manifest.id, &wasm)?;
        files.push((wasm_path.clone(), wasm));
    }
    for bundled in BUNDLED_DIRS {
        collect(&plugin.path, bundled, &mut files)?;
    }
    check_themes(&manifest, &files)?;
    if let Some(defaults) = distributed(&manifest, |var| std::env::var(var).ok())? {
        files.push((proto::plugin::SETTING_DEFAULTS.to_string(), defaults));
    }
    Ok((manifest, files))
}

/// The distributor's values for the plugin's settings, read from the
/// environment so none is ever committed; `None` when no variable is set. A
/// value its setting would refuse is an error that names the variable only.
fn distributed(
    manifest: &proto::plugin::Manifest,
    lookup: impl Fn(&str) -> Option<String>,
) -> Result<Option<Vec<u8>>> {
    let mut values = serde_json::Map::new();
    for setting in &manifest.settings {
        let var = proto::plugin::setting_default_var(&manifest.id, &setting.key);
        let Some(text) = lookup(&var).filter(|text| !text.is_empty()) else {
            continue;
        };
        let value = setting
            .parse(&text)
            .with_context(|| format!("${var} is not a value of setting `{}`", setting.key))?;
        if let Err(reason) = setting.check(&value) {
            bail!(
                "${var} is not usable for setting `{}`: {reason}",
                setting.key
            );
        }
        tracing::info!(plugin = %manifest.id, setting = %setting.key, "packing a distributed setting default");
        values.insert(setting.key.clone(), value);
    }
    match values.is_empty() {
        true => Ok(None),
        false => Ok(Some(serde_json::to_vec(&values)?)),
    }
}

/// Writes `<dist>/<id>-<version>.hplugin`.
pub(super) fn pack_plugin(
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

    fn entry_exists(archive: &Path, name: &str) -> bool {
        let mut zip = zip::ZipArchive::new(std::fs::File::open(archive).unwrap()).unwrap();
        let found = zip.by_name(name).is_ok();
        found
    }

    #[test]
    fn a_component_built_against_another_wit_is_not_packed() {
        let repo = tempfile::tempdir().unwrap();
        let plugin = plugin_on_disk(
            repo.path(),
            "id = \"tunnel\"\nname = \"Tunnel\"\nversion = \"0.2.0\"\nwasm = \"plugin.wasm\"\n",
        );
        let wasm = repo.path().join("built.wasm");
        std::fs::write(&wasm, stale_component()).unwrap();
        let component = cargo::Component {
            wasm,
            sdk_version: "1.0.0-beta.3".to_string(),
        };

        let error = pack_plugin(&plugin, Some(&component), repo.path()).unwrap_err();

        assert!(format!("{error:#}").contains("cargo update -p hestia-sdk"));
    }

    fn stale_component() -> Vec<u8> {
        use wasm_encoder::{
            Component, ComponentImportSection, ComponentTypeRef, ComponentTypeSection, InstanceType,
        };
        let mut types = ComponentTypeSection::new();
        types.instance(&InstanceType::new());
        let mut imports = ComponentImportSection::new();
        imports.import("hestia:plugin/gone@1.0.0", ComponentTypeRef::Instance(0));
        let mut component = Component::new();
        component.section(&types).section(&imports);
        component.finish()
    }

    #[test]
    fn a_packed_component_declares_the_api_of_its_sdk_and_bundles_its_folders() {
        let repo = tempfile::tempdir().unwrap();
        let plugin = plugin_on_disk(
            repo.path(),
            "id = \"tunnel\"\nname = \"Tunnel\"\nversion = \"0.2.0\"\nwasm = \"plugin.wasm\"\n\n[permissions]\n\"ui:css\" = {}\n\n[[contributes.themes]]\nid = \"dark\"\nname = \"Dark\"\nfile = \"themes/dark.css\"\n",
        );
        let wasm = repo.path().join("built.wasm");
        std::fs::write(&wasm, wasm_encoder::Component::new().finish()).unwrap();
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
        assert!(entry_exists(&archive, "plugin.wasm"));
        assert_eq!(entry(&archive, "assets/icon.txt").unwrap(), "icon");
        assert_eq!(entry(&archive, "themes/dark.css").unwrap(), "body{}");
    }

    #[test]
    fn a_distributors_value_is_packed_from_its_variable_and_a_bad_one_is_refused() {
        let manifest: proto::plugin::Manifest = toml::from_str(
            "id = \"cf-source\"\nname = \"CF\"\nversion = \"0.1.0\"\n\n[[settings]]\nkey = \"api-key\"\nkind = \"secret\"\n\n[[settings]]\nkey = \"limit\"\nkind = \"number\"\n",
        )
        .unwrap();
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |var: &str| {
                pairs
                    .iter()
                    .find(|(name, _)| *name == var)
                    .map(|(_, value)| value.to_string())
            }
        };

        assert!(distributed(&manifest, env(&[])).unwrap().is_none());
        let packed = distributed(
            &manifest,
            env(&[
                ("HESTIA_SETTING_CF_SOURCE_API_KEY", "k"),
                ("HESTIA_SETTING_CF_SOURCE_LIMIT", "5"),
            ]),
        )
        .unwrap()
        .unwrap();
        assert_eq!(packed, br#"{"api-key":"k","limit":5}"#);

        let refused = distributed(
            &manifest,
            env(&[("HESTIA_SETTING_CF_SOURCE_LIMIT", "s3cret")]),
        )
        .unwrap_err()
        .to_string();
        assert!(refused.contains("HESTIA_SETTING_CF_SOURCE_LIMIT") && !refused.contains("s3cret"));
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
