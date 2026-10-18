//! Self-update over the published release manifest (`latest.json`): the version
//! check, the signed artifact download, and applying it over this build.
//!
//! The daemon owns all three, so the endpoint, the trusted keys and the install
//! shapes are written down once and every front-end reaches them over `update.*`.

mod apply;
mod install;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use proto::error::Service;
use proto::update::{UpdateChannel, UpdateCheckResult, UpdateInfo, UpdateInstall};

use crate::download::{Downloader, ProgressFn};
use crate::minecraft::materialize;
use crate::signature::verify_file;
use crate::version::is_newer;

pub use apply::Applied;

pub struct Update {
    dir: PathBuf,
}

/// Every response the feed API produces is wrapped in one envelope, success or
/// failure alike, so the manifest arrives one level down. A failure carries no
/// `data` at all — which is why an error status is caught before this is read.
#[derive(serde::Deserialize)]
struct Envelope {
    data: Manifest,
}

#[derive(serde::Deserialize)]
struct Manifest {
    version: String,
    #[serde(default)]
    notes: String,
    #[serde(default)]
    platforms: HashMap<String, PlatformEntry>,
}

/// A platform's default artifact — the NSIS setup, the AppImage — beside the
/// other formats it ships. `formats` is additive: a build predating a format
/// never asks for it and reads the same manifest a newer one does.
#[derive(serde::Deserialize, Clone)]
struct PlatformEntry {
    url: String,
    signature: String,
    #[serde(default)]
    formats: HashMap<String, Artifact>,
}

#[derive(serde::Deserialize, Clone)]
struct Artifact {
    url: String,
    signature: String,
}

impl From<&PlatformEntry> for Artifact {
    fn from(entry: &PlatformEntry) -> Self {
        Artifact {
            url: entry.url.clone(),
            signature: entry.signature.clone(),
        }
    }
}

impl Update {
    pub fn new(dir: PathBuf) -> Self {
        Update { dir }
    }

    pub async fn check(&self, channel: UpdateChannel) -> Result<UpdateCheckResult> {
        let manifest = fetch_manifest(channel).await?;
        let install = install::detect();
        Ok(UpdateCheckResult {
            current: common::app::VERSION.to_string(),
            install,
            channel,
            available: available(&manifest).map(|entry| {
                let artifact = artifact_for(entry, install);
                UpdateInfo {
                    version: manifest.version.clone(),
                    notes: manifest.notes.clone(),
                    url: artifact
                        .as_ref()
                        .map(|a| a.url.clone())
                        .unwrap_or_else(|| entry.url.clone()),
                    applicable: artifact.is_some(),
                }
            }),
        })
    }

    /// Download the artifact matching how this copy was installed, discarding
    /// one whose signature does not verify. Returns the path and its version.
    pub async fn download(
        &self,
        channel: UpdateChannel,
        on_progress: &ProgressFn<'_>,
    ) -> Result<(PathBuf, String)> {
        let manifest = fetch_manifest(channel).await?;
        let install = install::detect();
        let entry = available(&manifest)
            .ok_or_else(|| anyhow!("{} is already the latest version", common::app::VERSION))?;
        let artifact = artifact_for(entry, install).ok_or_else(|| {
            anyhow!("this release has no artifact for how this copy of hestia was installed")
        })?;

        let name = artifact
            .url
            .rsplit('/')
            .next()
            .context("update url has no file name")?
            .to_string();
        materialize::validate_filename(&name)?;
        std::fs::create_dir_all(&self.dir).context("cannot create the update staging directory")?;
        let dest = self.dir.join(name);
        Downloader::new(None)
            .fetch(&artifact.url, &dest, None, on_progress)
            .await?;
        if let Err(e) = verify_file(&dest, &artifact.signature, common::app::update_pubkeys()) {
            let _ = std::fs::remove_file(&dest);
            return Err(e.context("update signature verification failed"));
        }
        Ok((dest, manifest.version))
    }

    /// `artifact` must be one this daemon staged: it arrives from a client and
    /// ends up executed with whatever rights the install shape needs.
    pub fn apply(&self, artifact: &Path) -> Result<Applied> {
        if artifact.parent() != Some(self.dir.as_path()) {
            bail!("{} is not a staged update artifact", artifact.display());
        }
        if !artifact.is_file() {
            bail!("the staged update artifact is gone — download it again");
        }
        apply::apply(artifact, install::detect())
    }
}

