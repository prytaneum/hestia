//! Installed packages on disk: `<data_home>/plugins/<id>/` holds `plugin.toml`,
//! `plugin.wasm` and the record of what the user approved and where it came from.

use std::collections::BTreeSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use common::schema::{self, Document};
use proto::error::IoOp;
use proto::plugin::{Choice, Grant, PluginSource};
use serde::{Deserialize, Serialize};

use crate::error::{PackageError, PluginError};
use crate::manifest::Manifest;
use crate::runtime::Shape;
use crate::trust::{same_key, Repository};

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

/// Where an installed package came from. A repository install keeps the key of
/// the repository that vouched for it, and the key of the author who signed
/// it, which every update must be signed by too.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Source {
    #[default]
    Dev,
    Repository {
        repository: String,
        repository_key: String,
        author: String,
    },
}

impl Source {
    /// The repository and author key an approval covers; none for a dev install.
    pub fn signer(&self) -> Option<(&str, &str)> {
        match self {
            Source::Dev => None,
            Source::Repository {
                repository, author, ..
            } => Some((repository, author)),
        }
    }

    /// Whether `repository`, as it is pinned now, is the one that vouched for
    /// this install: the same id under the same key.
    pub fn vouched_by(&self, repository: &Repository) -> bool {
        matches!(self, Source::Repository { repository: id, repository_key, .. }
            if *id == repository.id && same_key(repository_key, &repository.key))
    }

    pub fn public(&self) -> PluginSource {
        match self {
            Source::Dev => PluginSource::Dev,
            Source::Repository { .. } => PluginSource::Repository,
        }
    }
}

/// What the user approved: the digest, the grant it covered (what an update is
/// compared with), what they chose within it (optional permissions, entries),
/// where the package came from, and why it was revoked, if so.
#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct Approval {
    pub digest: String,
    pub grant: Grant,
    pub choice: Choice,
    pub source: Source,
    pub revoked: Option<String>,
}

impl Document for Approval {
    const NAME: &'static str = "approval.json";
}

/// `amending` serialises every rewrite of a record and the swap and removal of
/// a plugin's directory, so an amend never lands in a directory being replaced
/// and two amends never lose one another's field.
#[derive(Clone)]
pub(crate) struct Store {
    root: PathBuf,
    amending: Arc<Mutex<()>>,
}

/// `<data_home>/sandbox`, which holds one directory per plugin: its `/data`.
pub fn sandbox_root(data_home: &Path) -> PathBuf {
    data_home.join("sandbox")
}

impl Store {
    pub fn new(data_home: &Path) -> Self {
        Store {
            root: data_home.join("plugins"),
            amending: Arc::default(),
        }
    }

    pub fn dir(&self, id: &str) -> PathBuf {
        self.root.join(id)
    }

