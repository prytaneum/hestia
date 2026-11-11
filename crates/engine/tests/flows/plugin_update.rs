//! Updating an installed plugin in place: what is swapped, what is kept, when
//! the user's approval is needed and what a failed load puts back.

use std::path::{Path, PathBuf};

use engine::{Origin, PluginRecord, Plugins, Staged, UpdateOutcome, UpdateRequest};
use proto::plugin::{Permission, PluginSource};

use super::*;

const SIGNER: &str = "KEY";

fn manifest(version: &str, permissions: &[Permission]) -> String {
    let permissions: Vec<String> = permissions.iter().map(|p| format!("\"{p}\"")).collect();
    format!(
        "id = \"tunnel\"\nversion = \"{version}\"\napi = \"^{}\"\nwasm = \"plugin.wasm\"\n\
         permissions = [{}]\n",
        proto::plugin::API_VERSION,
        permissions.join(", ")
    )
}

fn package(root: &Path, version: &str, permissions: &[Permission]) -> PathBuf {
    let dir = root.join(version);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("plugin.wasm"), format!("code {version}")).unwrap();
    std::fs::write(dir.join("plugin.toml"), manifest(version, permissions)).unwrap();
    dir
}

fn staged(path: PathBuf, signer: Option<&str>) -> Staged {
    Staged {
        path,
        url: "https://repo.example/tunnel.hplugin".into(),
        origin: signer.map(|signer| Origin {
            signer: signer.into(),
            repository: "https://repo.example/plugins.json".into(),
        }),
        warnings: Vec::new(),
    }
}

fn install(plugins: &Plugins, root: &Path, version: &str) -> PluginRecord {
    let staged = staged(package(root, version, &[]), Some(SIGNER));
    let digest = plugins.inspect(&staged.path).unwrap().grant_digest;
    plugins
        .install_downloaded(&staged, &digest, &Cancel::new(), &|_| {}, |_| Ok(()))
        .expect("install")
}

async fn update(
    plugins: &Plugins,
    staged: &Staged,
    approve: &str,
    loads: bool,
) -> anyhow::Result<UpdateOutcome> {
    plugins
        .apply_update(
            UpdateRequest {
                id: "tunnel",
                staged,
                approve,
            },
            &Cancel::new(),
            &|_| {},
            |_| Ok(()),
            |_| async move {
                if loads {
                    Ok(())
                } else {
                    anyhow::bail!("init trapped")
                }
            },
        )
        .await
}

fn wasm(plugins: &Plugins) -> Vec<u8> {
    plugins.read_package("tunnel").unwrap().wasm.unwrap()
}

fn refusal(error: anyhow::Error) -> ErrorInfo {
    engine::error_info(error)
}

struct Rig {
    _home: tempfile::TempDir,
    packages: tempfile::TempDir,
    engine: engine::Engine,
}

impl Rig {
    fn new() -> Rig {
        let home = tempfile::tempdir().unwrap();
        let engine = engine::Engine::new(Some(home.path()));
        Rig {
            _home: home,
            packages: tempfile::tempdir().unwrap(),
            engine,
        }
    }

    fn plugins(&self) -> &Plugins {
        self.engine.plugins()
    }

    fn package(&self, version: &str, permissions: &[Permission]) -> PathBuf {
        package(self.packages.path(), version, permissions)
    }
}

#[tokio::test]
async fn an_update_swaps_the_package_and_keeps_the_old_one_and_the_data() {
    let rig = Rig::new();
    install(rig.plugins(), rig.packages.path(), "1.0.0");
    let kept = rig.plugins().data_dir("tunnel").unwrap().join("state.json");
    std::fs::write(&kept, "{}").unwrap();

    let next = staged(rig.package("2.0.0", &[]), Some(SIGNER));
    let outcome = update(rig.plugins(), &next, "", true)
        .await
        .expect("update");

    let UpdateOutcome::Applied { record, from } = outcome else {
        panic!("the update should apply");
    };
    assert_eq!(from, "1.0.0");
    assert_eq!(record.signer.as_deref(), Some(SIGNER));
    assert_eq!(record.source, PluginSource::Repo);
    assert_eq!(wasm(rig.plugins()), b"code 2.0.0");
    let previous = rig
        .plugins()
        .plugin_dir("tunnel")
        .unwrap()
        .join("package.prev");
    assert_eq!(
        std::fs::read(previous.join("plugin.wasm")).unwrap(),
        b"code 1.0.0"
    );
    assert!(kept.exists());
}

#[tokio::test]
async fn only_one_previous_version_is_kept() {
    let rig = Rig::new();
    install(rig.plugins(), rig.packages.path(), "1.0.0");
    for version in ["2.0.0", "3.0.0"] {
        let next = staged(rig.package(version, &[]), Some(SIGNER));
        update(rig.plugins(), &next, "", true)
            .await
            .expect("update");
    }

    let dir = rig.plugins().plugin_dir("tunnel").unwrap();
    assert_eq!(
        std::fs::read(dir.join("package.prev/plugin.wasm")).unwrap(),
        b"code 2.0.0"
    );
    let rolled = rig.plugins().rollback("tunnel").expect("rollback");
    assert_eq!(rolled.id, "tunnel");
    assert_eq!(wasm(rig.plugins()), b"code 2.0.0");
    assert!(rig.plugins().rollback("tunnel").is_err());
}

