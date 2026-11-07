//! Dev plugins: a folder run in place, so a rebuilt component is picked up
//! without packing and reinstalling.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{bail, Context, Result};
use proto::error::ErrorInfo;
use proto::plugin::PluginSource;

use super::install::{digest_package, invalid, Source};
use super::program::pin_programs;
use super::store::{PluginRecord, Plugins, MANIFEST, PACKAGE};

impl Plugins {
    /// Blocking. Like [`Self::install`] for a folder, except the folder is not
    /// copied: the record points at it, and the hashes it holds are refreshed by
    /// [`Self::refresh_dev`]. A folder inside the plugins directory is refused,
    /// since that is where installed packages live.
    pub fn install_dev(
        &self,
        path: &Path,
        approve: &str,
        validate: impl FnOnce(&[u8]) -> Result<()>,
    ) -> Result<PluginRecord> {
        let root = std::fs::canonicalize(path)
            .map_err(|error| invalid(format!("cannot resolve {}: {error}", path.display())))?;
        if !root.is_dir() {
            return Err(invalid(format!("{} is not a folder", root.display())));
        }
        let plugins_dir = std::fs::canonicalize(&self.dir).unwrap_or_else(|_| self.dir.clone());
        if root.starts_with(&plugins_dir) {
            return Err(invalid(
                "a dev folder cannot be inside the plugins directory",
            ));
        }
        let manifest = Source::Folder(root.clone())
            .read_manifest()
            .map_err(|error| invalid(format!("{error:#}")))?;
        let grant = manifest.grant();
        let grant_digest = grant.digest();
        if grant_digest != approve {
            tracing::warn!(plugin = %manifest.id, "dev install refused: the grant is not the one approved");
            bail!(ErrorInfo::PluginApprovalStale);
        }
        let (wasm_sha256, theme_sha256) = digest_package(&manifest, &root, validate)?;

        let program_paths = pin_programs(&manifest.id, &manifest);
        let id = manifest.id;
        let installed = self.get(&id);
        let copied = self.plugin_dir(&id)?.join(PACKAGE);
        {
            let _writes = self.writes.lock().unwrap();
            if copied.exists() {
                std::fs::remove_dir_all(&copied).context("cannot remove the installed package")?;
            }
        }
        let record = PluginRecord {
            id: id.clone(),
            source: PluginSource::Dev,
            dev_path: Some(root),
            wasm_sha256,
            theme_sha256,
            program_paths,
            grant,
            grant_digest,
            enabled: installed.is_none_or(|record| record.enabled),
            installed_unix: common::now_unix(),
            ..PluginRecord::default()
        };
        self.save(&record)?;
        tracing::info!(plugin = %id, "dev plugin installed");
        Ok(record)
    }

    /// The newest modification time among the dev folder's manifest, component
    /// and themes; `None` for a plugin that is not a dev one or whose files
    /// cannot be read right now.
    pub fn dev_stamp(&self, id: &str) -> Option<SystemTime> {
        let root = self.dev_root(id)?;
        let manifest = Source::Folder(root.clone()).read_manifest().ok();
        let watched = manifest.iter().flat_map(|manifest| {
            manifest
                .wasm
                .iter()
                .chain(manifest.contributes.themes.iter().map(|theme| &theme.file))
                .map(|relative| root.join(relative))
        });
        std::iter::once(root.join(MANIFEST))
            .chain(watched)
            .map(|file| std::fs::metadata(file).and_then(|meta| meta.modified()))
            .collect::<std::io::Result<Vec<_>>>()
            .ok()?
            .into_iter()
            .max()
    }

    /// Blocking. Re-reads a dev folder and refreshes the recorded hashes. The
    /// approved grant is left alone, so a manifest that now asks for more is
    /// caught by the caller's grant comparison when it loads.
    pub fn refresh_dev(&self, id: &str) -> Result<PluginRecord> {
        let mut record = self
            .get(id)
            .filter(|record| record.source == PluginSource::Dev)
            .with_context(|| format!("`{id}` is not a dev plugin"))?;
        let root = record
            .dev_path
            .clone()
            .context("the dev folder is not recorded")?;
        let manifest = Source::Folder(root.clone())
            .read_manifest()
            .map_err(|error| invalid(format!("{error:#}")))?;
        if manifest.id != id {
            return Err(invalid(format!(
                "plugin.toml now declares `{}`",
                manifest.id
            )));
        }
        let (wasm_sha256, theme_sha256) = digest_package(&manifest, &root, |_| Ok(()))?;
        record.wasm_sha256 = wasm_sha256;
        record.theme_sha256 = theme_sha256;
        self.save(&record)?;
        tracing::info!(plugin = %id, "dev plugin refreshed from its folder");
        Ok(record)
    }

