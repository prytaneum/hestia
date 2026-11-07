//! Installs and tracks Java runtimes: each install lives at
//! `<dir>/<vendor>-<major>/` beside a `runtime.json` record, and listing scans
//! that directory — the disk is the registry.

mod adoptium;
mod platform;
mod provider;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use proto::java::{JavaInstallPhase, JavaInstallProgress, JavaRelease, JavaRuntime};
use serde::{Deserialize, Serialize};

use crate::download::Downloader;
use crate::minecraft::materialize;
use crate::roster::{RegisterError, Roster};

pub use adoptium::ID as DEFAULT_PROVIDER;
pub use platform::JavaTarget;
pub use provider::{JavaPackage, JavaProvider};

const RUNTIME_RECORD: &str = "runtime.json";
/// An install extracts here and is renamed into place, so a runtime is only ever
/// registered whole.
const STAGING_SUFFIX: &str = ".staging";
/// Where the downloaded archive lands before extraction.
const SCRATCH: &str = "tmp";

pub struct JavaInstallOutcome {
    pub runtime: JavaRuntime,
    pub already_installed: bool,
}

pub struct Java {
    dir: PathBuf,
    providers: Roster<dyn JavaProvider>,
}

impl Java {
    pub fn new(dir: PathBuf) -> Self {
        let adoptium: Arc<dyn JavaProvider> = Arc::new(adoptium::Adoptium);
        Java {
            dir,
            providers: Roster::builtin([(adoptium.id().to_string(), adoptium)]),
        }
    }

    /// Add `owner`'s provider under `provider.id()`, which must read `<owner>.<id>`.
    pub fn register(
        &self,
        owner: &str,
        official: bool,
        provider: Arc<dyn JavaProvider>,
    ) -> Result<(), RegisterError> {
        let id = provider.id().to_string();
        self.providers.register(owner, official, &id, provider)
    }

    pub fn unregister(&self, owner: &str) -> usize {
        self.providers.unregister(owner)
    }

