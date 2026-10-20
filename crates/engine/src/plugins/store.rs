use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

use anyhow::{bail, Context, Result};
use proto::plugin::{is_plugin_id, Grant, Manifest, PluginSource};
use serde::{Deserialize, Serialize};

use crate::reclaim::{self, Reclaimed};
use crate::registry;
use crate::schema::Document;

pub(crate) const PACKAGE: &str = "package";
pub(crate) const DATA: &str = "data";
const MANIFEST: &str = "plugin.toml";
const STAGING_SUFFIX: &str = ".staging";

/// An installed package read from disk: its validated manifest and, unless it
/// is theme-only, the component bytes.
pub struct PluginPackage {
    pub manifest: Manifest,
    pub wasm: Option<Vec<u8>>,
}

/// What the daemon remembers about an installed plugin. Holds no secrets: a
/// plugin's secret settings live with the credential store, not here.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[serde(default, rename_all = "camelCase")]
pub struct PluginRecord {
    pub id: String,
    pub source: PluginSource,
    pub wasm_sha256: Option<String>,
    /// Package-relative theme file to its hex sha256.
    pub theme_sha256: BTreeMap<String, String>,
    /// What the user approved.
    pub grant: Grant,
    pub grant_digest: String,
    pub enabled: bool,
    pub installed_unix: i64,
}

impl Document for PluginRecord {
    const NAME: &'static str = "plugin.json";
}

pub struct Plugins {
    dir: PathBuf,
    writes: Mutex<()>,
}

impl Plugins {
    pub fn new(dir: PathBuf) -> Self {
        Plugins {
            dir,
            writes: Mutex::new(()),
        }
    }

    pub(crate) fn reclaim(&self) -> Reclaimed {
        reclaim::suffixed(&self.dir, STAGING_SUFFIX)
    }

    /// The plugin's directory; the id is checked because it becomes a path.
    pub fn plugin_dir(&self, id: &str) -> Result<PathBuf> {
        if !is_plugin_id(id) {
            bail!("`{id}` is not a valid plugin id");
        }
        Ok(self.dir.join(id))
    }

    pub fn package_dir(&self, id: &str) -> Result<PathBuf> {
        Ok(self.plugin_dir(id)?.join(PACKAGE))
    }

    pub fn data_dir(&self, id: &str) -> Result<PathBuf> {
        Ok(self.plugin_dir(id)?.join(DATA))
    }

    /// Blocking: reads and validates the installed manifest.
    pub fn read_manifest(&self, id: &str) -> Result<Manifest> {
        let path = self.package_dir(id)?.join(MANIFEST);
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("cannot read {}", path.display()))?;
        let manifest: Manifest =
            toml::from_str(&text).with_context(|| format!("invalid {}", path.display()))?;
        manifest.validate()?;
        if manifest.id != id {
            bail!("{} declares plugin `{}`", path.display(), manifest.id);
        }
        Ok(manifest)
    }

    /// Blocking: reads the manifest and the component it names.
    pub fn read_package(&self, id: &str) -> Result<PluginPackage> {
        let package = self.package_dir(id)?;
        let manifest = self.read_manifest(id)?;
        let wasm = manifest
            .wasm
            .as_ref()
            .map(|relative| {
                let component = package.join(relative);
                std::fs::read(&component)
                    .with_context(|| format!("cannot read {}", component.display()))
            })
            .transpose()?;
        Ok(PluginPackage { manifest, wasm })
    }

    pub fn list(&self) -> Vec<PluginRecord> {
        let mut records: Vec<PluginRecord> = registry::scan(&self.dir);
        records.sort_by(|a, b| a.id.cmp(&b.id));
        records
    }

    pub fn get(&self, id: &str) -> Option<PluginRecord> {
        registry::read_record(&self.plugin_dir(id).ok()?)
    }

    /// Persist a record, creating the plugin's `data/`. `package/` is left to
    /// the install's rename.
    pub fn save(&self, record: &PluginRecord) -> Result<()> {
        let dir = self.plugin_dir(&record.id)?;
        let _writes = self.writes.lock().unwrap();
        let data = dir.join(DATA);
        std::fs::create_dir_all(&data)
            .with_context(|| format!("cannot create {}", data.display()))?;
        registry::write_record(&dir, record)
    }

    /// False when no such plugin is installed.
    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<bool> {
        let dir = self.plugin_dir(id)?;
        let _writes = self.writes.lock().unwrap();
        let Some(mut record) = registry::read_record::<PluginRecord>(&dir) else {
            return Ok(false);
        };
        record.enabled = enabled;
        registry::write_record(&dir, &record)?;
        tracing::info!(plugin = %id, enabled, "plugin toggled");
        Ok(true)
    }

    /// Delete the plugin's directory through a rename, so a crash mid-delete
    /// never leaves a half-removed plugin that still scans as installed.
    pub fn remove(&self, id: &str) -> Result<bool> {
        let dir = self.plugin_dir(id)?;
        let _writes = self.writes.lock().unwrap();
        if !dir.is_dir() {
            return Ok(false);
        }
        let staging = reclaim::with_suffix(&dir, STAGING_SUFFIX);
        if staging.exists() {
            std::fs::remove_dir_all(&staging)
                .with_context(|| format!("cannot clear {}", staging.display()))?;
        }
        std::fs::rename(&dir, &staging)
            .with_context(|| format!("cannot remove {}", dir.display()))?;
        let record = registry::record_path::<PluginRecord>(&staging);
        std::fs::remove_file(&record)
            .with_context(|| format!("cannot remove {}", record.display()))?;
        if let Err(error) = std::fs::remove_dir_all(&staging) {
            tracing::warn!(plugin = %id, %error, "plugin files left for startup reclaim");
        }
        tracing::info!(plugin = %id, "plugin removed");
        Ok(true)
    }
}