    /// The installed ids. Staging left by an install that never finished is
    /// deleted on the way, and an update that never finished is undone: its
    /// `.previous` returns, whether or not the new version was moved in.
    pub fn ids(&self) -> Vec<String> {
        let names: Vec<String> = std::fs::read_dir(&self.root)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
            .filter_map(|entry| entry.file_name().into_string().ok())
            .collect();
        let mut ids = BTreeSet::new();
        for name in names {
            if name.ends_with(STAGING) {
                if let Err(error) = std::fs::remove_dir_all(self.root.join(&name)) {
                    tracing::warn!(dir = name, %error, "cannot reclaim plugin staging");
                }
            } else if let Some(id) = name.strip_suffix(PREVIOUS) {
                if !crate::id::is_valid(id) {
                    tracing::warn!(dir = name, "ignoring a directory that is no plugin's");
                    continue;
                }
                tracing::warn!(
                    plugin = id,
                    "an update never finished; restoring the version it replaced"
                );
                match self.rollback(id) {
                    Ok(()) => {
                        ids.insert(id.to_owned());
                    }
                    Err(error) => {
                        tracing::warn!(plugin = id, %error, "cannot restore the previous version");
                    }
                }
            } else if crate::id::is_valid(&name) {
                ids.insert(name);
            }
        }
        ids.into_iter().collect()
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
    /// until [`Store::commit`] or [`Store::rollback`]. `record` is built once
    /// nothing else can amend, so it carries the latest of what an amend wrote.
    pub fn swap(
        &self,
        package: &Package,
        record: impl FnOnce() -> Approval,
    ) -> Result<(), PluginError> {
        let _held = self.amending.lock().unwrap_or_else(|e| e.into_inner());
        let record = record();
        let id = &package.manifest.id;
        let (current, previous) = (self.dir(id), self.root.join(format!("{id}{PREVIOUS}")));
        let staging = self.stage(package, &record)?;
        let _ = std::fs::remove_dir_all(&previous);
        std::fs::rename(&current, &previous).map_err(|e| PluginError::io(IoOp::Move, e))?;
        std::fs::rename(&staging, &current).map_err(|error| {
            let _ = std::fs::rename(&previous, &current);
            PluginError::io(IoOp::Move, error)
        })
    }

    /// Deletes what a [`Store::swap`] set aside. Its name changes first, in one
    /// rename, to one a start-up scan deletes rather than restores: a crash
    /// halfway through the delete must not bring back a partial old version.
    pub fn commit(&self, id: &str) {
        let previous = self.root.join(format!("{id}{PREVIOUS}"));
        if !previous.exists() {
            return;
        }
        let doomed = self.root.join(format!("{id}{STAGING}"));
        let _ = std::fs::remove_dir_all(&doomed);
        if let Err(error) = std::fs::rename(&previous, &doomed) {
            tracing::error!(plugin = id, %error, "cannot discard the previous version");
            return;
        }
        if let Err(error) = std::fs::remove_dir_all(&doomed) {
            tracing::warn!(plugin = id, %error, "cannot delete the previous version");
        }
    }

    /// Brings back the version a [`Store::swap`] set aside.
    pub fn rollback(&self, id: &str) -> Result<(), PluginError> {
        let _held = self.amending.lock().unwrap_or_else(|e| e.into_inner());
        let previous = self.root.join(format!("{id}{PREVIOUS}"));
        let _ = std::fs::remove_dir_all(self.dir(id));
        std::fs::rename(&previous, self.dir(id)).map_err(|e| PluginError::io(IoOp::Move, e))
    }

    /// Rewrites the record in place; the digest and the package are untouched.
    /// A plugin that is not installed has no record to rewrite.
    pub fn amend(&self, id: &str, change: impl FnOnce(&mut Approval)) -> Result<(), PluginError> {
        let _held = self.amending.lock().unwrap_or_else(|e| e.into_inner());
        let mut record = self
            .record(id)
            .ok_or_else(|| PluginError::NotFound(id.to_owned()))?;
        change(&mut record);
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
        self.carry_state(id, &staging)?;
        Ok(staging)
    }

    /// Copies what else the installed version's directory holds (the plugin's
    /// settings, secrets and schedule runs, which the store does not name) into
    /// `staging`, permissions kept, so an update neither loses it nor leaves a
    /// rollback without it.
    fn carry_state(&self, id: &str, staging: &Path) -> Result<(), PluginError> {
        let Ok(entries) = std::fs::read_dir(self.dir(id)) else {
            return Ok(());
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            if [MANIFEST, COMPONENT, Approval::NAME]
                .iter()
                .any(|own| name == *own)
                || !entry.file_type().is_ok_and(|kind| kind.is_file())
            {
                continue;
            }
            std::fs::copy(entry.path(), staging.join(&name))
                .map_err(|e| PluginError::io(IoOp::Write, e))?;
        }
        Ok(())
    }

    /// Deletes the plugin and any version an update set aside, which would
    /// otherwise return at the next start.
    pub fn remove(&self, id: &str) -> Result<(), PluginError> {
        let _held = self.amending.lock().unwrap_or_else(|e| e.into_inner());
        self.commit(id);
        match std::fs::remove_dir_all(self.dir(id)) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                Err(PluginError::io(IoOp::Remove, error))
            }
            _ => Ok(()),
        }
    }
}