    /// The provider registered as `id`; a typed error naming it when none is.
    pub fn provider(&self, id: &str) -> Result<Arc<dyn JavaProvider>> {
        self.providers
            .snapshot()
            .into_iter()
            .find(|p| p.id() == id)
            .ok_or_else(|| {
                proto::error::ErrorInfo::JavaProviderUnknown { id: id.to_string() }.into()
            })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Drop the `.staging` trees and downloaded archives an interrupted install
    /// left behind. A runtime is only registered by the rename out of staging,
    /// so anything still staged belongs to a job that is gone — see
    /// `crate::reclaim`.
    pub fn reclaim(&self) -> crate::reclaim::Reclaimed {
        let base = self.dir();
        let mut freed = crate::reclaim::suffixed(base, STAGING_SUFFIX);
        freed += crate::reclaim::contents(&base.join(SCRATCH));
        freed
    }

    pub async fn releases(&self, provider: &str) -> Result<Vec<JavaRelease>> {
        self.provider(provider)?.releases().await
    }

    /// Scan the install directory; the disk is the registry.
    pub fn installed(&self) -> Vec<JavaRuntime> {
        let mut runtimes = Vec::new();
        if let Ok(entries) = std::fs::read_dir(self.dir()) {
            for entry in entries.flatten() {
                if entry.path().is_dir() {
                    if let Some(runtime) = read_runtime(&entry.path()) {
                        runtimes.push(runtime);
                    }
                }
            }
        }
        runtimes.sort_by_key(|r| r.major);
        runtimes
    }

    pub fn uninstall(&self, major: i32) -> bool {
        let mut removed = false;
        if let Ok(entries) = std::fs::read_dir(self.dir()) {
            for entry in entries.flatten() {
                if !entry.path().is_dir() {
                    continue;
                }
                if read_runtime(&entry.path()).map(|r| r.major) == Some(major) {
                    let _ = std::fs::remove_dir_all(entry.path());
                    removed = true;
                }
            }
        }
        if removed {
            tracing::info!(major, "uninstalled java runtime");
        } else {
            tracing::debug!(major, "no java runtime to uninstall");
        }
        removed
    }

    /// Bytes an install of `major` would download: `0` when it is already
    /// installed, so a launch can plan its total before fetching anything.
    pub async fn download_size(&self, provider: &str, major: i32) -> Result<u64> {
        if self.installed().iter().any(|r| r.major == major) {
            return Ok(0);
        }
        let provider = self.provider(provider)?;
        Ok(provider
            .resolve(major, &platform::host_target()?)
            .await?
            .size)
    }

    /// Blocking resolve → download → extract → register; a failed install leaves
    /// nothing behind. An already-installed line is returned as-is unless `force`.
    pub async fn install(
        &self,
        provider: &str,
        major: i32,
        force: bool,
        cancel: &crate::cancel::Cancel,
        on_progress: impl Fn(&JavaInstallProgress) + Send + Sync,
    ) -> Result<JavaInstallOutcome> {
        if major <= 0 {
            bail!(proto::error::ErrorInfo::InvalidValue {
                field: proto::error::Field::JavaVersion,
                reason: proto::error::Reason::JavaMajor
            });
        }
        tracing::info!(provider, major, force, "java install requested");
        if !force {
            if let Some(runtime) = self.installed().into_iter().find(|r| r.major == major) {
                tracing::info!(major, release = %runtime.release_name, "java already installed");
                return Ok(JavaInstallOutcome {
                    runtime,
                    already_installed: true,
                });
            }
        }
        let phase = |p: JavaInstallPhase| JavaInstallProgress {
            phase: p,
            current: 0,
            total: 0,
        };

        on_progress(&phase(JavaInstallPhase::Resolving));
        let provider = self.provider(provider)?;
        let package = provider.resolve(major, &platform::host_target()?).await?;
        materialize::validate_filename(&package.archive_name)?;
        tracing::info!(
            major,
            release = %package.release_name,
            archive = %package.archive_name,
            "resolved java package"
        );

        let paths = InstallPaths::for_package(self.dir(), &package);

        let _ = std::fs::remove_dir_all(&paths.staging);
        let result = self
            .run_install(&package, &paths, cancel, &on_progress)
            .await;
        if let Err(e) = &result {
            // A cancellation is not a failure: it is logged as what it is, and
            // cleaned up the same way, because the staging discipline does not
            // care why the install stopped.
            match crate::cancel::is_cancelled(e) {
                true => tracing::info!(major, "java install cancelled"),
                false => tracing::error!(major, "java install failed: {e:#}"),
            }
            paths.discard();
        }
        result?;
        let _ = std::fs::remove_file(&paths.archive);

        let outcome = read_runtime(&paths.install_dir)
            .map(|runtime| JavaInstallOutcome {
                runtime,
                already_installed: false,
            })
            .with_context(|| {
                format!(
                    "install of {} did not produce a usable runtime",
                    package.release_name
                )
            })?;
        tracing::info!(
            major,
            home = %outcome.runtime.home.display(),
            "installed java runtime"
        );
        Ok(outcome)
    }

    async fn run_install(
        &self,
        package: &JavaPackage,
        paths: &InstallPaths,
        cancel: &crate::cancel::Cancel,
        on_progress: &(impl Fn(&JavaInstallProgress) + Send + Sync),
    ) -> Result<()> {
        Downloader::new(None)
            .fetch(
                &package.url,
                &paths.archive,
                Some(&package.checksum),
                &|dp| {
                    // A JDK is a couple of hundred megabytes: the chunk loop is
                    // where a cancel has to be noticed, not after it lands.
                    cancel.check()?;
                    on_progress(&JavaInstallProgress {
                        phase: JavaInstallPhase::Downloading,
                        current: dp.downloaded,
                        total: dp.total,
                    });
                    Ok(())
                },
            )
            .await?;

        cancel.check()?;
        on_progress(&JavaInstallProgress {
            phase: JavaInstallPhase::Extracting,
            current: 0,
            total: 0,
        });
        tracing::debug!(archive = %paths.archive.display(), "extracting java archive");
        let archive_owned = paths.archive.clone();
        let staging_owned = paths.staging.clone();
        tokio::task::spawn_blocking(move || {
            crate::extract::extract_archive(&archive_owned, &staging_owned)
        })
        .await
        .context("extraction task panicked")??;

        let executable = platform::find_java_executable(&paths.staging).with_context(|| {
            format!(
                "archive {} contained no java executable",
                package.archive_name
            )
        })?;
        let relative = executable
            .strip_prefix(&paths.staging)
            .unwrap_or(&executable);
        write_runtime_record(&paths.staging, package, relative)?;

        let _ = std::fs::remove_dir_all(&paths.install_dir);
        std::fs::rename(&paths.staging, &paths.install_dir)
            .context("moving staged install into place")?;
        Ok(())
    }
}

/// Where one install's pieces live. The three travel together because they are
/// one story — the archive is extracted into staging, and only the rename onto
/// `install_dir` makes the runtime real — so passing them as three parameters
/// only made every signature that touches an install longer.
struct InstallPaths {
    /// The downloaded archive, under the scratch dir.
    archive: PathBuf,
    /// Where it extracts; a runtime here is not yet registered.
    staging: PathBuf,
    /// What the rename commits to — the disk is the registry.
    install_dir: PathBuf,
}

impl InstallPaths {
    fn for_package(base: &Path, package: &JavaPackage) -> Self {
        let install_dir = base.join(format!("{}-{}", package.vendor, package.major));
        InstallPaths {
            archive: base.join(SCRATCH).join(&package.archive_name),
            staging: crate::reclaim::with_suffix(&install_dir, STAGING_SUFFIX),
            install_dir,
        }
    }

