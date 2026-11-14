//! Reading a plugin package and installing it. Nothing here runs plugin code:
//! the component is only handed to the caller's `validate`.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use proto::error::ErrorInfo;
use proto::plugin::{compare_versions, Grant, Manifest, PluginInstallPhase, PluginSource};
use sha2::{Digest, Sha256};

use super::program::{approval_digest, carried_pins, changed_pins, pin_programs, Pins};
use super::remote::Staged;
use super::store::{
    Approver, PluginRecord, Plugins, Previous, MANIFEST, PACKAGE, PREVIOUS_PACKAGE, STAGING_SUFFIX,
};
use crate::cancel::Cancel;
use crate::checksum::{digest_file, hex};
use crate::extract::extract_zip_capped;
use crate::files::copy_tree;
use crate::reclaim;
use crate::registry;
use crate::usage::dir_size;

/// Uncompressed size of a package. A plugin is a component and a few assets;
/// this bounds what a hostile archive can write.
const PACKAGE_LIMIT: u64 = 64 * 1024 * 1024;
const MANIFEST_LIMIT: u64 = 1024 * 1024;
const RETIRED: &str = "previous";

/// What an update leaves behind: the record now installed and the one it replaced.
pub struct Updated {
    pub record: PluginRecord,
    pub previous: PluginRecord,
}

#[derive(Clone, Copy, Default)]
struct Intake<'a> {
    downloaded: Option<&'a Staged>,
    replacing: Option<&'a PluginRecord>,
}

pub struct Inspection {
    pub manifest: Manifest,
    pub grant: Grant,
    /// What the user approves: the grant together with `program_pins`, see
    /// [`approval_digest`].
    pub grant_digest: String,
    pub installed: Option<PluginRecord>,
    /// Where each `PATH` program resolves now; see [`pin_programs`].
    pub program_pins: Pins,
}

pub(super) enum Source {
    Archive(PathBuf),
    Folder(PathBuf),
}

