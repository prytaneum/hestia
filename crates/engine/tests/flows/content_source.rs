//! A content source contributed by a plugin, from the `static-index` example:
//! registered on enable, installed from, and kept inside the plugin's hosts.

use std::sync::Arc;

use engine::{LoadedPlugin, LocalFile, PluginRecord, PluginRuntime, PluginSettings};
use proto::content::{RequiredSetting, VersionQuery};
use proto::plugin::{Manifest, Permission, PermissionScope, ScopeKind, Setting, SettingKind};
use sha2::{Digest, Sha256};

use super::launch_hooks::NoHost;
use super::*;

const SOURCE: &str = "static-index.index";

pub(super) fn manifest() -> Manifest {
    let manifest: Manifest = toml::from_str(fixture::index::MANIFEST).expect("manifest");
    manifest.validate().expect("a valid manifest");
    manifest
}

const SETTINGS_FILE: &str = "static-index-settings.json";

fn declared() -> Vec<Setting> {
    vec![Setting {
        key: "api-key".into(),
        kind: SettingKind::Secret,
        ..Setting::default()
    }]
}

pub(super) fn load(
    home: &std::path::Path,
    files: &fixture::Files,
    hosts: &[&str],
) -> (Arc<LoadedPlugin>, Manifest) {
    let manifest = manifest();
    let wasm = std::fs::read(fixture::index::component()).expect("static-index component");
    let runtime = PluginRuntime::new(
        Arc::new(NoHost),
        fixture::plugin_cache(),
        Arc::new(engine::ProcessSupervisor::new(home.join("processes"))),
    )
    .expect("runtime");
    let mut grant = manifest.grant();
    grant.permissions.insert(
        Permission::NetworkConnect,
        PermissionScope::of(ScopeKind::Hosts, hosts.iter().copied()),
    );
    let record = PluginRecord {
        id: manifest.id.clone(),
        wasm_sha256: Some(
            Sha256::digest(&wasm)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
        ),
        grant,
        ..PluginRecord::default()
    };
    let data = home.join("static-index-data");
    std::fs::create_dir_all(&data).expect("data dir");
    std::fs::write(
        data.join("base-url.json"),
        serde_json::to_string(&files.url("files")).unwrap(),
    )
    .expect("base url");
    let settings = PluginSettings::new(home.join(SETTINGS_FILE), declared());
    let plugin = runtime.load(&record, &wasm, data, settings).expect("load");
    (Arc::new(plugin), manifest)
}

async fn rig(hosts: &[&str]) -> (engine::Engine, String, tempfile::TempDir) {
    rig_requiring(hosts, None).await
}

async fn rig_requiring(
    hosts: &[&str],
    requires: Option<&str>,
) -> (engine::Engine, String, tempfile::TempDir) {
    let home = tempfile::tempdir().expect("temp home");
    let files = fixture::Files::serve().await;
    let engine = fixture::engine(
        home.path(),
        vec![fixture::Flavor::resolving("fabric")],
        vec![],
    );
    let id = engine
        .create_instance("modded", "fabric", "1.21", None, &[])
        .await
        .expect("create the instance")
        .id;
    let (plugin, manifest) = load(home.path(), &files, hosts);
    if let Some(key) = requires {
        std::fs::write(
            home.path().join("static-index-data/requires-setting.json"),
            serde_json::to_string(key).unwrap(),
        )
        .expect("requires-setting");
    }
    engine::Contributions::connect(&plugin, &manifest, false)
        .await
        .register(&engine, &manifest.id);
    (engine, id, home)
}

