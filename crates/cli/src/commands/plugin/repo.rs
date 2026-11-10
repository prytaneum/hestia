//! The shape of a plugin repository: a cargo workspace whose `plugins/<id>/`
//! folders each hold a `plugin.toml`. Everything that reads or writes a
//! repository goes through here.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use proto::plugin::Manifest;
use serde::Deserialize;

pub const PLUGINS_DIR: &str = "plugins";
pub const MANIFEST: &str = "plugin.toml";
pub const CRATE_MANIFEST: &str = "Cargo.toml";

pub struct Repository {
    root: PathBuf,
}

/// What `[workspace.metadata.hestia]` declares: the store the repository is.
#[derive(Deserialize, Default, Debug, Clone)]
#[serde(default)]
pub struct StoreMeta {
    pub name: String,
    pub label: String,
    pub website: String,
    pub key: String,
    /// The optional spare key, pinned with `key` by whoever adds the repository.
    pub next: String,
}

/// One folder under `plugins/`, as found on disk. Whether it is valid is for
/// the caller to judge.
#[derive(Clone)]
pub struct PluginDir {
    pub dir_name: String,
    pub path: PathBuf,
    pub manifest: Manifest,
    pub has_crate: bool,
}

impl Repository {
    /// The nearest ancestor of `dir` (itself included) holding a `Cargo.toml`
    /// beside a `plugins/` folder.
    pub fn containing(dir: &Path) -> Option<Repository> {
        dir.ancestors()
            .find(|candidate| {
                candidate.join(CRATE_MANIFEST).is_file() && candidate.join(PLUGINS_DIR).is_dir()
            })
            .map(|root| Repository {
                root: root.to_path_buf(),
            })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The workspace `Cargo.toml`.
    pub fn workspace(&self) -> Result<toml::Table> {
        let path = self.root.join(CRATE_MANIFEST);
        std::fs::read_to_string(&path)
            .with_context(|| format!("cannot read {}", path.display()))?
            .parse()
            .with_context(|| format!("invalid {CRATE_MANIFEST}"))
    }

    pub fn plugin_path(&self, id: &str) -> PathBuf {
        self.root.join(PLUGINS_DIR).join(id)
    }

    /// Every folder under `plugins/`, sorted by name.
    pub fn plugins(&self) -> Result<Vec<PluginDir>> {
        let plugins_dir = self.root.join(PLUGINS_DIR);
        let mut names = Vec::new();
        for entry in std::fs::read_dir(&plugins_dir)
            .with_context(|| format!("cannot read {}", plugins_dir.display()))?
        {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                names.push(entry.file_name().to_string_lossy().into_owned());
            }
        }
        names.sort();
        names
            .into_iter()
            .map(|dir_name| read_plugin(&plugins_dir, dir_name))
            .collect()
    }
}

fn read_plugin(plugins_dir: &Path, dir_name: String) -> Result<PluginDir> {
    let path = plugins_dir.join(&dir_name);
    let manifest_path = path.join(MANIFEST);
    if !manifest_path.is_file() {
        bail!("{PLUGINS_DIR}/{dir_name} has no {MANIFEST}");
    }
    let text = std::fs::read_to_string(&manifest_path)
        .with_context(|| format!("cannot read {PLUGINS_DIR}/{dir_name}/{MANIFEST}"))?;
    let manifest = toml::from_str(&text)
        .with_context(|| format!("invalid {PLUGINS_DIR}/{dir_name}/{MANIFEST}"))?;
    Ok(PluginDir {
        has_crate: path.join(CRATE_MANIFEST).is_file(),
        dir_name,
        path,
        manifest,
    })
}

/// `[workspace.metadata.hestia]` of a parsed workspace manifest, if present.
pub fn store_meta(workspace: &toml::Table) -> Option<Result<StoreMeta>> {
    let table = workspace.get("workspace")?.get("metadata")?.get("hestia")?;
    Some(
        table
            .clone()
            .try_into()
            .context("invalid [workspace.metadata.hestia]"),
    )
}

/// The workspace's `members` or `exclude` list.
pub fn workspace_list(workspace: &toml::Table, key: &str) -> Vec<String> {
    workspace
        .get("workspace")
        .and_then(|table| table.get(key))
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|value| value.as_str().map(str::to_string))
        .collect()
}
