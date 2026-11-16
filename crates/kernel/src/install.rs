//! Installed packages on disk: `<data_home>/plugins/<id>/` holds `plugin.toml`,
//! `plugin.wasm` and the record of what the user approved and where it came from.

use std::io::Read;
use std::path::{Path, PathBuf};

use common::schema::{self, Document};
use proto::error::IoOp;
use proto::plugin::{Grant, PluginSource};
use serde::{Deserialize, Serialize};

use crate::error::{PackageError, PluginError};
use crate::manifest::Manifest;
use crate::runtime::Shape;

const MANIFEST: &str = "plugin.toml";
const COMPONENT: &str = "plugin.wasm";
pub(crate) const MANIFEST_MAX: u64 = 64 << 10;
pub(crate) const COMPONENT_MAX: u64 = 64 << 20;
const STAGING: &str = ".staging";
const PREVIOUS: &str = ".previous";

/// A package read from a directory; nothing of it has run or been compiled.
pub(crate) struct Package {
    pub manifest: Manifest,
    pub manifest_bytes: Vec<u8>,
    pub wasm: Vec<u8>,
    pub shape: Shape,
}

impl Package {
    pub fn read(dir: &Path) -> Result<Self, PackageError> {
        Package::new(
            read_bounded(&dir.join(MANIFEST), MANIFEST_MAX)?,
            read_bounded(&dir.join(COMPONENT), COMPONENT_MAX)?,
        )
    }

    pub fn new(manifest_bytes: Vec<u8>, wasm: Vec<u8>) -> Result<Self, PackageError> {
        let text = std::str::from_utf8(&manifest_bytes).map_err(|_| PackageError::NotUtf8)?;
        let manifest = Manifest::parse(text)?;
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

/// Where an installed package came from. A repository install is held to the
/// key that signed it for every update.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Source {
    #[default]
    Dev,
    Repository {
        repository: String,
        signer: String,
    },
}

impl Source {
    pub fn public(&self) -> PluginSource {
        match self {
            Source::Dev => PluginSource::Dev,
            Source::Repository { .. } => PluginSource::Repository,
        }
    }
}

/// What the user approved: the digest, the grant it covered (what an update is
/// compared with), where the package came from, and why it was revoked, if so.
#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct Approval {
    pub digest: String,
    pub grant: Grant,
    pub source: Source,
    pub revoked: Option<String>,
}

impl Document for Approval {
    const NAME: &'static str = "approval.json";
}

#[derive(Clone)]
pub(crate) struct Store {
    root: PathBuf,
}

/// `<data_home>/sandbox`, which holds one directory per plugin: its `/data`.
pub fn sandbox_root(data_home: &Path) -> PathBuf {
    data_home.join("sandbox")
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
            if let Some(id) = name.strip_suffix(PREVIOUS) {
                tracing::warn!(
                    plugin = id,
                    "an update never finished; restoring the version it replaced"
                );
                if let Err(error) = self.rollback(id) {
                    tracing::warn!(plugin = id, %error, "cannot restore the previous version");
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

    pub fn record(&self, id: &str) -> Option<Approval> {
        schema::load::<Approval>(&self.dir(id).join(Approval::NAME))
    }

    /// Staged beside the final directory and renamed into place, so a crash
    /// leaves no half-installed plugin. The caller serialises installs.
    pub fn save(&self, package: &Package, record: &Approval) -> Result<(), PluginError> {
        let staging = self.stage(package, record)?;
        std::fs::rename(&staging, self.dir(&package.manifest.id))
            .map_err(|e| PluginError::io(IoOp::Move, e))
    }

    /// Puts `package` in place of the installed version, which waits beside it
    /// until [`Store::commit`] or [`Store::rollback`].
    pub fn swap(&self, package: &Package, record: &Approval) -> Result<(), PluginError> {
        let id = &package.manifest.id;
        let (current, previous) = (self.dir(id), self.root.join(format!("{id}{PREVIOUS}")));
        let staging = self.stage(package, record)?;
        let _ = std::fs::remove_dir_all(&previous);
        std::fs::rename(&current, &previous).map_err(|e| PluginError::io(IoOp::Move, e))?;
        std::fs::rename(&staging, &current).map_err(|error| {
            let _ = std::fs::rename(&previous, &current);
            PluginError::io(IoOp::Move, error)
        })
    }

    pub fn commit(&self, id: &str) {
        let _ = std::fs::remove_dir_all(self.root.join(format!("{id}{PREVIOUS}")));
    }

    /// Brings back the version a [`Store::swap`] set aside.
    pub fn rollback(&self, id: &str) -> Result<(), PluginError> {
        let previous = self.root.join(format!("{id}{PREVIOUS}"));
        let _ = std::fs::remove_dir_all(self.dir(id));
        std::fs::rename(&previous, self.dir(id)).map_err(|e| PluginError::io(IoOp::Move, e))
    }

    /// Marks the installed version revoked, which keeps it from ever starting.
    pub fn revoke(&self, id: &str, reason: &str) -> Result<(), PluginError> {
        let mut record = self.record(id).unwrap_or_default();
        record.revoked = Some(reason.to_owned());
        schema::save(&self.dir(id).join(Approval::NAME), &record).map_err(PluginError::Save)
    }

    fn stage(&self, package: &Package, record: &Approval) -> Result<PathBuf, PluginError> {
        let id = &package.manifest.id;
        let staging = self.root.join(format!("{id}{STAGING}"));
        let _ = std::fs::remove_dir_all(&staging);
        std::fs::create_dir_all(&staging).map_err(|e| PluginError::io(IoOp::Create, e))?;
        let write = |name: &str, bytes: &[u8]| {
            std::fs::write(staging.join(name), bytes).map_err(|e| PluginError::io(IoOp::Write, e))
        };
        write(MANIFEST, &package.manifest_bytes)?;
        write(COMPONENT, &package.wasm)?;
        schema::save(&staging.join(Approval::NAME), record).map_err(PluginError::Save)?;
        Ok(staging)
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