#[tokio::test]
async fn a_mod_installs_from_a_source_a_plugin_contributes() {
    let (engine, id, home) = rig(&["127.0.0.1"]).await;
    let (cancel, report) = (Cancel::new(), |_: &ProvisionProgress| {});
    let mut spec = add(&["cobble-tweaks"]);
    spec.source = SOURCE.into();

    let done = engine
        .add_entry_content(
            EntryRef::Instance(&id),
            &spec,
            &NO_CHAIN,
            &job(&cancel, &report),
        )
        .await
        .expect("the batch runs");

    assert!(done.failures.is_empty(), "{:?}", done.failures);
    let [item] = done.items.as_slice() else {
        panic!("one item expected: {:?}", done.items);
    };
    assert_eq!(item.source, SOURCE);
    assert_eq!(item.project_id, "cobble-tweaks");
    assert!(home
        .path()
        .join("instances/modded/mods")
        .join(&item.filename)
        .exists());
}

#[tokio::test]
async fn the_source_is_offered_while_the_plugin_is_enabled_and_gone_after() {
    let (engine, _id, _home) = rig(&["127.0.0.1"]).await;
    let offered = |engine: &engine::Engine| {
        engine
            .content()
            .sources()
            .into_iter()
            .find(|source| source.id == SOURCE)
    };

    let source = offered(&engine).expect("registered on enable");
    assert_eq!(source.name, "Static index");
    assert_eq!(source.kinds, [ContentKind::Mod]);

    assert_eq!(engine.content().unregister("static-index"), 1);
    assert!(offered(&engine).is_none(), "unregistered on disable");
}

#[tokio::test]
async fn a_file_outside_the_plugins_hosts_is_never_offered_for_download() {
    let (engine, _id, _home) = rig(&["index.example.com"]).await;

    let versions = engine
        .content()
        .versions(&VersionQuery {
            source: SOURCE.into(),
            project: "cobble-tweaks".into(),
            ..VersionQuery::default()
        })
        .await
        .expect("the lookup itself works");

    assert!(versions.is_empty(), "{versions:?}");
}

#[tokio::test]
async fn a_guest_failure_names_the_plugin() {
    let (engine, _id, _home) = rig(&["127.0.0.1"]).await;

    let error = engine
        .content()
        .project(SOURCE, "no-such-project", None)
        .await
        .expect_err("the guest has no such project");

    assert!(
        matches!(
            engine::error_info(error),
            ErrorInfo::PluginCallFailed { id, .. } if id == "static-index"
        ),
        "the plugin is named, not a raw message"
    );
}

fn listed(engine: &engine::Engine) -> Option<proto::content::ContentSource> {
    engine
        .content()
        .sources()
        .into_iter()
        .find(|source| source.id == SOURCE)
}

#[tokio::test]
async fn a_source_waiting_on_a_setting_says_which_until_it_is_set() {
    let (engine, _id, home) = rig_requiring(&["127.0.0.1"], Some("api-key")).await;

    let waiting = listed(&engine).expect("listed, so a front-end can say why");
    assert_eq!(
        waiting.requires_setting,
        Some(RequiredSetting {
            plugin: Some("static-index".into()),
            key: "api-key".into(),
        })
    );
    let refused = engine
        .content()
        .project(SOURCE, "cobble-tweaks", None)
        .await
        .expect_err("an unconfigured source refuses calls");
    assert!(matches!(
        engine::error_info(refused),
        ErrorInfo::ContentSourceUnavailable { .. }
    ));

    let settings = PluginSettings::new(home.path().join(SETTINGS_FILE), declared());
    settings.set("api-key", serde_json::json!("   ")).unwrap();
    assert!(
        listed(&engine).unwrap().requires_setting.is_some(),
        "a blank value is not a setting"
    );

    settings
        .set("api-key", serde_json::json!("secret"))
        .unwrap();
    assert!(listed(&engine).unwrap().requires_setting.is_none());
    engine
        .content()
        .project(SOURCE, "cobble-tweaks", None)
        .await
        .expect("serves once the setting is set");
}

#[tokio::test]
async fn a_source_requiring_an_undeclared_setting_is_not_registered() {
    let (engine, _id, _home) = rig_requiring(&["127.0.0.1"], Some("no-such-setting")).await;

    assert!(listed(&engine).is_none());
}

