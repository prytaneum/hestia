//! Reading a plugin package and installing it. Nothing here runs plugin code:
//! the component is only handed to the caller's `validate`.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use proto::error::ErrorInfo;
use proto::plugin::{Grant, Manifest, PluginInstallPhase, PluginSource};
use sha2::{Digest, Sha256};

use super::store::{PluginRecord, Plugins, MANIFEST, PACKAGE, STAGING_SUFFIX};
use crate::cancel::Cancel;
use crate::checksum::{digest_file, hex};
use crate::extract::extract_zip_capped;
use crate::files::copy_tree;
use crate::reclaim;
use crate::usage::dir_size;

/// Uncompressed size of a package. A plugin is a component and a few assets;
/// this bounds what a hostile archive can write.
const PACKAGE_LIMIT: u64 = 64 * 1024 * 1024;
const MANIFEST_LIMIT: u64 = 1024 * 1024;
const RETIRED: &str = "previous";

pub struct Inspection {
    pub manifest: Manifest,
    pub grant: Grant,
    pub grant_digest: String,
    pub installed: Option<PluginRecord>,
}

enum Source {
    Archive(PathBuf),
    Folder(PathBuf),
}

impl Source {
    fn at(path: &Path) -> Result<Source> {
        if path.is_dir() {
            Ok(Source::Folder(path.to_path_buf()))
        } else if path.is_file() {
            Ok(Source::Archive(path.to_path_buf()))
        } else {
            Err(invalid(format!("{} does not exist", path.display())))
        }
    }

    fn kind(&self) -> PluginSource {
        match self {
            Source::Archive(_) => PluginSource::File,
            Source::Folder(_) => PluginSource::Folder,
        }
    }

    fn read_manifest(&self) -> Result<Manifest> {
        let text = match self {
            Source::Folder(dir) => read_limited(File::open(dir.join(MANIFEST))?)?,
            Source::Archive(file) => {
                let mut zip = zip::ZipArchive::new(File::open(file)?).context("not a zip")?;
                let entry = zip.by_name(MANIFEST).context("no plugin.toml")?;
                read_limited(entry)?
            }
        };
        parse_manifest(&text)
    }

    fn stage(&self, package: &Path) -> Result<()> {
        match self {
            Source::Archive(file) => extract_zip_capped(file, package, PACKAGE_LIMIT),
            Source::Folder(dir) => {
                if dir_size(dir) > PACKAGE_LIMIT {
                    bail!("the folder is larger than {PACKAGE_LIMIT} bytes");
                }
                copy_tree(dir, package).context("cannot copy the folder")
            }
        }
    }
}

struct Staging(PathBuf);

impl Drop for Staging {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.0) {
            if self.0.exists() {
                tracing::warn!(path = %self.0.display(), %error, "plugin staging left for startup reclaim");
            }
        }
    }
}

fn invalid(detail: impl std::fmt::Display) -> anyhow::Error {
    ErrorInfo::PluginInvalid {
        detail: detail.to_string(),
    }
    .into()
}

fn read_limited(reader: impl Read) -> Result<String> {
    let mut text = String::new();
    reader.take(MANIFEST_LIMIT + 1).read_to_string(&mut text)?;
    if text.len() as u64 > MANIFEST_LIMIT {
        bail!("{MANIFEST} is larger than {MANIFEST_LIMIT} bytes");
    }
    Ok(text)
}

fn parse_manifest(text: &str) -> Result<Manifest> {
    let manifest: Manifest = toml::from_str(text).context("invalid plugin.toml")?;
    manifest.validate()?;
    Ok(manifest)
}

impl Plugins {
    /// Blocking.
    pub fn inspect(&self, path: &Path) -> Result<Inspection> {
        let manifest = Source::at(path)?
            .read_manifest()
            .map_err(|error| invalid(format!("{error:#}")))?;
        let grant = manifest.grant();
        Ok(Inspection {
            grant_digest: grant.digest(),
            installed: self.get(&manifest.id),
            manifest,
            grant,
        })
    }

