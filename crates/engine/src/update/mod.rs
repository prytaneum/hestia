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
use proto::plugin::is_api_version;
use proto::update::{UpdateChannel, UpdateCheckResult, UpdateInfo, UpdateInstall};

use crate::download::{Downloader, ProgressFn};
use crate::minecraft::materialize;
use crate::signature::verify_file;
use crate::version::is_newer;

pub use apply::Applied;

const STAGED_PLUGINS: &str = "plugins";

/// What a newer manifest offers this install: the version, the plugin API its
/// daemon exposes, and the artifact to fetch.
pub struct Release {
    pub version: String,
    pub plugin_api: Option<String>,
    artifact: Artifact,
}

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
    /// The plugin API the release's daemon exposes. A manifest predating the
    /// field asks for no plugin coordination.
    #[serde(default)]
    plugin_api: Option<String>,
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

    /// The release to download for this install, or why there is none.
    pub async fn release(&self, channel: UpdateChannel) -> Result<Release> {
        let manifest = fetch_manifest(channel).await?;
        let install = install::detect();
        let entry = available(&manifest)
            .ok_or_else(|| anyhow!("{} is already the latest version", common::app::VERSION))?;
        let artifact = artifact_for(entry, install).ok_or_else(|| {
            anyhow!("this release has no artifact for how this copy of hestia was installed")
        })?;
        let plugin_api = manifest.plugin_api.filter(|api| {
            let valid = is_api_version(api);
            if !valid {
                tracing::warn!(%api, "the release manifest names a plugin API that is not a version; ignored");
            }
            valid
        });
        Ok(Release {
            version: manifest.version,
            plugin_api,
            artifact,
        })
    }

    /// Download the release's artifact, discarding one whose signature does
    /// not verify. Returns where it was staged.
    pub async fn download(
        &self,
        release: &Release,
        on_progress: &ProgressFn<'_>,
    ) -> Result<PathBuf> {
        let artifact = &release.artifact;
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
        Ok(dest)
    }

    /// Where the official plugins built for plugin API `api` wait for the
    /// daemon that exposes it. Inside the data home, which an installer
    /// leaves alone, so the new daemon finds them wherever it was installed.
    pub fn staged_plugins(&self, api: &str) -> PathBuf {
        self.dir.join(STAGED_PLUGINS).join(api)
    }

    /// Forgets every staged plugin build, whichever API it was built for.
    pub fn discard_staged_plugins(&self) {
        let _ = std::fs::remove_dir_all(self.dir.join(STAGED_PLUGINS));
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
    fn only_a_newer_version_is_available() {
        let manifest = Manifest {
            version: "0.0.0".into(),
            notes: String::new(),
            plugin_api: None,
            platforms: HashMap::from([(
                format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
                entry(),
            )]),
        };
        assert!(available(&manifest).is_none());
    }

    #[test]
    fn a_manifest_names_the_plugin_api_or_leaves_it_out() {
        let with: Manifest =
            serde_json::from_str(r#"{"version":"1.0.0","plugin_api":"2.0.0"}"#).unwrap();
        let without: Manifest = serde_json::from_str(r#"{"version":"1.0.0"}"#).unwrap();
        assert_eq!(with.plugin_api.as_deref(), Some("2.0.0"));
        assert_eq!(without.plugin_api, None);
    }

    #[test]
    fn staged_plugins_wait_per_api_and_are_discarded_together() {
        let home = tempfile::tempdir().unwrap();
        let update = Update::new(home.path().join("updates"));
        let (next, later) = (
            update.staged_plugins("2.0.0"),
            update.staged_plugins("3.0.0"),
        );
        assert_ne!(next, later);
        for dir in [&next, &later] {
            std::fs::create_dir_all(dir).unwrap();
        }

        update.discard_staged_plugins();

        assert!(!next.exists() && !later.exists());
    }
}
