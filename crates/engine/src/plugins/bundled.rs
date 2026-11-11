//! The official plugins an installer carries, installed on the first start
//! after they arrive. A bundled package is trusted for one reason: its
//! `.minisig` verifies against an official key, which is why its computed
//! grant installs without a prompt (the user approved the installer that
//! carries it). Nothing here can install a package that fails that check.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use proto::plugin::{compare_versions, Grant, PluginSource};
use serde::{Deserialize, Serialize};

use super::official::is_official_signer_in;
use super::remote::{Origin, Staged};
use super::repos::verify_build;
use super::repositories::official_url;
use super::store::{Approver, Plugins};
use crate::cancel::Cancel;
use crate::schema::{self, Document};

const PACKAGE_EXTENSION: &str = "hplugin";
const SIGNATURE_SUFFIX: &str = ".minisig";
const PACKAGE_LIMIT: u64 = 64 * 1024 * 1024;
const SIGNATURE_LIMIT: u64 = 16 * 1024;

/// Every id this build has seen installed from a bundle. A bundled plugin
/// that is absent yet listed here was removed by the user and stays removed.
#[derive(Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct Ledger {
    seeded: BTreeSet<String>,
}

impl Document for Ledger {
    const NAME: &'static str = "bundled.json";
}

/// A bundled package that was not installed, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundleRefusal {
    pub file: String,
    pub reason: String,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Seeded {
    pub installed: Vec<String>,
    pub upgraded: Vec<String>,
    pub refused: Vec<BundleRefusal>,
}

enum Outcome {
    Installed,
    Upgraded,
    Kept,
}

impl Plugins {
    /// Blocking. Installs each package in `dir` that is missing, or newer than
    /// the official install it would replace; `keys` are the official keys.
    /// Never replaces a newer version, never re-installs a removed plugin and
    /// never widens an installed grant. A missing `dir` seeds nothing.
    pub fn seed_bundled(
        &self,
        dir: &Path,
        keys: &[String],
        validate: impl Fn(&[u8]) -> Result<()>,
    ) -> Seeded {
        let mut seeded = Seeded::default();
        let packages = packages_in(dir);
        if packages.is_empty() {
            tracing::debug!(dir = %dir.display(), "no bundled plugins");
            return seeded;
        }
        let ledger_path = self.dir.join(Ledger::NAME);
        let mut ledger = schema::load::<Ledger>(&ledger_path).unwrap_or_default();
        for path in packages {
            match self.seed_one(&path, keys, &validate, &mut ledger) {
                Ok((id, Outcome::Installed)) => seeded.installed.push(id),
                Ok((id, Outcome::Upgraded)) => seeded.upgraded.push(id),
                Ok((_, Outcome::Kept)) => {}
                Err(error) => {
                    let file = file_name(&path);
                    let reason = format!("{error:#}");
                    tracing::error!(%file, %reason, "bundled plugin not installed");
                    seeded.refused.push(BundleRefusal { file, reason });
                }
            }
        }
        if let Err(error) = schema::save(&ledger_path, &ledger) {
            tracing::error!(%error, "cannot record the bundled plugins that were seeded");
        }
        seeded
    }

    fn seed_one(
        &self,
        path: &Path,
        keys: &[String],
        validate: &impl Fn(&[u8]) -> Result<()>,
        ledger: &mut Ledger,
    ) -> Result<(String, Outcome)> {
        let inspection = self.inspect(path)?;
        let id = inspection.manifest.id.clone();
        let version = inspection.manifest.version.clone();
        let signer = verify(path, keys, &id, &version)?;

        let outcome = match self.get(&id) {
            Some(installed) => {
                ledger.seeded.insert(id.clone());
                let official: Vec<&str> = keys.iter().map(String::as_str).collect();
                let ours = installed.source != PluginSource::Dev
                    && is_official_signer_in(installed.signer.as_deref(), &official);
                let current = self.read_manifest(&id)?.version;
                if !ours || compare_versions(&version, &current).is_le() {
                    tracing::debug!(plugin = %id, %version, %current, "bundled plugin is not newer; kept");
                    return Ok((id, Outcome::Kept));
                }
                if inspection.grant.widening_over(&installed.grant) != Grant::default() {
                    bail!("{version} asks for more than {current} was approved for; update it to approve");
                }
                Outcome::Upgraded
            }
            None if ledger.seeded.contains(&id) => {
                tracing::debug!(plugin = %id, "bundled plugin was removed; leaving it removed");
                return Ok((id, Outcome::Kept));
            }
            None => Outcome::Installed,
        };

        let staged = Staged {
            path: path.to_path_buf(),
            url: String::new(),
            origin: Some(Origin {
                signer,
                repository: official_url(),
            }),
            warnings: Vec::new(),
        };
        let mut record = self.install_downloaded(
            &staged,
            &inspection.grant_digest,
            &Cancel::new(),
            &|_| {},
            |wasm| validate(wasm),
        )?;
        record.url = None;
        record.approved_by = Approver::Official;
        self.save(&record)?;
        ledger.seeded.insert(id.clone());
        tracing::info!(plugin = %id, %version, "bundled plugin installed");
        Ok((id, outcome))
    }
}