    /// Blocking. Nothing outside the staging directory is touched until every
    /// check has passed.
    pub fn install(
        &self,
        path: &Path,
        approve: &str,
        cancel: &Cancel,
        on_phase: &dyn Fn(PluginInstallPhase),
        validate: impl FnOnce(&[u8]) -> Result<()>,
    ) -> Result<PluginRecord> {
        let source = Source::at(path)?;
        let id = source
            .read_manifest()
            .map_err(|error| invalid(format!("{error:#}")))?
            .id;
        let dir = self.plugin_dir(&id)?;
        let staging = reclaim::with_suffix(&dir, STAGING_SUFFIX);
        if staging.exists() {
            std::fs::remove_dir_all(&staging)
                .with_context(|| format!("cannot clear {}", staging.display()))?;
        }
        let _staging = Staging(staging.clone());
        let staged = staging.join(PACKAGE);

        on_phase(PluginInstallPhase::Extracting);
        source
            .stage(&staged)
            .map_err(|error| invalid(format!("{error:#}")))?;
        cancel.check()?;

        on_phase(PluginInstallPhase::Verifying);
        let manifest = std::fs::read_to_string(staged.join(MANIFEST))
            .map_err(anyhow::Error::from)
            .and_then(|text| parse_manifest(&text))
            .map_err(|error| invalid(format!("{error:#}")))?;
        if manifest.id != id {
            return Err(invalid("plugin.toml changed while it was read"));
        }
        let grant = manifest.grant();
        let grant_digest = grant.digest();
        if grant_digest != approve {
            tracing::warn!(plugin = %id, "install refused: the grant is not the one approved");
            bail!(ErrorInfo::PluginApprovalStale);
        }
        let wasm_sha256 = manifest
            .wasm
            .as_ref()
            .map(|relative| {
                let wasm = std::fs::read(staged.join(relative))
                    .map_err(|error| invalid(format!("cannot read {relative}: {error}")))?;
                validate(&wasm).map_err(|error| invalid(format!("{error:#}")))?;
                Ok::<_, anyhow::Error>(hex(&Sha256::digest(&wasm)))
            })
            .transpose()?;
        let mut theme_sha256 = BTreeMap::new();
        for theme in &manifest.contributes.themes {
            let sha256 = digest_file::<Sha256>(&staged.join(&theme.file))
                .map_err(|error| invalid(format!("cannot read {}: {error}", theme.file)))?;
            theme_sha256.insert(theme.file.clone(), sha256);
        }
        cancel.check()?;

        on_phase(PluginInstallPhase::Installing);
        let installed = self.get(&id);
        self.swap_package(&dir, &staged, &staging.join(RETIRED))?;
        let record = PluginRecord {
            id: id.clone(),
            source: source.kind(),
            wasm_sha256,
            theme_sha256,
            grant,
            grant_digest,
            enabled: installed.as_ref().is_none_or(|record| record.enabled),
            installed_unix: common::now_unix(),
        };
        self.save(&record)?;
        tracing::info!(plugin = %id, updated = installed.is_some(), "plugin installed");
        Ok(record)
    }

    /// Moves the old package aside before the new one goes in, and back if
    /// that fails, so the plugin directory never holds half of each.
    fn swap_package(&self, dir: &Path, staged: &Path, retired: &Path) -> Result<()> {
        let _writes = self.writes.lock().unwrap();
        std::fs::create_dir_all(dir)?;
        let package = dir.join(PACKAGE);
        let replacing = package.exists();
        if replacing {
            std::fs::rename(&package, retired).context("cannot set the old package aside")?;
        }
        if let Err(error) = std::fs::rename(staged, &package) {
            if replacing {
                if let Err(restore) = std::fs::rename(retired, &package) {
                    tracing::error!(%restore, "cannot restore the old plugin package");
                }
            }
            return Err(error).context("cannot move the package into place");
        }
        Ok(())
    }
}
