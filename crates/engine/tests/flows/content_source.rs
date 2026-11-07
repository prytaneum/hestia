//! A content source contributed by a plugin, from the `static-index` example:
//! registered on enable, installed from, and kept inside the plugin's hosts.

use std::sync::Arc;

use engine::{LoadedPlugin, PluginRecord, PluginRuntime};
use proto::content::VersionQuery;
use proto::plugin::Manifest;
use sha2::{Digest, Sha256};

use super::launch_hooks::NoHost;
use super::*;

const SOURCE: &str = "static-index.index";

fn manifest() -> Manifest {
    let manifest: Manifest = toml::from_str(fixture::index::MANIFEST).expect("manifest");
    manifest.validate().expect("a valid manifest");
    manifest
}

fn load(
    home: &std::path::Path,
    files: &fixture::Files,
    hosts: &[&str],
) -> (Arc<LoadedPlugin>, Manifest) {
    let manifest = manifest();
    let wasm = std::fs::read(fixture::index::component()).expect("static-index component");
    let runtime = PluginRuntime::new(
        Arc::new(NoHost),
        home.join("plugin-cache"),
        Arc::new(engine::ProcessSupervisor::new(home.join("processes"))),
        Arc::new(engine::Presence::over_discord()),
    )
    .expect("runtime");
    let mut grant = manifest.grant();
    grant.network = hosts.iter().map(|host| host.to_string()).collect();
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
    let settings = engine::PluginSettings::new(home.join("static-index-settings.json"), Vec::new());
    let plugin = runtime.load(&record, &wasm, data, settings).expect("load");
    (Arc::new(plugin), manifest)
}

async fn rig(hosts: &[&str]) -> (engine::Engine, String, tempfile::TempDir) {
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
    let sources = engine::connect_sources(&plugin, &manifest).await;
    engine::register_sources(engine.content(), &manifest.id, false, sources);
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