/// The key id of the official key that signed the package at `path`.
fn verify(path: &Path, keys: &[String], id: &str, version: &str) -> Result<String> {
    let bytes = read_capped(path, PACKAGE_LIMIT)?;
    let signature_path = PathBuf::from(format!("{}{SIGNATURE_SUFFIX}", path.display()));
    let signature = String::from_utf8(read_capped(&signature_path, SIGNATURE_LIMIT)?)
        .context("the signature is not text")?;
    Ok(verify_build(
        &bytes,
        &signature,
        keys,
        id,
        version,
        &file_name(path),
    )?)
}

fn read_capped(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let size = std::fs::metadata(path)
        .with_context(|| format!("cannot read {}", path.display()))?
        .len();
    if size > limit {
        bail!("{} is larger than {limit} bytes", path.display());
    }
    std::fs::read(path).with_context(|| format!("cannot read {}", path.display()))
}

fn packages_in(dir: &Path) -> Vec<PathBuf> {
    let mut packages: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file() && path.extension().is_some_and(|ext| ext == PACKAGE_EXTENSION)
        })
        .collect();
    packages.sort();
    packages
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use proto::plugin::API_VERSION;
    use proto::repository::build_comment;
    use zip::write::SimpleFileOptions;

    use super::*;
    use crate::plugins::repos::key_id_of;
    use crate::plugins::testkit::Signer;

    struct Home {
        _root: tempfile::TempDir,
        plugins: Plugins,
        bundle: PathBuf,
    }

    fn home() -> Home {
        let root = tempfile::tempdir().unwrap();
        let bundle = root.path().join("bundle");
        std::fs::create_dir_all(&bundle).unwrap();
        Home {
            plugins: Plugins::new(root.path().join("plugins")),
            bundle,
            _root: root,
        }
    }

    fn archive(id: &str, version: &str, permissions: &str) -> Vec<u8> {
        let manifest = format!(
            "id = \"{id}\"\nversion = \"{version}\"\napi = \"^{API_VERSION}\"\nwasm = \"plugin.wasm\"\n{permissions}"
        );
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (name, body) in [
            ("plugin.toml", manifest.as_bytes()),
            ("plugin.wasm", b"component"),
        ] {
            zip.start_file(name, SimpleFileOptions::default()).unwrap();
            zip.write_all(body).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    fn signature_of(path: &Path) -> PathBuf {
        PathBuf::from(format!("{}{SIGNATURE_SUFFIX}", path.display()))
    }

    fn bundle(home: &Home, signer: &Signer, id: &str, version: &str, permissions: &str) -> PathBuf {
        let file = format!("{id}-{version}.hplugin");
        let bytes = archive(id, version, permissions);
        let path = home.bundle.join(&file);
        std::fs::write(&path, &bytes).unwrap();
        let signature = signer.sign(&bytes, &build_comment(id, version, &file));
        std::fs::write(signature_of(&path), signature).unwrap();
        path
    }

    fn unbundle(path: &Path) {
        std::fs::remove_file(path).unwrap();
        std::fs::remove_file(signature_of(path)).unwrap();
    }

    fn seed(home: &Home, signer: &Signer) -> Seeded {
        home.plugins
            .seed_bundled(&home.bundle, &[signer.key()], |_| Ok(()))
    }

    fn version_of(home: &Home, id: &str) -> String {
        home.plugins.read_manifest(id).unwrap().version
    }

    #[test]
    fn a_missing_or_empty_bundle_seeds_nothing() {
        let home = home();
        let official = Signer::new();

        assert_eq!(seed(&home, &official), Seeded::default());
        let gone =
            home.plugins
                .seed_bundled(&home.bundle.join("absent"), &[official.key()], |_| Ok(()));
        assert_eq!(gone, Seeded::default());
        assert!(home.plugins.list().is_empty());
    }

    #[test]
    fn a_verified_package_installs_pre_approved_and_enabled() {
        let home = home();
        let official = Signer::new();
        bundle(
            &home,
            &official,
            "aa",
            "1.0.0",
            "permissions = [\"entry:read\"]\n",
        );

        let seeded = seed(&home, &official);

        assert_eq!(seeded.installed, ["aa"]);
        let record = home.plugins.get("aa").unwrap();
        assert_eq!(record.approved_by, Approver::Official);
        assert_eq!(record.source, PluginSource::Repo);
        assert_eq!(record.url, None);
        assert_eq!(record.signer, Some(key_id_of(&official.key()).unwrap()));
        assert_eq!(record.grant_digest, record.grant.digest());
        assert!(record.enabled);
        assert!(!record.grant.permissions.is_empty());
    }

    #[test]
    fn a_restart_changes_nothing() {
        let home = home();
        let official = Signer::new();
        bundle(&home, &official, "aa", "1.0.0", "");
        seed(&home, &official);

        assert_eq!(seed(&home, &official), Seeded::default());
    }

    #[test]
    fn a_removed_plugin_stays_removed() {
        let home = home();
        let official = Signer::new();
        bundle(&home, &official, "aa", "1.0.0", "");
        seed(&home, &official);
        home.plugins.remove("aa").unwrap();

        assert_eq!(seed(&home, &official), Seeded::default());
        assert!(home.plugins.get("aa").is_none());
    }

    #[test]
    fn a_plugin_installed_before_the_bundle_is_not_resurrected_once_removed() {
        let home = home();
        let official = Signer::new();
        let path = bundle(&home, &official, "aa", "1.0.0", "");
        let digest = home.plugins.inspect(&path).unwrap().grant_digest;
        home.plugins
            .install(&path, &digest, &Cancel::new(), &|_| {}, |_| Ok(()))
            .unwrap();
        seed(&home, &official);
        home.plugins.remove("aa").unwrap();

        seed(&home, &official);

        assert!(home.plugins.get("aa").is_none());
    }

    #[test]
    fn a_newer_bundle_upgrades_and_an_older_one_never_downgrades() {
        let home = home();
        let official = Signer::new();
        bundle(&home, &official, "aa", "1.0.0", "");
        seed(&home, &official);

        let newer = bundle(&home, &official, "aa", "2.0.0", "");
        assert_eq!(seed(&home, &official).upgraded, ["aa"]);
        assert_eq!(version_of(&home, "aa"), "2.0.0");

        unbundle(&newer);
        assert_eq!(seed(&home, &official), Seeded::default());
        assert_eq!(version_of(&home, "aa"), "2.0.0");
    }

    #[test]
    fn an_upgrade_that_widens_the_grant_is_refused() {
        let home = home();
        let official = Signer::new();
        bundle(&home, &official, "aa", "1.0.0", "");
        seed(&home, &official);
        bundle(
            &home,
            &official,
            "aa",
            "2.0.0",
            "permissions = [\"entry:read\"]\n",
        );

        let seeded = seed(&home, &official);

        assert!(seeded.upgraded.is_empty());
        assert_eq!(seeded.refused.len(), 1);
        assert_eq!(version_of(&home, "aa"), "1.0.0");
    }

    #[test]
    fn a_package_a_third_party_signed_is_not_installed() {
        let home = home();
        let (official, other) = (Signer::new(), Signer::new());
        bundle(&home, &other, "aa", "1.0.0", "");

        let seeded = seed(&home, &official);

        assert!(seeded.installed.is_empty());
        assert_eq!(seeded.refused.len(), 1);
        assert_eq!(seeded.refused[0].file, "aa-1.0.0.hplugin");
        assert!(home.plugins.get("aa").is_none());
    }

    #[test]
    fn a_tampered_or_unsigned_package_is_not_installed() {
        let home = home();
        let official = Signer::new();
        let tampered = bundle(&home, &official, "aa", "1.0.0", "");
        std::fs::write(
            &tampered,
            archive("aa", "1.0.0", "permissions = [\"entry:write\"]\n"),
        )
        .unwrap();
        let unsigned = bundle(&home, &official, "bb", "1.0.0", "");
        std::fs::remove_file(signature_of(&unsigned)).unwrap();

        let seeded = seed(&home, &official);

        assert!(seeded.installed.is_empty());
        assert_eq!(seeded.refused.len(), 2);
        assert!(home.plugins.list().is_empty());
    }

    #[test]
    fn a_signature_made_for_another_file_name_is_refused() {
        let home = home();
        let official = Signer::new();
        let path = bundle(&home, &official, "aa", "1.0.0", "");
        let renamed = home.bundle.join("zz.hplugin");
        std::fs::rename(&path, &renamed).unwrap();
        std::fs::rename(signature_of(&path), signature_of(&renamed)).unwrap();

        assert_eq!(seed(&home, &official).refused.len(), 1);
    }

    #[test]
    fn one_refused_package_does_not_stop_the_others() {
        let home = home();
        let (official, other) = (Signer::new(), Signer::new());
        bundle(&home, &other, "aa", "1.0.0", "");
        bundle(&home, &official, "bb", "1.0.0", "");

        let seeded = seed(&home, &official);

        assert_eq!(seeded.installed, ["bb"]);
        assert_eq!(seeded.refused.len(), 1);
    }
}
