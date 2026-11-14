//! A Java distribution contributed by a plugin, from the `replay-java` fixture:
//! registered on enable, and its builds accepted only from the plugin's hosts.

use std::sync::Arc;

use engine::{PluginRecord, PluginRuntime, PluginSettings};
use proto::plugin::Manifest;
use sha2::{Digest, Sha256};

use super::launch_hooks::NoHost;
use super::*;

const DISTRIBUTION: &str = "replay-java.replay";

async fn rig() -> (engine::Engine, tempfile::TempDir) {
    let home = tempfile::tempdir().expect("temp home");
    let manifest: Manifest = toml::from_str(fixture::java::MANIFEST).expect("manifest");
    manifest.validate().expect("a valid manifest");
    let wasm = std::fs::read(fixture::java::component()).expect("replay-java component");
    let runtime = PluginRuntime::new(
        Arc::new(NoHost),
        fixture::plugin_cache(),
        Arc::new(engine::ProcessSupervisor::new(
            home.path().join("processes"),
        )),
    )
    .expect("runtime");
    let record = PluginRecord {
        id: manifest.id.clone(),
        wasm_sha256: Some(
            Sha256::digest(&wasm)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
        ),
        grant: manifest.grant(),
        ..PluginRecord::default()
    };
    let data = home.path().join("replay-java-data");
    std::fs::create_dir_all(&data).expect("data dir");
    let settings = PluginSettings::new(home.path().join("replay-java-settings.json"), Vec::new());
    let plugin = Arc::new(runtime.load(&record, &wasm, data, settings).expect("load"));
    let engine = fixture::engine(home.path(), vec![], vec![]);
    engine::Contributions::connect(&plugin, &manifest, false)
        .await
        .register(&engine, &manifest.id);
    (engine, home)
}

#[tokio::test]
async fn a_java_build_is_accepted_only_from_the_hosts_the_plugin_was_granted() {
    let (engine, _home) = rig().await;
    let java = engine.java();

    let releases = java.releases(DISTRIBUTION).await.expect("the releases");
    assert_eq!(
        releases
            .iter()
            .map(|release| release.major)
            .collect::<Vec<_>>(),
        [17, 21]
    );

    java.download_size(DISTRIBUTION, 21)
        .await
        .expect("a build on a granted host");
    let off_host = java
        .download_size(DISTRIBUTION, 17)
        .await
        .expect_err("a build on a host outside the grant is refused");
    assert!(
        format!("{off_host:#}").contains("refused build"),
        "{off_host:#}"
    );
    java.download_size(DISTRIBUTION, 8)
        .await
        .expect_err("a version the plugin has no build for");
}
