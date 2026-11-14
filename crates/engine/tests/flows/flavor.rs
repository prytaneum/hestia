//! A flavor contributed by a plugin, from the `replay-flavor` fixture:
//! registered on enable, provisioned from, and refused when its profile names
//! anything the plugin was not granted.

use std::sync::Arc;

use engine::{LoadedPlugin, PluginRecord, PluginRuntime};
use proto::plugin::{Manifest, Permission, PermissionScope, ScopeKind};
use sha2::{Digest, Sha256};

use super::launch_hooks::NoHost;
use super::*;

const FLAVOR: &str = "replay-flavor.replay";
const PLUGIN: &str = "replay-flavor";

fn load(
    home: &std::path::Path,
    files: &fixture::Files,
    hosts: &[&str],
    unhashed: &[&str],
    engine: &Arc<engine::Engine>,
    processes: &Arc<engine::ProcessSupervisor>,
) -> Arc<LoadedPlugin> {
    let manifest: Manifest = toml::from_str(fixture::replay::MANIFEST).expect("manifest");
    manifest.validate().expect("a valid manifest");
    let wasm = std::fs::read(fixture::replay::component()).expect("replay-flavor component");
    let runtime = PluginRuntime::new(Arc::new(NoHost), fixture::plugin_cache(), processes.clone())
        .expect("runtime")
        .with_java(engine.clone());
    let mut grant = manifest.grant();
    let mut network = PermissionScope::of(ScopeKind::Hosts, hosts.iter().copied());
    network.extend(ScopeKind::Unhashed, unhashed.iter().copied());
    grant
        .permissions
        .insert(Permission::NetworkConnect, network);
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
    rig_unhashed(hosts, &[]).await
}

async fn rig_unhashed(
    hosts: &[&str],
    unhashed: &[&str],
) -> (Arc<engine::Engine>, tempfile::TempDir) {
    let (engine, home, _) = rig_supervised(hosts, unhashed).await;
    (engine, home)
}