    fn dev_root(&self, id: &str) -> Option<PathBuf> {
        self.get(id)
            .filter(|record| record.source == PluginSource::Dev)?
            .dev_path
    }
}

#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::time::Duration;

    use super::*;

    const MANIFEST_TEXT: &str = "id = \"demo\"\nname = \"Demo\"\nversion = \"0.1.0\"\napi = \"^1.0.0\"\nwasm = \"plugin.wasm\"\n";

    struct Fixture {
        _home: tempfile::TempDir,
        plugins: Plugins,
        folder: tempfile::TempDir,
    }

    fn fixture() -> Fixture {
        let home = tempfile::tempdir().unwrap();
        let folder = tempfile::tempdir().unwrap();
        std::fs::write(folder.path().join(MANIFEST), MANIFEST_TEXT).unwrap();
        std::fs::write(folder.path().join("plugin.wasm"), b"one").unwrap();
        let plugins = Plugins::new(home.path().join("plugins"));
        Fixture {
            _home: home,
            plugins,
            folder,
        }
    }

    fn digest_of(manifest: &str) -> String {
        toml::from_str::<proto::plugin::Manifest>(manifest)
            .unwrap()
            .grant()
            .digest()
    }

    fn rewrite(folder: &Path, name: &str, contents: &str, ahead: u64) {
        let path = folder.join(name);
        std::fs::write(&path, contents).unwrap();
        File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(SystemTime::now() + Duration::from_secs(ahead))
            .unwrap();
    }

    fn install(fixture: &Fixture) -> PluginRecord {
        fixture
            .plugins
            .install_dev(fixture.folder.path(), &digest_of(MANIFEST_TEXT), |_| Ok(()))
            .unwrap()
    }

    #[test]
    fn a_dev_install_reads_the_folder_in_place() {
        let fixture = fixture();
        let record = install(&fixture);

        assert_eq!(record.source, PluginSource::Dev);
        let package = fixture.plugins.read_package("demo").unwrap();
        assert_eq!(package.wasm.as_deref(), Some(&b"one"[..]));
        assert_eq!(
            fixture.plugins.package_dir("demo").unwrap(),
            std::fs::canonicalize(fixture.folder.path()).unwrap()
        );
        let copied = fixture.plugins.plugin_dir("demo").unwrap().join(PACKAGE);
        assert!(!copied.exists());
    }

    #[test]
    fn a_changed_component_moves_the_stamp_and_refreshes_the_hash() {
        let fixture = fixture();
        let before = install(&fixture);
        let stamp = fixture.plugins.dev_stamp("demo").unwrap();

        rewrite(fixture.folder.path(), "plugin.wasm", "two", 60);

        assert!(fixture.plugins.dev_stamp("demo").unwrap() > stamp);
        let after = fixture.plugins.refresh_dev("demo").unwrap();
        assert_ne!(after.wasm_sha256, before.wasm_sha256);
        assert_eq!(
            fixture.plugins.get("demo").unwrap().wasm_sha256,
            after.wasm_sha256
        );
    }

    #[test]
    fn a_widened_manifest_keeps_the_approved_grant_and_reads_as_stale() {
        let fixture = fixture();
        let approved = install(&fixture).grant_digest;

        let widened = format!("{MANIFEST_TEXT}permissions = [\"entry:read\"]\n");
        rewrite(fixture.folder.path(), MANIFEST, &widened, 60);
        let record = fixture.plugins.refresh_dev("demo").unwrap();

        assert_eq!(record.grant_digest, approved);
        let manifest = fixture.plugins.read_manifest("demo").unwrap();
        assert_ne!(manifest.grant().digest(), record.grant_digest);
    }

    #[test]
    fn a_dev_folder_inside_the_plugins_directory_is_refused() {
        let fixture = fixture();
        let inside = fixture.plugins.plugin_dir("other").unwrap().join(PACKAGE);
        std::fs::create_dir_all(&inside).unwrap();
        std::fs::write(inside.join(MANIFEST), MANIFEST_TEXT).unwrap();
        std::fs::write(inside.join("plugin.wasm"), b"one").unwrap();

        let error = fixture
            .plugins
            .install_dev(&inside, &digest_of(MANIFEST_TEXT), |_| Ok(()))
            .unwrap_err();

        assert!(format!("{error:#}").contains("inside the plugins directory"));
    }
}
