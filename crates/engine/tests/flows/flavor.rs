//! A flavor contributed by a plugin, from the `replay-flavor` fixture:
//! registered on enable, provisioned from, and refused when its profile names
//! anything the plugin was not granted.

use std::sync::Arc;

use engine::{LoadedPlugin, PluginRecord, PluginRuntime};
use proto::plugin::Manifest;
use sha2::{Digest, Sha256};

use super::launch_hooks::NoHost;
use super::*;

const FLAVOR: &str = "replay-flavor.replay";

fn load(home: &std::path::Path, files: &fixture::Files, hosts: &[&str]) -> Arc<LoadedPlugin> {
    let manifest: Manifest = toml::from_str(fixture::replay::MANIFEST).expect("manifest");
    manifest.validate().expect("a valid manifest");
    let wasm = std::fs::read(fixture::replay::component()).expect("replay-flavor component");
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
    let data = home.join("replay-flavor-data");
    std::fs::create_dir_all(&data).expect("data dir");
    std::fs::write(
        data.join("base-url.json"),
        serde_json::to_string(&files.url("").trim_end_matches('/')).unwrap(),
    )
    .expect("base url");
    let settings =
        engine::PluginSettings::new(home.join("replay-flavor-settings.json"), Vec::new());
    Arc::new(runtime.load(&record, &wasm, data, settings).expect("load"))
}

async fn rig(hosts: &[&str]) -> (engine::Engine, tempfile::TempDir) {
    let home = tempfile::tempdir().expect("temp home");
    let files = fixture::Files::serve().await;
    let engine = fixture::engine(home.path(), vec![], vec![]);
    fixture::java_runtime(home.path(), 21);
    let manifest: Manifest = toml::from_str(fixture::replay::MANIFEST).expect("manifest");
    let plugin = load(home.path(), &files, hosts);
    let flavors = engine::connect_flavors(&plugin, &manifest).await;
    engine::register_flavors(engine.minecraft(), &manifest.id, false, flavors);
    (engine, home)
}

fn server(version: &str) -> ServerCreateSpec {
    ServerCreateSpec {
        flavor: FLAVOR.into(),
        version: version.into(),
        ..spec(FLAVOR)
    }
}

#[tokio::test]
async fn a_server_provisions_from_a_flavor_a_plugin_contributes() {
    let (engine, _home) = rig(&["127.0.0.1"]).await;
    let (cancel, report) = (Cancel::new(), |_: &ProvisionProgress| {});

    let record = engine
        .provision_server(server("1.21"), &job(&cancel, &report))
        .await
        .expect("provision the server")
        .0;

    assert_eq!(record.profile.flavor, FLAVOR);
    assert!(engine
        .servers()
        .data_dir(&record)
        .join("server.jar")
        .is_file());
}

#[tokio::test]
async fn the_flavor_is_offered_while_the_plugin_is_enabled_and_gone_after() {
    let (engine, _home) = rig(&["127.0.0.1"]).await;
    let offered = |engine: &engine::Engine| {
        engine
            .minecraft()
            .flavors(engine::Side::Client)
            .into_iter()
            .find(|flavor| flavor.id == FLAVOR)
    };

    assert_eq!(
        offered(&engine).expect("registered on enable").name,
        "Replay"
    );
    assert_eq!(engine.minecraft().unregister("replay-flavor"), 1);
    assert!(offered(&engine).is_none(), "unregistered on disable");
}

#[tokio::test]
async fn a_client_profile_resolves_with_the_hosts_own_ids() {
    let (engine, home) = rig(&["127.0.0.1"]).await;

    let profile = engine
        .minecraft()
        .resolve_instance(FLAVOR, "1.21", None, home.path(), None)
        .await
        .expect("the profile is vetted and returned");

    assert_eq!(profile.flavor, FLAVOR);
    assert_eq!(profile.game_version, "1.21");
}

#[tokio::test]
async fn a_profile_with_one_artifact_off_the_plugins_hosts_is_refused_whole() {
    let (engine, home) = rig(&["127.0.0.1"]).await;
    let (cancel, report) = (Cancel::new(), |_: &ProvisionProgress| {});

    engine
        .provision_server(server("foreign"), &job(&cancel, &report))
        .await
        .expect_err("the jar is on a host the plugin was not granted");
    let error = engine
        .minecraft()
        .resolve_instance(FLAVOR, "foreign", None, home.path(), None)
        .await
        .expect_err("so is the client");

    assert!(
        matches!(
            engine::error_info(error),
            ErrorInfo::PluginCallFailed { id, .. } if id == "replay-flavor"
        ),
        "the plugin is named, not a raw message"
    );
    assert!(engine.servers().list().is_empty());
}

#[tokio::test]
async fn a_profile_without_a_sha1_is_refused() {
    let (engine, home) = rig(&["127.0.0.1"]).await;

    engine
        .minecraft()
        .resolve_server(FLAVOR, "unsigned", None, home.path(), None)
        .await
        .expect_err("an artifact that cannot be verified is not downloaded");
}
