//! The shape of a plugin repository: a cargo workspace whose `plugins/<id>/`
//! folders each hold a `plugin.toml`. Everything that reads or writes a
//! repository goes through here.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use proto::plugin::Manifest;

pub const PLUGINS_DIR: &str = "plugins";
pub const MANIFEST: &str = "plugin.toml";
pub const CRATE_MANIFEST: &str = "Cargo.toml";

pub struct Repository {
    root: PathBuf,
}

/// One folder under `plugins/`, as found on disk. Whether it is valid is for
/// the caller to judge.
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
