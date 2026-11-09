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

fn load(
    home: &std::path::Path,
    files: &fixture::Files,
    hosts: &[&str],
    engine: &Arc<engine::Engine>,
) -> Arc<LoadedPlugin> {
    let manifest: Manifest = toml::from_str(fixture::replay::MANIFEST).expect("manifest");
    manifest.validate().expect("a valid manifest");
    let wasm = std::fs::read(fixture::replay::component()).expect("replay-flavor component");
    let runtime = PluginRuntime::new(
        Arc::new(NoHost),
        home.join("plugin-cache"),
        Arc::new(engine::ProcessSupervisor::new(home.join("processes"))),
    )
    .expect("runtime")
    .with_java(engine.clone());
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

async fn rig(hosts: &[&str]) -> (Arc<engine::Engine>, tempfile::TempDir) {
    let home = tempfile::tempdir().expect("temp home");
    let files = fixture::Files::serve().await;
    let engine = Arc::new(fixture::engine(home.path(), vec![], vec![]));
    fixture::java_runtime(home.path(), 21);
    let manifest: Manifest = toml::from_str(fixture::replay::MANIFEST).expect("manifest");
    let plugin = load(home.path(), &files, hosts, &engine);
    engine::Contributions::connect(&plugin, &manifest)
        .await
        .register(&engine, &manifest.id, false);
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

/// The managed Java the fixture's `builder` program runs: a script that, asked
/// to `build`, writes the jar into its working directory and, asked to
/// `symlink`, leaves a link to a file outside it.
#[cfg(unix)]
fn builder_java(home: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    let java = home.join("java/fixture-21/bin/java");
    std::fs::write(
        &java,
        "#!/bin/sh\ncase \"$1\" in\nbuild) printf 'built jar' > built.jar;;\nsymlink) ln -sf /etc/passwd link.jar;;\nesac\nexit 0\n",
    )
    .expect("builder java");
    std::fs::set_permissions(&java, std::fs::Permissions::from_mode(0o755)).expect("executable");
}

async fn provision(engine: &engine::Engine, version: &str) -> anyhow::Result<engine::ServerRecord> {
    let (cancel, report) = (Cancel::new(), |_: &ProvisionProgress| {});
    engine
        .provision_server(server(version), &job(&cancel, &report))
        .await
        .map(|(record, _)| record)
}

fn refusal(error: anyhow::Error) -> String {
    match engine::error_info(error) {
        ErrorInfo::PluginCallFailed { id, detail } => {
            assert_eq!(id, "replay-flavor");
            detail
        }
        other => panic!("expected the plugin to be named, got {other:?}"),
    }
}

#[cfg(unix)]
#[tokio::test]
async fn a_built_server_jar_is_produced_in_the_build_area_and_placed_in_the_server() {
    let (engine, home) = rig(&["127.0.0.1"]).await;
    builder_java(home.path());

    let record = provision(&engine, "built").await.expect("provision");

    let data = engine.servers().data_dir(&record);
    assert_eq!(std::fs::read(data.join("built.jar")).unwrap(), b"built jar");
    assert!(
        home.path()
            .join("meta/flavors")
            .join(FLAVOR)
            .join("built.jar")
            .is_file(),
        "the program ran in the flavor's own build area"
    );
    assert!(
        !data.join("server.jar").exists(),
        "nothing was downloaded for a built jar"
    );
}

#[tokio::test]
async fn a_managed_java_program_resolves_to_the_runtime_under_the_data_home() {
    let (engine, home) = rig(&["127.0.0.1"]).await;

    let executable = engine::ManagedJava::executable(engine.as_ref(), 21)
        .await
        .expect("the runtime laid out under java/ is found, not installed again");

    let exe = if cfg!(windows) { "java.exe" } else { "java" };
    assert_eq!(
        executable,
        home.path().join("java/fixture-21/bin").join(exe)
    );
}

#[tokio::test]
async fn an_output_outside_the_build_area_is_refused() {
    let (engine, _home) = rig(&["127.0.0.1"]).await;

    let detail = refusal(provision(&engine, "outside").await.expect_err("refused"));

    assert!(
        detail.contains("not a file under the build area"),
        "{detail}"
    );
    assert!(engine.servers().list().is_empty());
}

#[cfg(unix)]
#[tokio::test]
async fn a_symlink_output_is_refused() {
    let (engine, home) = rig(&["127.0.0.1"]).await;
    builder_java(home.path());

    let detail = refusal(provision(&engine, "symlink").await.expect_err("refused"));

    assert!(detail.contains("symlink"), "{detail}");
}

#[tokio::test]
async fn a_missing_built_artifact_fails_the_provision_naming_the_flavor() {
    let (engine, _home) = rig(&["127.0.0.1"]).await;

    let detail = refusal(
        provision(&engine, "empty")
            .await
            .expect_err("nothing was built"),
    );

    assert!(detail.contains("did not produce `built.jar`"), "{detail}");
    assert!(detail.contains("replay"), "{detail}");
    assert!(engine.servers().list().is_empty());
}

#[tokio::test]
async fn a_built_client_library_lands_under_the_shared_libraries() {
    let (engine, home) = rig(&["127.0.0.1"]).await;
    let meta = home.path().join("meta");
    let client_jar = meta.join("versions/built/built.jar");
    std::fs::create_dir_all(client_jar.parent().unwrap()).unwrap();
    std::fs::write(&client_jar, b"vanilla client").unwrap();
    let (cancel, report) = (Cancel::new(), |_: &ProvisionProgress| {});

    let built = ["example/built/1.0/built-1.0.jar".to_string()];
    engine
        .minecraft()
        .install(
            engine::Side::Client,
            FLAVOR,
            &engine::InstallRequest {
                game_version: "built",
                loader_version: None,
                root: &meta,
                meta: &meta,
                minecraft_jar: Some(&client_jar),
                built: &built,
                java: std::path::Path::new("java"),
                cache: None,
                processes: engine.processes(),
            },
            &job(&cancel, &report),
        )
        .await
        .expect("install");

    assert_eq!(
        std::fs::read(meta.join("libraries/example/built/1.0/built-1.0.jar")).unwrap(),
        b"vanilla client",
        "the guest saw the vanilla jar read-only and built from it"
    );
}