    /// Drop everything a failed or cancelled install staged. What it does not
    /// touch is `install_dir`: an earlier, working runtime must survive.
    fn discard(&self) {
        let _ = std::fs::remove_dir_all(&self.staging);
        let _ = std::fs::remove_file(&self.archive);
    }
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct RuntimeRecord {
    vendor: String,
    major: i32,
    release_name: String,
    executable: String,
}

fn write_runtime_record(
    install_dir: &Path,
    package: &JavaPackage,
    relative_exe: &Path,
) -> Result<()> {
    let record = RuntimeRecord {
        vendor: package.vendor.clone(),
        major: package.major,
        release_name: package.release_name.clone(),
        executable: to_forward_slashes(relative_exe),
    };
    let text = serde_json::to_string_pretty(&record).expect("record serializes");
    std::fs::write(install_dir.join(RUNTIME_RECORD), format!("{text}\n"))
        .with_context(|| format!("cannot write {RUNTIME_RECORD}"))?;
    Ok(())
}

fn read_runtime(install_dir: &Path) -> Option<JavaRuntime> {
    let text = std::fs::read_to_string(install_dir.join(RUNTIME_RECORD)).ok()?;
    let record: RuntimeRecord = serde_json::from_str(&text).ok()?;
    let executable = install_dir.join(&record.executable);
    if record.major <= 0 || !executable.is_file() {
        return None;
    }
    let home = executable
        .parent()
        .and_then(Path::parent)
        .unwrap_or(install_dir)
        .to_path_buf();
    Some(JavaRuntime {
        vendor: record.vendor,
        major: record.major,
        release_name: record.release_name,
        home,
        executable,
        // Usage is a cross-store question; the daemon's list handler fills it.
        in_use: false,
    })
}

fn to_forward_slashes(path: &Path) -> String {
    path.components()
        .filter_map(|c| c.as_os_str().to_str())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;

    struct Stub;

    #[async_trait]
    impl JavaProvider for Stub {
        fn id(&self) -> &str {
            "acme.zulu"
        }

        async fn releases(&self) -> Result<Vec<JavaRelease>> {
            Ok(vec![JavaRelease {
                major: 21,
                lts: false,
            }])
        }

        async fn resolve(&self, _: i32, _: &JavaTarget) -> Result<JavaPackage> {
            bail!("stub")
        }
    }

    #[test]
    fn the_default_provider_is_adoptium() {
        let java = Java::new(PathBuf::from("java"));
        assert_eq!(
            crate::config::Settings::default().java.provider,
            DEFAULT_PROVIDER
        );
        assert_eq!(
            java.provider(DEFAULT_PROVIDER).expect("built in").id(),
            "adoptium"
        );
    }

    #[tokio::test]
    async fn a_registered_provider_is_selected_by_id_and_unknown_ids_are_named() {
        let java = Java::new(PathBuf::from("java"));
        java.register("acme", false, Arc::new(Stub))
            .expect("namespaced");

        let releases = java.releases("acme.zulu").await.expect("stub releases");
        assert_eq!(releases[0].major, 21);

        let error = java.releases("nope").await.expect_err("unknown");
        assert!(matches!(
            error.downcast_ref::<proto::error::ErrorInfo>(),
            Some(proto::error::ErrorInfo::JavaProviderUnknown { id }) if id == "nope"
        ));

        assert_eq!(java.unregister("acme"), 1);
        assert!(java.provider("acme.zulu").is_err());
    }
}
