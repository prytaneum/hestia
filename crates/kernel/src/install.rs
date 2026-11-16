//! Installed packages on disk: `<data_home>/plugins/<id>/` holds `plugin.toml`,
//! `plugin.wasm` and the digest the user approved.

use std::io::Read;
use std::path::{Path, PathBuf};

use common::schema::{self, Document};
use proto::error::IoOp;
use serde::{Deserialize, Serialize};

use crate::error::{PackageError, PluginError};
use crate::manifest::Manifest;
use crate::runtime::Shape;

const MANIFEST: &str = "plugin.toml";
const COMPONENT: &str = "plugin.wasm";
const MANIFEST_MAX: u64 = 64 << 10;
const COMPONENT_MAX: u64 = 64 << 20;
const STAGING: &str = ".staging";

/// A package read from a directory; nothing of it has run or been compiled.
pub(crate) struct Package {
    pub manifest: Manifest,
    pub manifest_bytes: Vec<u8>,
    pub wasm: Vec<u8>,
    pub shape: Shape,
}

impl Package {
    pub fn read(dir: &Path) -> Result<Self, PackageError> {
        let manifest_bytes = read_bounded(&dir.join(MANIFEST), MANIFEST_MAX)?;
        let text = std::str::from_utf8(&manifest_bytes).map_err(|_| PackageError::NotUtf8)?;
        let manifest = Manifest::parse(text)?;
        let wasm = read_bounded(&dir.join(COMPONENT), COMPONENT_MAX)?;
        let shape = Shape::read(&wasm)?;
        Ok(Package {
            manifest,
            manifest_bytes,
            wasm,
            shape,
        })
    }
}

/// Refuses a link, so a package cannot point Hestia at another file of the
/// user's, and anything but a regular file, whose length says nothing about
/// what reading it returns.
fn read_bounded(path: &Path, max: u64) -> Result<Vec<u8>, PackageError> {
    let file = path.display().to_string();
    let unreadable = |error: std::io::Error| PackageError::Unreadable {
        file: file.clone(),
        detail: error.to_string(),
    };
    if !std::fs::symlink_metadata(path)
        .map_err(unreadable)?
        .file_type()
        .is_file()
    {
        return Err(PackageError::NotAFile(file));
    }
    let opened = std::fs::File::open(path).map_err(unreadable)?;
    if !opened.metadata().map_err(unreadable)?.is_file() {
        return Err(PackageError::NotAFile(file));
    }
    let mut bytes = Vec::new();
    opened
        .take(max + 1)
        .read_to_end(&mut bytes)
        .map_err(unreadable)?;
    if bytes.len() as u64 > max {
        return Err(PackageError::TooLarge { file, max });
    }
    Ok(bytes)
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct Approval {
    digest: String,
}

impl Document for Approval {
    const NAME: &'static str = "approval.json";
}

#[derive(Clone)]
pub(crate) struct Store {
    root: PathBuf,
}

impl Store {
    pub fn new(data_home: &Path) -> Self {
        Store {
            root: data_home.join("plugins"),
        }
    }

    pub fn dir(&self, id: &str) -> PathBuf {
        self.root.join(id)
    }

    /// The installed ids. Staging left by an install that never finished is
    /// deleted on the way.
    pub fn ids(&self) -> Vec<String> {
        let mut ids = Vec::new();
        for entry in std::fs::read_dir(&self.root)
            .into_iter()
            .flatten()
            .flatten()
        {
            let Ok(name) = entry.file_name().into_string() else {
                continue;
            };
            if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                continue;
            }
            if name.ends_with(STAGING) {
                if let Err(error) = std::fs::remove_dir_all(entry.path()) {
                    tracing::warn!(dir = name, %error, "cannot reclaim plugin staging");
                }
                continue;
            }
            if crate::id::is_valid(&name) {
                ids.push(name);
            }
        }
        ids.sort();
        ids
    }

    pub fn approved(&self, id: &str) -> Option<String> {
        schema::load::<Approval>(&self.dir(id).join(Approval::NAME)).map(|a| a.digest)
    }

    /// Staged beside the final directory and renamed into place, so a crash
    /// leaves no half-installed plugin. The caller serialises installs.
    pub fn save(&self, package: &Package, digest: &str) -> Result<(), PluginError> {
        let id = &package.manifest.id;
        let staging = self.root.join(format!("{id}{STAGING}"));
        let _ = std::fs::remove_dir_all(&staging);
        std::fs::create_dir_all(&staging).map_err(|e| PluginError::io(IoOp::Create, e))?;
        let write = |name: &str, bytes: &[u8]| {
            std::fs::write(staging.join(name), bytes).map_err(|e| PluginError::io(IoOp::Write, e))
        };
        write(MANIFEST, &package.manifest_bytes)?;
        write(COMPONENT, &package.wasm)?;
        schema::save(
            &staging.join(Approval::NAME),
            &Approval {
                digest: digest.to_owned(),
            },
        )
        .map_err(PluginError::Save)?;
        std::fs::rename(&staging, self.dir(id)).map_err(|e| PluginError::io(IoOp::Move, e))
    }

    pub fn remove(&self, id: &str) -> Result<(), PluginError> {
        match std::fs::remove_dir_all(self.dir(id)) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                Err(PluginError::io(IoOp::Remove, error))
            }
            _ => Ok(()),
        }
    }
}