/// `HESTIA_UPDATE_ENDPOINT` points a **debug** build at a local feed, standing
/// in for the whole URL — channel segment included, so one served file drives
/// either channel. The signature is still checked against the compiled-in keys,
/// so a local test signs with a key this build trusts.
fn endpoint(channel: UpdateChannel) -> String {
    #[cfg(debug_assertions)]
    if let Ok(url) = std::env::var("HESTIA_UPDATE_ENDPOINT") {
        if !url.trim().is_empty() {
            return url;
        }
    }
    format!(
        "{}/{}",
        common::app::UPDATE_ENDPOINT.trim_end_matches('/'),
        channel.as_str()
    )
}

async fn fetch_manifest(channel: UpdateChannel) -> Result<Manifest> {
    let envelope: Envelope = crate::net::get_json(Service::Release, &endpoint(channel))
        .await
        .context("cannot read the update manifest")?;
    Ok(envelope.data)
}

fn available(manifest: &Manifest) -> Option<&PlatformEntry> {
    if !is_newer(&manifest.version, common::app::VERSION) {
        return None;
    }
    manifest.platforms.get(&format!(
        "{}-{}",
        std::env::consts::OS,
        std::env::consts::ARCH
    ))
}

/// A package install accepts only its own format: handing a deb install an
/// AppImage downloads something it has no way to apply.
fn artifact_for(entry: &PlatformEntry, install: UpdateInstall) -> Option<Artifact> {
    match install {
        UpdateInstall::Unmanaged => None,
        UpdateInstall::Nsis | UpdateInstall::AppImage => Some(Artifact::from(entry)),
        UpdateInstall::Deb | UpdateInstall::Rpm => entry
            .formats
            .get(install::manifest_format(install)?)
            .cloned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry() -> PlatformEntry {
        PlatformEntry {
            url: "https://example.test/Hestia.AppImage".into(),
            signature: "default-sig".into(),
            formats: HashMap::from([(
                "deb".to_string(),
                Artifact {
                    url: "https://example.test/hestia.deb".into(),
                    signature: "deb-sig".into(),
                },
            )]),
        }
    }

    #[test]
    fn a_package_install_takes_its_own_format() {
        let artifact = artifact_for(&entry(), UpdateInstall::Deb).unwrap();
        assert_eq!(artifact.url, "https://example.test/hestia.deb");
        assert_eq!(artifact.signature, "deb-sig");
    }

    #[test]
    fn the_default_entry_serves_appimage_and_nsis() {
        for install in [UpdateInstall::AppImage, UpdateInstall::Nsis] {
            let artifact = artifact_for(&entry(), install).unwrap();
            assert_eq!(artifact.url, "https://example.test/Hestia.AppImage");
        }
    }

    #[test]
    fn a_format_the_manifest_omits_is_not_substituted() {
        assert!(artifact_for(&entry(), UpdateInstall::Rpm).is_none());
    }

    #[test]
    fn an_unmanaged_build_is_offered_nothing_to_apply() {
        assert!(artifact_for(&entry(), UpdateInstall::Unmanaged).is_none());
    }

    #[test]
    fn the_manifest_is_read_out_of_the_response_envelope() {
        let envelope: Envelope = serde_json::from_str(
            r#"{"success": true, "message": "Current release",
                "data": {"version": "1.3.0", "channel": "stable", "platforms": {}}}"#,
        )
        .unwrap();
        assert_eq!(envelope.data.version, "1.3.0");
        assert!(envelope.data.notes.is_empty());
    }

    #[test]
    fn a_manifest_without_formats_still_reads() {
        let entry: PlatformEntry =
            serde_json::from_str(r#"{"url": "https://e.test/setup.exe", "signature": "sig"}"#)
                .unwrap();
        assert!(entry.formats.is_empty());
        assert_eq!(
            artifact_for(&entry, UpdateInstall::Nsis).unwrap().url,
            "https://e.test/setup.exe"
        );
    }

    #[test]
    fn only_a_newer_version_is_available() {
        let manifest = Manifest {
            version: "0.0.0".into(),
            notes: String::new(),
            platforms: HashMap::from([(
                format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
                entry(),
            )]),
        };
        assert!(available(&manifest).is_none());
    }
}