async fn rig_supervised(
    hosts: &[&str],
    unhashed: &[&str],
) -> (
    Arc<engine::Engine>,
    tempfile::TempDir,
    Arc<engine::ProcessSupervisor>,
) {
    let home = tempfile::tempdir().expect("temp home");
    let files = fixture::Files::serve().await;
    let engine = Arc::new(fixture::engine(
        home.path(),
        vec![fixture::Flavor::serving("vanilla", &files)],
        vec![],
    ));
    fixture::java_runtime(home.path(), 21);
    let manifest: Manifest = toml::from_str(fixture::replay::MANIFEST).expect("manifest");
    let processes = Arc::new(engine::ProcessSupervisor::new(
        home.path().join("processes"),
    ));
    let plugin = load(home.path(), &files, hosts, unhashed, &engine, &processes);
    engine::Contributions::connect(&plugin, &manifest, false)
        .await
        .register(&engine, &manifest.id);
    (engine, home, processes)
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
async fn a_sha256_artifact_is_downloaded_and_verified() {
    let (engine, _home) = rig(&["127.0.0.1"]).await;

    let record = provision(&engine, "sha256")
        .await
        .expect("a sha256 the download matches is accepted");
    assert!(engine
        .servers()
        .data_dir(&record)
        .join("server.jar")
        .is_file());

    provision_named(&engine, "wrongsum", "other")
        .await
        .expect_err("a sha256 the download does not match fails the provision");
}

#[tokio::test]
async fn a_profile_without_a_checksum_is_refused() {
    let (engine, home) = rig(&["127.0.0.1"]).await;

    engine
        .minecraft()
        .resolve_server(FLAVOR, "unsigned", None, home.path(), None)
        .await
        .expect_err("an artifact that cannot be verified is not downloaded");
}

#[tokio::test]
async fn an_unhashed_artifact_is_accepted_only_from_a_host_the_grant_lists() {
    let (declared, _home) = rig_unhashed(&["127.0.0.1"], &["127.0.0.1"]).await;
    let record = provision(&declared, "generated")
        .await
        .expect("a host the plugin declared may serve an unhashed jar");
    assert!(declared
        .servers()
        .data_dir(&record)
        .join("server.jar")
        .is_file());

    let (undeclared, _home) = rig(&["127.0.0.1"]).await;
    provision(&undeclared, "generated")
        .await
        .expect_err("the marker alone is not enough");
}

#[tokio::test]
async fn a_client_built_on_vanilla_is_the_additions_merged_over_the_vanilla_profile() {
    let (engine, home) = rig(&["127.0.0.1"]).await;

    let profile = engine
        .minecraft()
        .resolve_instance(FLAVOR, "onvanilla", None, home.path(), None)
        .await
        .expect("the additions are vetted and merged");

    assert_eq!(profile.flavor, FLAVOR);
    assert_eq!(profile.game_version, "onvanilla");
    assert_eq!(profile.loader_version.as_deref(), Some("9.9"));
    assert_eq!(
        profile.client.filename, "client.jar",
        "the client is vanilla's"
    );
    assert_eq!(profile.main_class, "example.loader.Main");
    assert_eq!(profile.jvm_args, ["-Dloader"]);
    assert_eq!(profile.game_args, ["--loader"]);
    assert_eq!(
        profile
            .libraries
            .iter()
            .map(|lib| lib.name.as_str())
            .collect::<Vec<_>>(),
        ["example:loader:1.0"]
    );
}

#[tokio::test]
async fn a_server_built_on_vanilla_runs_the_jar_the_flavor_names() {
    let (engine, _home) = rig(&["127.0.0.1"]).await;

    let record = provision(&engine, "onvanilla")
        .await
        .expect("provision a server built on vanilla");

    assert_eq!(record.profile.loader_version.as_deref(), Some("9.9"));
    assert_eq!(record.profile.java_major, 21, "the Java major is vanilla's");
    assert!(engine
        .servers()
        .data_dir(&record)
        .join("loader-server.jar")
        .is_file());
}

#[tokio::test]
async fn an_addition_off_the_plugins_hosts_is_refused_before_vanilla_is_asked() {
    let (engine, home) = rig(&["127.0.0.1"]).await;

    engine
        .minecraft()
        .resolve_instance(FLAVOR, "onvanilla-foreign", None, home.path(), None)
        .await
        .expect_err("the added library is on a host the plugin was not granted");
    provision(&engine, "onvanilla-foreign")
        .await
        .expect_err("so is the server jar");
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
        "#!/bin/sh\ncase \"$1\" in\nbuild) printf 'built jar' > built.jar;;\nexpand) printf '%s' \"$2\" > built.jar;;\nordered) echo start >> order.log; sleep 1; printf 'built jar' > built.jar; echo end >> order.log;;\nsymlink) ln -sf /etc/passwd link.jar;;\ncount) echo run >> count.log; touch marker;;\nnarrate) echo \"Compiling thing\"; echo \"[INFO] noise\"; sleep 2; printf 'built jar' > built.jar;;\nslow) sleep 5; touch first;;\nsecond) touch second;;\ncopy) cp dl/tool.jar built.jar;;\nesac\nexit 0\n",
    )
    .expect("builder java");
    std::fs::set_permissions(&java, std::fs::Permissions::from_mode(0o755)).expect("executable");
}

async fn provision(engine: &engine::Engine, version: &str) -> anyhow::Result<engine::ServerRecord> {
    provision_named(engine, version, "smp").await
}