#[tokio::test]
async fn a_version_that_fails_to_load_is_rolled_back() {
    let rig = Rig::new();
    let installed = install(rig.plugins(), rig.packages.path(), "1.0.0");

    let next = staged(rig.package("2.0.0", &[]), Some(SIGNER));
    let outcome = update(rig.plugins(), &next, "", false)
        .await
        .expect("a failed load is reported, not raised");

    let UpdateOutcome::RolledBack {
        record,
        version,
        error,
    } = outcome
    else {
        panic!("the update should be rolled back");
    };
    assert_eq!(version, "2.0.0");
    assert_eq!(error, "init trapped");
    assert_eq!(record.grant_digest, installed.grant_digest);
    assert_eq!(wasm(rig.plugins()), b"code 1.0.0");
    assert_eq!(
        rig.plugins().get("tunnel").unwrap().wasm_sha256,
        installed.wasm_sha256
    );
    let dir = rig.plugins().plugin_dir("tunnel").unwrap();
    assert!(!dir.join("package.prev").exists());
    assert!(!dir.join("package.failed").exists());
}

#[tokio::test]
async fn a_widened_grant_waits_for_approval_and_changes_nothing() {
    let rig = Rig::new();
    install(rig.plugins(), rig.packages.path(), "1.0.0");
    let widened = staged(rig.package("3.0.0", &[Permission::EntryRead]), Some(SIGNER));
    let digest = rig.plugins().inspect(&widened.path).unwrap().grant_digest;

    for approve in ["", "stale"] {
        let refused = update(rig.plugins(), &widened, approve, true)
            .await
            .map(|_| ())
            .expect_err("not approved");
        assert!(
            matches!(refusal(refused), ErrorInfo::PluginUpdateNeedsApproval { id } if id == "tunnel")
        );
    }
    assert_eq!(wasm(rig.plugins()), b"code 1.0.0");
    let dir = rig.plugins().plugin_dir("tunnel").unwrap();
    assert!(!dir.join("package.prev").exists());

    update(rig.plugins(), &widened, &digest, true)
        .await
        .expect("approved");
    assert_eq!(wasm(rig.plugins()), b"code 3.0.0");
    assert!(rig
        .plugins()
        .get("tunnel")
        .unwrap()
        .grant
        .permissions
        .contains(&Permission::EntryRead));
}

#[tokio::test]
async fn a_narrowed_grant_applies_without_approval() {
    let rig = Rig::new();
    let wide = staged(rig.package("1.0.0", &[Permission::EntryRead]), Some(SIGNER));
    let digest = rig.plugins().inspect(&wide.path).unwrap().grant_digest;
    rig.plugins()
        .install_downloaded(&wide, &digest, &Cancel::new(), &|_| {}, |_| Ok(()))
        .expect("install");

    let narrow = staged(rig.package("2.0.0", &[]), Some(SIGNER));
    update(rig.plugins(), &narrow, "", true)
        .await
        .expect("narrower applies");

    assert!(rig
        .plugins()
        .get("tunnel")
        .unwrap()
        .grant
        .permissions
        .is_empty());
}

#[tokio::test]
async fn an_update_must_be_newer_and_signed_by_the_installed_key() {
    let rig = Rig::new();
    install(rig.plugins(), rig.packages.path(), "2.0.0");

    for (version, signer) in [("1.0.0", Some(SIGNER)), ("2.0.0", Some(SIGNER))] {
        let older = staged(rig.package(version, &[]), signer);
        let refused = update(rig.plugins(), &older, "", true)
            .await
            .map(|_| ())
            .expect_err("not newer");
        assert!(matches!(refusal(refused), ErrorInfo::PluginInvalid { .. }));
    }
    for signer in [Some("OTHER"), None] {
        let foreign = staged(rig.package("3.0.0", &[]), signer);
        let refused = update(rig.plugins(), &foreign, "", true)
            .await
            .map(|_| ())
            .expect_err("wrong signer");
        assert!(matches!(refusal(refused), ErrorInfo::PluginInvalid { .. }));
    }
    assert_eq!(wasm(rig.plugins()), b"code 2.0.0");
}

#[tokio::test]
async fn a_swap_interrupted_between_its_renames_is_put_back_at_startup() {
    let rig = Rig::new();
    install(rig.plugins(), rig.packages.path(), "1.0.0");
    let next = staged(rig.package("2.0.0", &[]), Some(SIGNER));
    update(rig.plugins(), &next, "", true)
        .await
        .expect("update");
    let dir = rig.plugins().plugin_dir("tunnel").unwrap();
    std::fs::remove_dir_all(dir.join("package")).unwrap();

    rig.engine.recover();

    assert_eq!(wasm(rig.plugins()), b"code 1.0.0");
    assert!(!dir.join("package.prev").exists());
}