impl Source {
    pub(super) fn at(path: &Path) -> Result<Source> {
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

    pub(super) fn read_manifest(&self) -> Result<Manifest> {
        let manifest = self.read_declared()?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub(super) fn read_declared(&self) -> Result<Manifest> {
        let text = match self {
            Source::Folder(dir) => read_limited(File::open(dir.join(MANIFEST))?)?,
            Source::Archive(file) => {
                let mut zip = zip::ZipArchive::new(File::open(file)?).context("not a zip")?;
                let entry = zip.by_name(MANIFEST).context("no plugin.toml")?;
                read_limited(entry)?
            }
        };
        toml::from_str(&text).context("invalid plugin.toml")
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

pub(super) fn invalid(detail: impl std::fmt::Display) -> anyhow::Error {
    ErrorInfo::PluginInvalid {
        detail: detail.to_string(),
    }
    .into()
}

/// Hashes the component (after `validate` accepts it) and the themes of the
/// package under `root`.
pub(super) fn digest_package(
    manifest: &Manifest,
    root: &Path,
    validate: impl FnOnce(&[u8]) -> Result<()>,
) -> Result<(Option<String>, BTreeMap<String, String>)> {
    let wasm_sha256 = manifest
        .wasm
        .as_ref()
        .map(|relative| {
            let wasm = std::fs::read(root.join(relative))
                .map_err(|error| invalid(format!("cannot read {relative}: {error}")))?;
            validate(&wasm).map_err(|error| invalid(format!("{error:#}")))?;
            Ok::<_, anyhow::Error>(hex(&Sha256::digest(&wasm)))
        })
        .transpose()?;
    let mut theme_sha256 = BTreeMap::new();
    for theme in &manifest.contributes.themes {
        let sha256 = digest_file::<Sha256>(&root.join(&theme.file))
            .map_err(|error| invalid(format!("cannot read {}: {error}", theme.file)))?;
        theme_sha256.insert(theme.file.clone(), sha256);
    }
    Ok((wasm_sha256, theme_sha256))
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
    /// Blocking. The grant a package asks for, read without asking whether
    /// this host can run it: an app update checks a build for the next plugin
    /// API before the host that would run it exists.
    pub fn declared_grant(&self, path: &Path) -> Result<Grant> {
        Ok(Source::at(path)?.read_declared()?.grant())
    }

    /// Blocking.
    pub fn inspect(&self, path: &Path) -> Result<Inspection> {
        let manifest = Source::at(path)?
            .read_manifest()
            .map_err(|error| invalid(format!("{error:#}")))?;
        let grant = manifest.grant();
        let program_pins = self.pin(&manifest.id, &manifest);
        Ok(Inspection {
            grant_digest: approval_digest(&grant, &program_pins),
            installed: self.get(&manifest.id),
            program_pins,
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
        self.install_from(path, Intake::default(), approve, cancel, on_phase, validate)
    }

    /// [`Self::install`] of an archive fetched from `url`, which the record keeps.
    pub fn install_downloaded(
        &self,
        staged: &Staged,
        approve: &str,
        cancel: &Cancel,
        on_phase: &dyn Fn(PluginInstallPhase),
        validate: impl FnOnce(&[u8]) -> Result<()>,
    ) -> Result<PluginRecord> {
        let intake = Intake {
            downloaded: Some(staged),
            replacing: None,
        };
        self.install_from(&staged.path, intake, approve, cancel, on_phase, validate)
    }

    /// Blocking. Swaps a newer build of an installed repository or link plugin
    /// in, keeping the old package as the one previous version. A grant wider
    /// than the installed one needs `approve` to be its digest; an equal or
    /// narrower one applies without it. `data/` is untouched.
    pub fn update(
        &self,
        staged: &Staged,
        id: &str,
        approve: &str,
        cancel: &Cancel,
        on_phase: &dyn Fn(PluginInstallPhase),
        validate: impl FnOnce(&[u8]) -> Result<()>,
    ) -> Result<Updated> {
        let previous = self
            .get(id)
            .ok_or_else(|| ErrorInfo::PluginNotFound { id: id.to_string() })?;
        if !matches!(previous.source, PluginSource::Repo | PluginSource::Url) {
            return Err(invalid("only a repository or link install can be updated"));
        }
        let intake = Intake {
            downloaded: Some(staged),
            replacing: Some(&previous),
        };
        let record =
            self.install_from(&staged.path, intake, approve, cancel, on_phase, validate)?;
        Ok(Updated { record, previous })
    }

    fn install_from(
        &self,
        path: &Path,
        intake: Intake<'_>,
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
        let resolved = self.pin(&id, &manifest);
        let resolved_digest = approval_digest(&grant, &resolved);
        let (program_pins, approved_by) = match intake.replacing {
            Some(installed) => {
                self.check_update(installed, &manifest, intake.downloaded)?;
                let moved = changed_pins(&installed.program_pins, &resolved, &manifest);
                let widened = grant.widens(&installed.grant);
                if !widened && moved.is_empty() {
                    (
                        carried_pins(&installed.program_pins, &manifest),
                        installed.approved_by,
                    )
                } else if resolved_digest != approve {
                    tracing::warn!(plugin = %id, widened, moved = ?moved, "update refused: it asks for more than was approved");
                    bail!(ErrorInfo::PluginUpdateNeedsApproval { id });
                } else {
                    (resolved, Approver::User)
                }
            }
            None if resolved_digest != approve => {
                tracing::warn!(plugin = %id, "install refused: the grant and pins are not the ones approved");
                bail!(ErrorInfo::PluginApprovalStale);
            }
            None => (resolved, Approver::User),
        };
        let grant_digest = approval_digest(&grant, &program_pins);
        let (wasm_sha256, theme_sha256) = digest_package(&manifest, &staged, validate)?;
        cancel.check()?;

        on_phase(PluginInstallPhase::Installing);
        let installed = self.get(&id);
        let origin = intake.downloaded.and_then(|staged| staged.origin.as_ref());
        let retired = match intake.replacing {
            Some(previous) => {
                self.keep_previous(&dir, previous)?;
                dir.join(PREVIOUS_PACKAGE)
            }
            None => {
                self.discard_previous(&dir);
                staging.join(RETIRED)
            }
        };
        self.swap_package(&dir, &staged, &retired)?;
        let record = PluginRecord {
            id: id.clone(),
            source: match intake.downloaded {
                Some(Staged {
                    origin: Some(_), ..
                }) => PluginSource::Repo,
                Some(_) => PluginSource::Url,
                None => source.kind(),
            },
            url: intake.downloaded.map(|staged| staged.url.clone()),
            dev_path: None,
            signer: origin.map(|origin| origin.signer.clone()),
            repository: origin.map(|origin| origin.repository.clone()),
            wasm_sha256,
            theme_sha256,
            grant,
            grant_digest,
            approved_by,
            program_pins,
            enabled: installed.as_ref().is_none_or(|record| record.enabled),
            installed_unix: common::now_unix(),
        };
        self.save(&record)?;
        tracing::info!(plugin = %id, updated = installed.is_some(), "plugin installed");
        Ok(record)
    }

    pub(super) fn pin(&self, plugin: &str, manifest: &Manifest) -> Pins {
        pin_programs(plugin, manifest, self.resolver)
    }

    fn check_update(
        &self,
        installed: &PluginRecord,
        manifest: &Manifest,
        downloaded: Option<&Staged>,
    ) -> Result<()> {
        if manifest.id != installed.id {
            return Err(invalid("the update is a different plugin"));
        }
        let signer = downloaded
            .and_then(|staged| staged.origin.as_ref())
            .map(|origin| &origin.signer);
        if signer != installed.signer.as_ref() {
            tracing::warn!(plugin = %installed.id, "update refused: not signed by the installed plugin's key");
            return Err(invalid(
                "the update is not signed by the key that signed the installed plugin",
            ));
        }
        let current = self.read_manifest(&installed.id)?.version;
        if !compare_versions(&manifest.version, &current).is_gt() {
            return Err(invalid(format!(
                "{} is not newer than the installed {current}",
                manifest.version
            )));
        }
        Ok(())
    }

    /// Records what the swap is about to replace; the old package follows by rename.
    fn keep_previous(&self, dir: &Path, previous: &PluginRecord) -> Result<()> {
        let _writes = self.writes.lock().unwrap();
        std::fs::create_dir_all(dir)?;
        let retired = dir.join(PREVIOUS_PACKAGE);
        if retired.exists() {
            std::fs::remove_dir_all(&retired).context("cannot clear the older previous version")?;
        }
        registry::write_record(dir, &Previous(previous.clone()))
    }

    fn discard_previous(&self, dir: &Path) {
        let _writes = self.writes.lock().unwrap();
        let _ = std::fs::remove_file(registry::record_path::<Previous>(dir));
        let _ = std::fs::remove_dir_all(dir.join(PREVIOUS_PACKAGE));
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

#[cfg(test)]
mod tests {

    use super::*;
    use crate::cancel::Cancel;
    use crate::plugins::program::Resolver;
    use proto::plugin::ProgramPin;

    const GIT_A: &str = "/opt/a/git";
    const GIT_B: &str = "/opt/b/git";

    fn pin(path: &str, content: &str) -> Option<ProgramPin> {
        Some(ProgramPin {
            path: path.into(),
            sha256: hex(&Sha256::digest(content)),
        })
    }

    fn at_a(_: &str) -> Option<ProgramPin> {
        pin(GIT_A, "git 2.43")
    }

    fn at_b(_: &str) -> Option<ProgramPin> {
        pin(GIT_B, "git 2.43")
    }

    fn upgraded_at_a(_: &str) -> Option<ProgramPin> {
        pin(GIT_A, "git 2.50")
    }

    fn nowhere(_: &str) -> Option<ProgramPin> {
        None
    }

    struct Rig {
        _home: tempfile::TempDir,
        folders: tempfile::TempDir,
        plugins: Plugins,
    }

    impl Rig {
        fn new(resolver: Resolver) -> Rig {
            let home = tempfile::tempdir().unwrap();
            let mut plugins = Plugins::new(home.path().join("plugins"));
            plugins.resolver = resolver;
            Rig {
                _home: home,
                folders: tempfile::tempdir().unwrap(),
                plugins,
            }
        }

        fn resolving(&mut self, resolver: Resolver) {
            self.plugins.resolver = resolver;
        }

        fn package(&self, version: &str, extra: &str) -> Staged {
            self.package_granting(version, "\"git\"", extra)
        }

        fn package_granting(&self, version: &str, granted: &str, extra: &str) -> Staged {
            let dir = self.folders.path().join(version);
            std::fs::create_dir_all(&dir).unwrap();
            let manifest = format!(
                "id = \"demo\"\nname = \"Demo\"\nversion = \"{version}\"\napi = \"^1.0.0\"\nwasm = \"plugin.wasm\"\n\n[permissions]\n\"process:spawn\" = {{ programs = [{granted}] }}\n\n[[programs]]\nid = \"git\"\ncommand = \"git\"\nargs = [\"**\"]\n{extra}"
            );
            std::fs::write(dir.join(MANIFEST), manifest).unwrap();
            std::fs::write(dir.join("plugin.wasm"), version).unwrap();
            Staged {
                path: dir,
                url: "https://repo.example/demo.hplugin".into(),
                origin: None,
                warnings: Vec::new(),
            }
        }

        fn digest(&self, staged: &Staged) -> String {
            self.plugins.inspect(&staged.path).unwrap().grant_digest
        }

        fn install(&self, staged: &Staged) -> PluginRecord {
            let digest = self.digest(staged);
            self.plugins
                .install_downloaded(staged, &digest, &Cancel::new(), &|_| {}, |_| Ok(()))
                .unwrap()
        }

        fn update(&self, staged: &Staged, approve: &str) -> Result<Updated> {
            self.plugins
                .update(staged, "demo", approve, &Cancel::new(), &|_| {}, |_| Ok(()))
        }
    }

    fn refusal(error: anyhow::Error) -> ErrorInfo {
        error.downcast::<ErrorInfo>().expect("a typed refusal")
    }

    #[test]
    fn the_digest_follows_the_pin() {
        let mut rig = Rig::new(at_a);
        let staged = rig.package("1.0.0", "");
        let first = rig.digest(&staged);
        assert_eq!(first, rig.digest(&staged));

        rig.resolving(at_b);
        assert_ne!(first, rig.digest(&staged));
        rig.resolving(upgraded_at_a);
        assert_ne!(first, rig.digest(&staged), "the content hash is bound too");
        rig.resolving(nowhere);
        assert_ne!(first, rig.digest(&staged));
    }

    #[test]
    fn an_install_whose_pin_moved_since_the_preview_is_stale() {
        let mut rig = Rig::new(at_a);
        let staged = rig.package("1.0.0", "");
        let previewed = rig.digest(&staged);

        rig.resolving(at_b);
        let error = rig
            .plugins
            .install_downloaded(&staged, &previewed, &Cancel::new(), &|_| {}, |_| Ok(()))
            .unwrap_err();

        assert!(matches!(refusal(error), ErrorInfo::PluginApprovalStale));
        assert!(rig.plugins.get("demo").is_none());
    }

    #[test]
    fn the_record_digest_covers_its_pins() {
        let rig = Rig::new(at_a);
        let record = rig.install(&rig.package("1.0.0", ""));

        assert_eq!(record.program_pins["git"], at_a("git").unwrap());
        assert_eq!(
            record.grant_digest,
            approval_digest(&record.grant, &record.program_pins)
        );
        assert_eq!(record.source, PluginSource::Url);
    }

    #[test]
    fn an_update_with_the_same_pin_needs_no_approval() {
        let rig = Rig::new(at_a);
        let installed = rig.install(&rig.package("1.0.0", ""));

        let updated = rig.update(&rig.package("1.1.0", ""), "").unwrap();

        assert_eq!(updated.record.program_pins, installed.program_pins);
        assert_eq!(updated.record.grant_digest, installed.grant_digest);
    }

    #[test]
    fn an_update_whose_pin_would_move_needs_the_matching_digest() {
        let mut rig = Rig::new(at_a);
        rig.install(&rig.package("1.0.0", ""));
        let next = rig.package("1.1.0", "");
        rig.resolving(at_b);

        for approve in ["", "stale"] {
            let error = rig.update(&next, approve).map(|_| ()).unwrap_err();
            assert!(matches!(
                refusal(error),
                ErrorInfo::PluginUpdateNeedsApproval { id } if id == "demo"
            ));
        }
        assert_eq!(
            rig.plugins.get("demo").unwrap().program_pins["git"],
            at_a("git").unwrap()
        );

        let updated = rig.update(&next, &rig.digest(&next)).unwrap();
        assert_eq!(updated.record.program_pins["git"], at_b("git").unwrap());
        assert_eq!(
            updated.record.grant_digest,
            approval_digest(&updated.record.grant, &updated.record.program_pins)
        );
    }

    #[test]
    fn an_update_whose_binary_changed_at_the_same_path_needs_approval() {
        let mut rig = Rig::new(at_a);
        rig.install(&rig.package("1.0.0", ""));
        let next = rig.package("1.1.0", "");
        rig.resolving(upgraded_at_a);

        for approve in ["", "stale"] {
            let error = rig.update(&next, approve).map(|_| ()).unwrap_err();
            assert!(matches!(
                refusal(error),
                ErrorInfo::PluginUpdateNeedsApproval { id } if id == "demo"
            ));
        }
        assert_eq!(
            rig.plugins.get("demo").unwrap().program_pins["git"],
            at_a("git").unwrap()
        );

        let updated = rig.update(&next, &rig.digest(&next)).unwrap();
        assert_eq!(
            updated.record.program_pins["git"],
            upgraded_at_a("git").unwrap()
        );
    }

    #[test]
    fn an_update_whose_program_vanished_from_the_path_needs_approval() {
        let mut rig = Rig::new(at_a);
        rig.install(&rig.package("1.0.0", ""));
        rig.resolving(nowhere);

        let error = rig
            .update(&rig.package("1.1.0", ""), "")
            .map(|_| ())
            .unwrap_err();

        assert!(matches!(
            refusal(error),
            ErrorInfo::PluginUpdateNeedsApproval { .. }
        ));
    }

    #[test]
    fn a_program_that_was_not_pinned_stays_unpinned_on_an_unapproved_update() {
        let mut rig = Rig::new(nowhere);
        rig.install(&rig.package("1.0.0", ""));
        rig.resolving(at_a);

        let updated = rig.update(&rig.package("1.1.0", ""), "").unwrap();

        assert!(updated.record.program_pins.is_empty());
    }

    fn approved_by_official(rig: &Rig) {
        let mut record = rig.plugins.get("demo").unwrap();
        record.approved_by = Approver::Official;
        rig.plugins.save(&record).unwrap();
    }

    #[test]
    fn an_update_that_needs_no_approval_keeps_who_approved_the_install() {
        let rig = Rig::new(at_a);
        rig.install(&rig.package("1.0.0", ""));
        approved_by_official(&rig);

        let updated = rig.update(&rig.package("1.1.0", ""), "").unwrap();

        assert_eq!(updated.record.approved_by, Approver::Official);
        assert_eq!(
            rig.plugins.get("demo").unwrap().approved_by,
            Approver::Official
        );
    }

    #[test]
    fn an_update_the_user_approves_is_approved_by_the_user() {
        let mut rig = Rig::new(at_a);
        rig.install(&rig.package("1.0.0", ""));
        approved_by_official(&rig);
        let next = rig.package("1.1.0", "");
        rig.resolving(at_b);

        assert!(rig.update(&next, "").is_err());
        assert_eq!(
            rig.plugins.get("demo").unwrap().approved_by,
            Approver::Official
        );
        let updated = rig.update(&next, &rig.digest(&next)).unwrap();

        assert_eq!(updated.record.approved_by, Approver::User);
    }

    #[test]
    fn a_newly_declared_program_the_user_approves_is_approved_by_the_user() {
        let rig = Rig::new(at_a);
        rig.install(&rig.package("1.0.0", ""));
        approved_by_official(&rig);
        let widened = rig.package_granting(
            "1.1.0",
            "\"git\", \"curl\"",
            "\n[[programs]]\nid = \"curl\"\ncommand = \"curl\"\nargs = [\"**\"]\n",
        );

        assert!(rig.update(&widened, "").is_err());
        let updated = rig.update(&widened, &rig.digest(&widened)).unwrap();

        assert_eq!(updated.record.approved_by, Approver::User);
    }

    const SHAPES: &str = r#"
id = "demo"
version = "1.0.0"
api = "^1.0.0"
wasm = "plugin.wasm"

[permissions]
"process:spawn" = { programs = ["git", "buildtools", "tool"] }

[[programs]]
id = "git"
command = "git"
args = ["**"]
requires = { name = "Git", url = "https://git-scm.com/downloads" }

[[programs]]
id = "buildtools"
java = { min = 17, max = 21 }

[[programs.invocations]]
name = "build"
args = ["-jar", "${data}/BuildTools.jar", "--rev", "*"]

[[programs.invocations]]
name = "version"
args = ["-version"]

[[programs]]
id = "tool"
command = "bin/tool"
args = ["--serve"]
env = ["TOOL_TOKEN"]
sha256 = { x86_64-unknown-linux-gnu = "0000000000000000000000000000000000000000000000000000000000000000" }

[[programs]]
id = "game"
java = "profile"
args = ["-version"]
"#;

    #[test]
    fn an_invalid_manifest_reports_every_program_error_at_once() {
        let broken = SHAPES
            .replace("command = \"git\"", "command = \"sh\"")
            .replace("java = \"profile\"", "java = { min = 21, max = 17 }");
        let error = parse_manifest(&broken).unwrap_err().to_string();

        assert!(error.contains("program `git`"), "{error}");
        assert!(error.contains("program `game`"), "{error}");
    }
}