async fn provision_named(
    engine: &engine::Engine,
    version: &str,
    name: &str,
) -> anyhow::Result<engine::ServerRecord> {
    let (cancel, report) = (Cancel::new(), |_: &ProvisionProgress| {});
    let spec = ServerCreateSpec {
        name: name.into(),
        ..server(version)
    };
    engine
        .provision_server(spec, &job(&cancel, &report))
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
            .join(PLUGIN)
            .join("built.jar")
            .is_file(),
        "the program ran in the flavor's own build area"
    );
    assert!(
        !data.join("server.jar").exists(),
        "nothing was downloaded for a built jar"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn a_build_argument_names_the_host_path_of_the_build_area() {
    let (engine, home) = rig(&["127.0.0.1"]).await;
    builder_java(home.path());

    let record = provision(&engine, "expand").await.expect("provision");

    let data = engine.servers().data_dir(&record);
    let area = home.path().join("meta/flavors").join(PLUGIN);
    assert_eq!(
        std::fs::read_to_string(data.join("built.jar")).unwrap(),
        format!("{}/x", area.display())
    );
}

#[cfg(unix)]
#[tokio::test]
async fn two_builds_of_one_flavor_run_one_after_the_other() {
    let (engine, home) = rig(&["127.0.0.1"]).await;
    builder_java(home.path());

    let (first, second) = tokio::join!(
        provision_named(&engine, "ordered", "one"),
        provision_named(&engine, "ordered", "two")
    );
    first.expect("first provision");
    second.expect("second provision");

    let log = std::fs::read_to_string(
        home.path()
            .join("meta/flavors")
            .join(PLUGIN)
            .join("order.log"),
    )
    .unwrap();
    assert_eq!(log, "start\nend\nstart\nend\n");
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
        detail.contains("not a path under the build area"),
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
                java_major: 21,
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

async fn install_client(
    engine: &engine::Engine,
    home: &std::path::Path,
    version: &str,
    built: &[String],
) -> anyhow::Result<()> {
    let meta = home.join("meta");
    let client_jar = meta.join("versions").join(version).join("client.jar");
    std::fs::create_dir_all(client_jar.parent().unwrap()).unwrap();
    std::fs::write(&client_jar, b"vanilla client").unwrap();
    let (cancel, report) = (Cancel::new(), |_: &ProvisionProgress| {});
    engine
        .minecraft()
        .install(
            engine::Side::Client,
            FLAVOR,
            &engine::InstallRequest {
                game_version: version,
                loader_version: None,
                root: &meta,
                meta: &meta,
                minecraft_jar: Some(&client_jar),
                built,
                java_major: 21,
                java: std::path::Path::new("java"),
                cache: None,
                processes: engine.processes(),
            },
            &job(&cancel, &report),
        )
        .await
}

#[tokio::test]
async fn a_server_built_as_trees_is_placed_and_launches_from_the_args_file_of_this_os() {
    let (engine, home) = rig(&["127.0.0.1"]).await;

    let record = provision(&engine, "tree").await.expect("provision");

    let data = engine.servers().data_dir(&record);
    assert_eq!(
        std::fs::read(data.join("tree/unix_args.txt")).unwrap(),
        b"unix"
    );
    assert_eq!(
        std::fs::read(
            home.path()
                .join("meta/libraries/example/loader/1.0/loader-1.0.jar")
        )
        .unwrap(),
        b"loader",
        "a tree aimed at the libraries root lands there, not in the server"
    );
    let proto::profile::ServerLaunch::ArgsFile(args) = &record.profile.launch else {
        panic!("the profile launches from an args file");
    };
    assert_eq!(args.for_os("windows"), "tree/win_args.txt");
    assert_eq!(args.for_os("linux"), "tree/unix_args.txt");
}

#[tokio::test]
async fn an_args_file_that_leaves_the_data_directory_is_refused() {
    let (engine, _home) = rig(&["127.0.0.1"]).await;

    let detail = refusal(provision(&engine, "badargs").await.expect_err("refused"));

    assert!(detail.contains("refused profile"), "{detail}");
    assert!(engine.servers().list().is_empty());
}

#[tokio::test]
async fn a_library_off_the_classpath_is_kept_off_it_and_still_placed() {
    let (engine, home) = rig(&["127.0.0.1"]).await;

    let profile = engine
        .minecraft()
        .resolve_instance(FLAVOR, "tree", None, home.path(), None)
        .await
        .expect("resolve");
    let built = profile
        .built()
        .into_iter()
        .map(String::from)
        .collect::<Vec<_>>();
    assert_eq!(built.len(), 1);
    assert!(
        !profile.libraries[0].classpath,
        "the flag the guest set survives vetting"
    );

    install_client(&engine, home.path(), "tree", &built)
        .await
        .expect("install");

    assert_eq!(
        std::fs::read(home.path().join("meta/libraries").join(&built[0])).unwrap(),
        b"off"
    );
}

#[tokio::test]
async fn a_client_has_no_data_directory_to_place_a_tree_in() {
    let (engine, home) = rig(&["127.0.0.1"]).await;

    let error = install_client(&engine, home.path(), "tree-entry", &[])
        .await
        .expect_err("refused");

    assert!(refusal(error).contains("no destination"));
}

fn host_facts() -> (&'static str, &'static str, &'static str) {
    let os = if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "osx"
    } else {
        "linux"
    };
    let arch = if cfg!(target_arch = "aarch64") {
        "arm64"
    } else if cfg!(target_arch = "x86") {
        "x86"
    } else {
        "x86_64"
    };
    (os, arch, if cfg!(windows) { ";" } else { ":" })
}

#[tokio::test]
async fn a_maven_library_is_completed_by_the_host_and_materialised_at_launch() {
    let (engine, home) = rig_unhashed(&["127.0.0.1"], &["127.0.0.1"]).await;
    fixture::account(home.path(), "player");

    let record = engine
        .create_instance("mavened", FLAVOR, "onvanilla-maven", None, &[])
        .await
        .expect("a Maven library resolves");

    let library = record
        .profile
        .libraries
        .iter()
        .find(|library| library.name == "example:mvn:1.0")
        .expect("the library is persisted as a plain one");
    assert_eq!(library.path, "example/mvn/1.0/mvn-1.0.jar");
    assert!(library
        .artifact
        .url
        .ends_with("/example/mvn/1.0/mvn-1.0.jar"));
    assert_eq!(library.artifact.filename, "mvn-1.0.jar");
    assert_eq!(
        library
            .artifact
            .checksum
            .as_ref()
            .map(|sum| sum.hex.as_str()),
        Some(fixture::Files::BODY_SHA1),
        "the sidecar is the checksum"
    );

    let (cancel, report) = (Cancel::new(), |_: &ProvisionProgress| {});
    engine
        .prepare_instance(launch(&record.id, None), &job(&cancel, &report))
        .await
        .expect("prepare");
    assert_eq!(
        std::fs::read(
            home.path()
                .join("meta/libraries/example/mvn/1.0/mvn-1.0.jar")
        )
        .unwrap(),
        fixture::Files::BODY
    );
}

#[tokio::test]
async fn a_maven_library_whose_sidecar_is_missing_refuses_the_whole_answer() {
    let (engine, home) = rig(&["127.0.0.1"]).await;

    let error = engine
        .minecraft()
        .resolve_instance(FLAVOR, "onvanilla-maven-missing", None, home.path(), None)
        .await
        .expect_err("a library with no checksum to verify against");
    assert!(error.to_string().contains("nosum"), "{error:#}");
}

#[tokio::test]
async fn a_sidecar_on_a_host_outside_the_grant_is_not_fetched() {
    let (engine, home) = rig(&["localhost"]).await;

    let error = engine
        .minecraft()
        .resolve_instance(FLAVOR, "onvanilla-maven", None, home.path(), None)
        .await
        .expect_err("the repository is not a granted host");
    assert!(error.to_string().contains("127.0.0.1"), "{error:#}");
}

#[tokio::test]
async fn a_malformed_coordinate_refuses_the_whole_answer() {
    let (engine, home) = rig(&["127.0.0.1"]).await;

    let error = engine
        .minecraft()
        .resolve_instance(FLAVOR, "onvanilla-maven-bad", None, home.path(), None)
        .await
        .expect_err("`example:bad` is not a coordinate");
    assert!(error.to_string().contains("example:bad"), "{error:#}");
}

#[tokio::test]
async fn libraries_and_arguments_the_rules_disallow_here_never_persist() {
    let (engine, home) = rig(&["127.0.0.1"]).await;

    let profile = engine
        .minecraft()
        .resolve_instance(FLAVOR, "onvanilla-rules", None, home.path(), None)
        .await
        .expect("resolve");

    let names: Vec<&str> = profile
        .libraries
        .iter()
        .map(|lib| lib.name.as_str())
        .collect();
    assert!(names.contains(&"example:loader:1.0"), "{names:?}");
    assert!(names.contains(&"example:here:1"), "{names:?}");
    assert!(!names.contains(&"example:elsewhere:1"), "{names:?}");
    assert_eq!(profile.jvm_args, ["-Dloader", "-Dhere"]);

    let server = provision(&engine, "onvanilla").await.expect("server");
    assert_eq!(server.profile.jvm_args, ["-Dloader", "-Dhere"]);
}

#[tokio::test]
async fn a_guest_is_told_the_machine_it_resolves_for() {
    let (engine, home) = rig(&["127.0.0.1"]).await;
    let (os, arch, separator) = host_facts();

    let profile = engine
        .minecraft()
        .resolve_instance(FLAVOR, "onvanilla-target", None, home.path(), None)
        .await
        .expect("resolve");

    assert_eq!(
        profile.game_args.last().map(String::as_str),
        Some(format!("--target={os}/{arch}/{separator}/None").as_str()),
        "no Java major is known before vanilla is resolved"
    );
}

fn area(home: &std::path::Path) -> std::path::PathBuf {
    home.join("meta/flavors").join(PLUGIN)
}

async fn provision_seen(
    engine: &engine::Engine,
    version: &str,
    cancel: &Cancel,
) -> (anyhow::Result<engine::ServerRecord>, Vec<String>) {
    let seen = std::sync::Mutex::new(Vec::new());
    let report = |progress: &ProvisionProgress| seen.lock().unwrap().push(progress.detail.clone());
    let outcome = engine
        .provision_server(server(version), &job(cancel, &report))
        .await
        .map(|(record, _)| record);
    let details = seen.lock().unwrap().clone();
    (outcome, details)
}

#[cfg(unix)]
#[tokio::test]
async fn a_plan_runs_under_the_supervisor_and_the_host_places_its_output() {
    let (engine, home, processes) = rig_supervised(&["127.0.0.1"], &[]).await;
    builder_java(home.path());

    let record = provision(&engine, "plan").await.expect("provision");

    let data = engine.servers().data_dir(&record);
    assert_eq!(std::fs::read(data.join("built.jar")).unwrap(), b"built jar");
    let steps: Vec<_> = processes
        .list()
        .into_iter()
        .filter(|info| info.id.starts_with("build-replay-flavor-plan-0-"))
        .collect();
    assert_eq!(steps.len(), 1, "the step ran as one supervised task");
    assert_eq!(steps[0].owner.as_deref(), Some("plugin:replay-flavor"));
}

#[cfg(unix)]
#[tokio::test]
async fn a_step_narrates_only_the_lines_its_filter_lets_through() {
    let (engine, home) = rig(&["127.0.0.1"]).await;
    builder_java(home.path());

    let (outcome, details) = provision_seen(&engine, "plan-narrate", &Cancel::new()).await;

    outcome.expect("provision");
    assert!(
        details.iter().any(|line| line == "Compiling thing"),
        "{details:?}"
    );
    assert!(
        details.iter().all(|line| !line.contains("noise")),
        "{details:?}"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn a_guests_own_exec_shows_its_output_as_it_prints() {
    let (engine, home) = rig(&["127.0.0.1"]).await;
    builder_java(home.path());

    let (outcome, details) = provision_seen(&engine, "live", &Cancel::new()).await;

    outcome.expect("provision");
    assert!(
        details.iter().any(|line| line == "[INFO] noise"),
        "{details:?}"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn a_step_whose_result_exists_is_skipped_and_racing_builds_run_it_once() {
    let (engine, home) = rig(&["127.0.0.1"]).await;
    builder_java(home.path());

    let (first, second) = tokio::join!(
        provision_named(&engine, "plan-steps", "one"),
        provision_named(&engine, "plan-steps", "two")
    );
    first.expect("first provision");
    second.expect("second provision");
    provision_named(&engine, "plan-steps", "three")
        .await
        .expect("a later provision");

    let log = std::fs::read_to_string(area(home.path()).join("count.log")).unwrap();
    assert_eq!(log, "run\n", "the counting step ran once for three creates");
}

#[cfg(unix)]
#[tokio::test]
async fn a_build_joins_the_step_already_running_under_its_task_id() {
    let (engine, home, processes) = rig_supervised(&["127.0.0.1"], &[]).await;
    builder_java(home.path());
    let area = area(home.path());
    std::fs::create_dir_all(&area).unwrap();
    let counting = proto::provider::flavor::BuildStep {
        program: "builder".into(),
        args: vec!["count".into()],
        creates: vec!["/build/marker".into()],
        ..Default::default()
    };
    processes
        .start(proto::process::ProcessSpec {
            id: engine::build_task_id(PLUGIN, "plan-steps", 0, &counting),
            program: "/bin/sh".into(),
            args: vec![
                "-c".into(),
                "echo run >> count.log; sleep 1; touch marker".into(),
            ],
            cwd: Some(area.clone()),
            ..Default::default()
        })
        .await
        .expect("a build already running");

    provision(&engine, "plan-steps").await.expect("provision");

    let log = std::fs::read_to_string(area.join("count.log")).unwrap();
    assert_eq!(
        log, "run\n",
        "joined the running step instead of starting another"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn cancelling_stops_the_running_step_and_starts_no_later_one() {
    let (engine, home) = rig(&["127.0.0.1"]).await;
    builder_java(home.path());
    let cancel = Cancel::new();

    let ((outcome, _), ()) = tokio::join!(provision_seen(&engine, "plan-slow", &cancel), async {
        tokio::time::sleep(std::time::Duration::from_millis(600)).await;
        cancel.cancel();
    });

    assert!(engine::is_cancelled(&outcome.expect_err("cancelled")));
    assert!(!area(home.path()).join("second").exists());
    assert!(!area(home.path()).join("first").exists());
    assert!(engine.servers().list().is_empty());
}

#[cfg(unix)]
#[tokio::test]
async fn the_host_downloads_what_a_plan_fetches_before_its_steps_run() {
    let (engine, home) = rig(&["127.0.0.1"]).await;
    builder_java(home.path());

    let record = provision(&engine, "plan-fetch").await.expect("provision");

    let fetched = std::fs::read(area(home.path()).join("dl/tool.jar")).unwrap();
    assert!(!fetched.is_empty());
    assert_eq!(
        std::fs::read(engine.servers().data_dir(&record).join("built.jar")).unwrap(),
        fetched
    );
}

#[cfg(unix)]
#[tokio::test]
async fn a_plan_naming_anything_undeclared_or_outside_the_area_is_refused_before_it_runs() {
    let (engine, home) = rig(&["127.0.0.1"]).await;
    builder_java(home.path());

    for (version, reason) in [
        ("refused-program", "is not a program this plugin declares"),
        ("refused-args", "outside what `strict` declares"),
        ("refused-env", "does not declare the environment variable"),
        ("refused-cwd", "is not a path under the build area"),
        ("refused-creates", "is not a path under the build area"),
        ("refused-dest", "is not a path under the build area"),
        ("refused-output", "is not a path under the build area"),
        ("refused-mount", "has no `data` directory"),
    ] {
        let detail = refusal(provision(&engine, version).await.expect_err(version));
        assert!(detail.contains("build plan refused"), "{version}: {detail}");
        assert!(detail.contains(reason), "{version}: {detail}");
    }

    let area = area(home.path());
    assert!(!area.join("built.jar").exists(), "no step ran");
    assert!(!home.path().join("escaped").exists());
    assert!(engine.servers().list().is_empty());
}