fn local(dir: &std::path::Path, name: &str, text: &str) -> LocalFile {
    let path = dir.join(name);
    std::fs::write(&path, text).unwrap();
    LocalFile {
        path,
        sha1: format!("sha1-of-{name}"),
    }
}

#[tokio::test]
async fn identify_reads_only_the_files_it_is_asked_about_and_only_for_that_call() {
    let (engine, _id, home) = rig(&["127.0.0.1"]).await;
    let files = tempfile::tempdir().expect("files");
    let known = local(files.path(), "known.jar", "cobble-tweaks-1.0.0.jar");
    let unknown = local(files.path(), "unknown.jar", "something else");
    local(files.path(), "bystander.jar", "cobble-tweaks-1.0.0.jar");

    let found = engine
        .content()
        .identify(&[known.clone(), unknown.clone()])
        .await;

    assert_eq!(found.len(), 1, "{found:?}");
    let identified = &found[&known.sha1][0];
    assert_eq!(identified.version.id, "cobble-tweaks-1");
    assert_eq!(
        identified.version.name, "writable=false escaped=false listed=2",
        "read-only, no way out, and only the two files lent"
    );
    assert!(
        std::fs::read_dir(home.path()).unwrap().all(|entry| !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".identify")),
        "the staging directory is gone after the call"
    );
    assert_eq!(
        std::fs::read_to_string(&known.path).unwrap(),
        "cobble-tweaks-1.0.0.jar",
        "the host's file is untouched"
    );
}

const SHA1: &str = "32a22d349740b9065493b697ae61279f4f85aea2";

#[tokio::test]
async fn a_pack_manifest_resolves_through_the_source_that_owns_it() {
    let (engine, _id, _home) = rig(&["127.0.0.1"]).await;

    let pack = engine
        .content()
        .resolve_manifest(
            SOURCE,
            &serde_json::json!({
                "name": "Cozy",
                "files": [
                    { "path": "mods/a.jar", "url": "a.jar", "sha1": SHA1 },
                    { "path": "mods/withheld.jar", "url": "" },
                ],
            }),
        )
        .await
        .expect("resolves");

    assert_eq!(pack.source, SOURCE);
    assert_eq!(pack.name, "Cozy");
    assert!(pack.files.iter().all(|file| file.source == SOURCE));
    assert!(pack.files[0].artifact.url.ends_with("/files/a.jar"));
    assert!(
        pack.files[1].artifact.url.is_empty(),
        "a withheld file is listed, and the download names it"
    );
}

#[tokio::test]
async fn a_pack_file_the_rule_refuses_refuses_the_pack() {
    let (engine, _id, _home) = rig(&["127.0.0.1"]).await;
    let engine = &engine;
    let resolve = |files: serde_json::Value| async move {
        engine
            .content()
            .resolve_manifest(SOURCE, &serde_json::json!({ "files": files }))
            .await
    };

    let escaping =
        resolve(serde_json::json!([{ "path": "../outside.jar", "url": "a.jar", "sha1": SHA1 }]))
            .await
            .expect_err("a path out of the entry");
    assert!(matches!(
        engine::error_info(escaping),
        ErrorInfo::ModpackInvalid { .. }
    ));

    for (name, file) in [
        (
            "a host the plugin was not granted",
            serde_json::json!({ "path": "mods/a.jar", "url": "https://evil.example.org/a.jar", "sha1": SHA1 }),
        ),
        (
            "no checksum",
            serde_json::json!({ "path": "mods/a.jar", "url": "a.jar" }),
        ),
        (
            "a malformed checksum",
            serde_json::json!({ "path": "mods/a.jar", "url": "a.jar", "sha1": "abc" }),
        ),
    ] {
        let refused = resolve(serde_json::json!([file])).await.expect_err(name);
        assert!(
            matches!(
                engine::error_info(refused),
                ErrorInfo::PluginCallFailed { .. }
            ),
            "{name}"
        );
    }
}
