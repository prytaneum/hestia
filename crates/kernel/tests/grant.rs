//! Manifest, derived grant, install and start-up of WASM plugins, against real
//! components built from `tests/guest/` and `tests/guest-services/`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use kernel::{
    denials, Builtin, Busy, Descriptor, GrantError, GuestError, Limits, ManifestError,
    PackageError, PluginError, Plugins, Sandbox, SandboxError, Services, Shape, ShapeError,
};
use proto::plugin::{PluginKind, PluginState, ScopeKind, Tier};
use sha2::{Digest, Sha256};

mod fixture;
use fixture::{guest, scratch, service_guest, wasm};

/// Mounts `/data` and keeps settings and secrets beside it, under one home.
struct Files(PathBuf);

impl Services for Files {
    fn data_dir(&self, plugin: &str) -> Option<PathBuf> {
        Some(kernel::sandbox_root(&self.0).join(plugin))
    }

    fn state_dir(&self, plugin: &str) -> Option<PathBuf> {
        Some(self.0.join("plugins").join(plugin))
    }
}

fn sandbox(home: &Path) -> Arc<Sandbox> {
    let sandbox = Sandbox::new(
        scratch().join("plugin-cache"),
        &scratch().join("plugin-cache.key"),
        Limits::default(),
    );
    Arc::new(
        sandbox
            .expect("sandbox")
            .with_services(Arc::new(Files(home.to_owned()))),
    )
}

fn manifest(dir: &Path, id: &str, extra: &str) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(
        dir.join("plugin.toml"),
        format!("id = \"{id}\"\nname = \"{id}\"\nversion = \"0.1.0\"\n{extra}"),
    )
    .unwrap();
}

fn package(dir: &Path, id: &str, component: PathBuf, extra: &str) {
    manifest(dir, id, extra);
    std::fs::copy(component, dir.join("plugin.wasm")).unwrap();
}

fn plugins(home: &Path, builtins: Vec<Arc<dyn Builtin>>) -> Plugins {
    Plugins::new(home, builtins, Some(sandbox(home)))
        .unwrap()
        .with_developer(true)
}

fn state(plugins: &Plugins, id: &str) -> Option<PluginState> {
    plugins
        .list()
        .into_iter()
        .find(|p| p.id == id)
        .map(|p| p.state)
}

fn cache_entry(plugin: &str) -> PathBuf {
    let name: String = Sha256::digest(plugin)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    scratch().join("plugin-cache").join(format!("{name}.cwasm"))
}

fn refusal(result: Result<proto::plugin::Inspection, PluginError>) -> PackageError {
    match result {
        Err(PluginError::Package(reason)) => reason,
        other => panic!("expected a package refusal, got {other:?}"),
    }
}

#[tokio::test]
async fn a_package_is_refused_at_the_edge_for_what_it_says_and_what_it_is() {
    let home = tempfile::tempdir().unwrap();
    let plugins = plugins(home.path(), Vec::new());
    let pkg = home.path().join("pkg");
    let hosts = |hosts: &str| format!("[permissions.\"network:connect\"]\nhosts = [{hosts}]\n");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::copy(guest("net"), pkg.join("plugin.wasm")).unwrap();

    type Check = fn(&PackageError) -> bool;
    let long = "a".repeat(65);
    let cases: [(&str, String, Check); 12] = [
        ("../x", hosts("\"a.example\""), |e| {
            matches!(e, PackageError::Manifest(ManifestError::Id(_)))
        }),
        ("Upper", hosts("\"a.example\""), |e| {
            matches!(e, PackageError::Manifest(ManifestError::Id(_)))
        }),
        (&long, hosts("\"a.example\""), |e| {
            matches!(e, PackageError::Manifest(ManifestError::Id(_)))
        }),
        ("con", hosts("\"a.example\""), |e| {
            matches!(e, PackageError::Manifest(ManifestError::Id(_)))
        }),
        ("ok", hosts("\"a.example\\u001b[2K\""), |e| {
            matches!(e, PackageError::Manifest(ManifestError::Scope(_)))
        }),
        ("ok", hosts("\"a.example\\u202e\""), |e| {
            matches!(e, PackageError::Manifest(ManifestError::Scope(_)))
        }),
        ("ok", hosts("\"1.2.3.4\""), |e| {
            matches!(e, PackageError::Grant(GrantError::Host { .. }))
        }),
        ("ok", hosts("\"*.com\""), |e| {
            matches!(e, PackageError::Grant(GrantError::Host { .. }))
        }),
        ("ok", String::new(), |e| {
            matches!(e, PackageError::Grant(GrantError::ScopeMissing(_)))
        }),
        (
            "ok",
            format!(
                "{}[permissions.\"secrets\"]\nhosts = [\"a.example\"]\n",
                hosts("\"a.example\"")
            ),
            |e| matches!(e, PackageError::Grant(GrantError::ScopeNotNeeded(_))),
        ),
        (
            "ok",
            "[permissions.\"network:connect\"]\npaths = [\"x\"]\n".into(),
            |e| matches!(e, PackageError::Grant(GrantError::ScopeKind { .. })),
        ),
        (
            "ok",
            format!(
                "{}[permissions.\"other:thing\"]\nhosts = [\"a.example\"]\n",
                hosts("\"a.example\"")
            ),
            |e| matches!(e, PackageError::Grant(GrantError::ScopeNotNeeded(_))),
        ),
    ];
    for (id, extra, expected) in cases {
        manifest(&pkg, id, &extra);
        let reason = refusal(plugins.inspect(&pkg).await);
        assert!(expected(&reason), "{id} {extra:?}: {reason}");
    }

    manifest(&pkg, "ok", &hosts("\"a.example\""));
    std::fs::write(
        pkg.join("plugin.toml"),
        "id = \"ok\"\nname = \"\\u001b[1AHestia\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    assert!(matches!(
        refusal(plugins.inspect(&pkg).await),
        PackageError::Manifest(ManifestError::Name)
    ));

    std::fs::write(pkg.join("plugin.toml"), vec![b'#'; (64 << 10) + 1]).unwrap();
    assert!(matches!(
        refusal(plugins.inspect(&pkg).await),
        PackageError::TooLarge { .. }
    ));

    manifest(&pkg, "ok", &hosts("\"a.example\""));
    for (bytes, core) in [(&b"\0asm\x01\0\0\0"[..], true), (&b"nonsense"[..], false)] {
        std::fs::write(pkg.join("plugin.wasm"), bytes).unwrap();
        let reason = refusal(plugins.inspect(&pkg).await);
        assert!(
            matches!(
                (&reason, core),
                (PackageError::Shape(ShapeError::NotAComponent), true)
                    | (PackageError::Shape(ShapeError::Malformed(_)), false)
            ),
            "{reason}"
        );
    }

    #[cfg(unix)]
    {
        let secret = home.path().join("elsewhere.toml");
        std::fs::write(&secret, "id = \"ok\"\nname = \"ok\"\nversion = \"0.1.0\"\n").unwrap();
        std::fs::remove_file(pkg.join("plugin.toml")).unwrap();
        std::os::unix::fs::symlink(&secret, pkg.join("plugin.toml")).unwrap();
        assert!(matches!(
            refusal(plugins.inspect(&pkg).await),
            PackageError::NotAFile(_)
        ));
    }
}

#[tokio::test]
async fn the_grant_comes_from_the_component_and_the_digest_from_what_it_reaches() {
    let home = tempfile::tempdir().unwrap();
    let plugins = plugins(home.path(), Vec::new());
    let pkg = home.path().join("pkg");
    let scoped = |hosts: &str| format!("[permissions.\"network:connect\"]\nhosts = [{hosts}]\n");

    package(&pkg, "grant-net", guest("net"), &scoped("\"a.example\""));
    let inspection = plugins.inspect(&pkg).await.unwrap();
    let names: Vec<_> = inspection
        .grant
        .permissions
        .iter()
        .map(|p| p.permission.as_str())
        .collect();
    assert_eq!(names, ["network:connect", "secrets"]);
    let scope = inspection.grant.permissions[0].scope.as_ref().unwrap();
    assert_eq!(
        (scope.kind, scope.values.as_slice()),
        (ScopeKind::Hosts, ["a.example".to_string()].as_slice())
    );
    assert!(inspection
        .grant
        .contributes
        .iter()
        .any(|name| name.contains("lifecycle")));
    assert!(
        state(&plugins, "grant-net").is_none(),
        "inspecting installs nothing"
    );

    for widened in [
        scoped("\"b.example\""),
        format!("{}# a comment\n", scoped("\"a.example\"")),
    ] {
        package(&pkg, "grant-net", guest("net"), &widened);
        let changed = plugins.inspect(&pkg).await.unwrap();
        assert_ne!(changed.approval_digest, inspection.approval_digest);
    }
}

#[tokio::test]
async fn an_ungranted_interface_answers_denied_naming_its_permission() {
    let home = tempfile::tempdir().unwrap();
    let sandbox = sandbox(home.path());
    let registry = plugins(home.path(), Vec::new());

    let tour = wasm(service_guest("tour"));
    let shape = Shape::read(&tour).unwrap();
    let denied = denials(registry.table(), &shape, &[]);
    assert_eq!(denied.values().collect::<Vec<_>>(), ["secrets"]);
    let opened = sandbox.open("grant-denied", tour).await.unwrap();
    let loaded = sandbox
        .link(&opened, &denied, &Default::default(), Default::default())
        .unwrap();
    let data = kernel::sandbox_root(home.path()).join("grant-denied");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(data.join("mode"), "secret").unwrap();
    match sandbox.init(&loaded).await {
        Err(SandboxError::Guest(GuestError::Denied(permission))) => {
            assert_eq!(permission, "secrets")
        }
        other => panic!("expected denied(secrets), got {other:?}"),
    }

    let hello = wasm(guest("hello"));
    let denied = std::collections::HashMap::from([("hestia:kernel/log@0.1.0".into(), "x".into())]);
    let opened = sandbox.open("grant-undeniable", hello).await.unwrap();
    assert!(
        matches!(
            sandbox.link(&opened, &denied, &Default::default(), Default::default()),
            Err(SandboxError::Load(_))
        ),
        "a function with no error to answer cannot be denied, only refused"
    );

    let reach = wasm(service_guest("reach"));
    let shape = Shape::read(&reach).unwrap();
    let denied = denials(
        registry.table(),
        &shape,
        &["network:connect", "process:spawn"],
    );
    assert_eq!(denied.values().collect::<Vec<_>>(), ["ipc:connect"]);
    let opened = sandbox.open("grant-resource", reach).await.unwrap();
    assert!(
        matches!(
            sandbox.link(&opened, &denied, &Default::default(), Default::default()),
            Err(SandboxError::Load(_))
        ),
        "an interface that holds a resource cannot be denied, only refused"
    );
}

struct Core;

impl Builtin for Core {
    fn descriptor(&self) -> Descriptor {
        Descriptor {
            id: "core",
            name: "core",
            provides: None,
            requires: &[],
        }
    }

    fn enable(&self) -> anyhow::Result<()> {
        Ok(())
    }

    fn disable(&self) -> Result<(), Busy> {
        Ok(())
    }
}

#[tokio::test]
async fn an_installed_plugin_survives_restarts_and_removal_takes_its_cache() {
    let home = tempfile::tempdir().unwrap();
    let pkg = home.path().join("pkg");
    let build = || plugins(home.path(), vec![Arc::new(Core)]);
    let registry = build();

    package(&pkg, "grant-keep", guest("hello"), "");
    let approved = registry.inspect(&pkg).await.unwrap().approval_digest;
    assert!(matches!(
        registry.install(&pkg, "stale", Default::default()).await,
        Err(PluginError::Changed(_))
    ));
    let (first, second) = tokio::join!(
        registry.install(&pkg, &approved, Default::default()),
        registry.install(&pkg, &approved, Default::default())
    );
    let installed: Vec<_> = [first, second].into_iter().filter_map(Result::ok).collect();
    assert_eq!(installed.len(), 1, "one of two racing installs wins");
    assert_eq!(
        (installed[0].kind, installed[0].state),
        (PluginKind::Wasm, PluginState::Enabled)
    );
    package(&pkg, "core", guest("hello"), "");
    let digest = registry.inspect(&pkg).await.unwrap().approval_digest;
    assert!(matches!(
        registry.install(&pkg, &digest, Default::default()).await,
        Err(PluginError::Exists(_))
    ));
    assert!(matches!(
        registry.remove("core").await,
        Err(PluginError::Builtin(_))
    ));
    assert!(cache_entry("grant-keep").exists());

    let restarted = build();
    restarted.restore().await;
    assert_eq!(state(&restarted, "grant-keep"), Some(PluginState::Enabled));
    restarted.disable("grant-keep", false).await.unwrap();
    let again = build();
    again.restore().await;
    assert_eq!(state(&again, "grant-keep"), Some(PluginState::Disabled));
    assert_eq!(
        again.enable("grant-keep").await.unwrap().state,
        PluginState::Enabled
    );

    let blind = Plugins::new(home.path(), Vec::new(), None).unwrap();
    blind.restore().await;
    assert_eq!(
        state(&blind, "grant-keep"),
        Some(PluginState::Failed),
        "without a sandbox it is listed and never runs"
    );

    std::fs::copy(
        guest("chatty"),
        home.path().join("plugins/grant-keep/plugin.wasm"),
    )
    .unwrap();
    let tampered = build();
    tampered.restore().await;
    assert_eq!(state(&tampered, "grant-keep"), Some(PluginState::Failed));
    tampered.disable("grant-keep", false).await.unwrap();
    let still_off = build();
    still_off.restore().await;
    assert_eq!(
        state(&still_off, "grant-keep"),
        Some(PluginState::Disabled),
        "a plugin the user turned off stays off while it cannot load"
    );

    std::fs::create_dir_all(home.path().join("plugins/grant-half.staging")).unwrap();
    still_off.remove("grant-keep").await.unwrap();
    assert!(!home.path().join("plugins/grant-keep").exists());
    assert!(!cache_entry("grant-keep").exists());
    assert!(state(&still_off, "grant-keep").is_none());
    build().restore().await;
    assert!(
        !home.path().join("plugins/grant-half.staging").exists(),
        "a restart reclaims an unfinished install"
    );
}

#[tokio::test]
async fn a_secret_is_stored_only_for_a_plugin_that_asks_for_it() {
    let home = tempfile::tempdir().unwrap();
    let registry = plugins(home.path(), Vec::new());
    for (id, component) in [
        ("grant-asks", service_guest("tour")),
        ("grant-quiet", guest("hello")),
    ] {
        let pkg = home.path().join(id);
        package(&pkg, id, component, "");
        let data = kernel::sandbox_root(home.path()).join(id);
        std::fs::create_dir_all(&data).unwrap();
        std::fs::write(data.join("mode"), "free").unwrap();
        let inspection = registry.inspect(&pkg).await.unwrap();
        registry
            .install(&pkg, &inspection.approval_digest, Default::default())
            .await
            .unwrap();
    }

    registry
        .store_secret("grant-asks", "token", "s3cret")
        .unwrap();
    let secrets =
        std::fs::read_to_string(home.path().join("plugins/grant-asks/secrets.json")).unwrap();
    assert!(secrets.contains("s3cret"));
    assert!(matches!(
        registry.store_secret("grant-quiet", "token", "s3cret"),
        Err(PluginError::Secret(_))
    ));
}

#[tokio::test]
async fn a_plugin_that_reaches_its_trap_limit_is_disabled_for_good() {
    let home = tempfile::tempdir().unwrap();
    let pkg = home.path().join("pkg");
    let registry = plugins(home.path(), Vec::new());

    package(&pkg, "grant-trap", guest("trap"), "");
    let approved = registry.inspect(&pkg).await.unwrap().approval_digest;
    let installed = registry
        .install(&pkg, &approved, Default::default())
        .await
        .unwrap();
    assert_eq!(installed.state, PluginState::Failed);
    assert!(matches!(
        registry.enable("grant-trap").await,
        Err(PluginError::Failed { .. })
    ));
    assert_eq!(state(&registry, "grant-trap"), Some(PluginState::Failed));
    assert!(registry.enable("grant-trap").await.is_err());
    assert_eq!(state(&registry, "grant-trap"), Some(PluginState::Disabled));

    let restarted = plugins(home.path(), Vec::new());
    restarted.restore().await;
    assert_eq!(state(&restarted, "grant-trap"), Some(PluginState::Disabled));
}

#[tokio::test]
async fn a_plugin_reaches_only_what_it_was_granted_and_a_moved_program_loses_its_approval() {
    let home = tempfile::tempdir().unwrap();
    let apart = tempfile::tempdir().unwrap();
    let tool = apart
        .path()
        .join(if cfg!(windows) { "tool.exe" } else { "tool" });
    std::fs::write(&tool, b"v1").unwrap();
    let data = kernel::sandbox_root(home.path()).join("grant-reach");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(data.join("tool"), tool.to_str().unwrap()).unwrap();
    let extra = format!(
        "[permissions.\"network:connect\"]\nhosts = [\"files.example\"]\n\
         [permissions.\"process:spawn\"]\nprograms = [{{ path = '{}', args = ['--list'] }}]\n\
         [permissions.\"ipc:connect\"]\nendpoints = ['/run/granted.sock', '\\\\.\\pipe\\granted']\n",
        tool.display()
    );
    let pkg = home.path().join("pkg");
    package(&pkg, "grant-reach", service_guest("reach"), &extra);
    let build = || plugins(home.path(), Vec::new());

    let registry = build();
    let inspection = registry.inspect(&pkg).await.unwrap();
    let granted: Vec<_> = inspection
        .grant
        .permissions
        .iter()
        .map(|p| (p.permission.as_str(), p.tier))
        .collect();
    assert_eq!(
        granted,
        [
            ("ipc:connect", Tier::Unrestricted),
            ("network:connect", Tier::Sandboxed),
            ("process:spawn", Tier::Unrestricted),
        ]
    );
    assert!(inspection.grant.requires.is_empty());
    let installed = registry
        .install(&pkg, &inspection.approval_digest, Default::default())
        .await
        .unwrap();
    assert_eq!(
        installed.state,
        PluginState::Enabled,
        "its init saw every refusal"
    );

    let restarted = build();
    restarted.restore().await;
    assert_eq!(state(&restarted, "grant-reach"), Some(PluginState::Enabled));

    std::fs::write(&tool, b"v2").unwrap();
    let moved = build();
    moved.restore().await;
    assert_eq!(
        state(&moved, "grant-reach"),
        Some(PluginState::Failed),
        "a program that is not the one approved is not run"
    );
    assert_ne!(
        moved.inspect(&pkg).await.unwrap().approval_digest,
        inspection.approval_digest
    );

    let own = build().reserving(vec![tool.clone()], Vec::new());
    assert!(
        matches!(own.inspect(&pkg).await, Err(PluginError::Package(_))),
        "Hestia's own binary is never granted"
    );
}

/// A component whose only import is `wasi:http/outgoing-handler@0.2.6`.
fn http_component() -> Vec<u8> {
    let name = b"wasi:http/outgoing-handler@0.2.6";
    let mut bytes = vec![0, 0x61, 0x73, 0x6d, 0x0d, 0, 1, 0, 7, 3, 1, 0x42, 0];
    bytes.extend([10, name.len() as u8 + 5, 1, 0, name.len() as u8]);
    bytes.extend(name);
    bytes.extend([5, 0]);
    bytes
}

#[tokio::test]
async fn a_wasi_http_plugin_installs_without_requiring_a_package() {
    let home = tempfile::tempdir().unwrap();
    let pkg = home.path().join("pkg");
    manifest(
        &pkg,
        "grant-http",
        "[permissions.\"network:connect\"]\nhosts = [\"api.example\"]\n",
    );
    std::fs::write(pkg.join("plugin.wasm"), http_component()).unwrap();
    let registry = plugins(home.path(), Vec::new());
    let inspection = registry.inspect(&pkg).await.unwrap();
    assert!(inspection.grant.requires.is_empty());
    let installed = registry
        .install(&pkg, &inspection.approval_digest, Default::default())
        .await
        .unwrap();
    assert_eq!(installed.state, PluginState::Enabled);
    assert!(installed.requires.is_empty());

    let restarted = plugins(home.path(), Vec::new());
    restarted.restore().await;
    assert_eq!(state(&restarted, "grant-http"), Some(PluginState::Enabled));
}

#[tokio::test]
async fn a_package_built_for_another_api_line_is_refused_at_inspect() {
    let home = tempfile::tempdir().unwrap();
    let plugins = plugins(home.path(), Vec::new());
    let pkg = home.path().join("pkg");
    for (api, accepted) in [
        ("", true),
        ("api = \"0.1.0\"\n", true),
        ("api = \"0.2.0\"\n", false),
        ("api = \"0.0.9\"\n", false),
        ("api = \"1.0.0\"\n", false),
        ("api = \"0.1.0-rc.1\"\n", false),
        ("api = \"0.1.0+build\"\n", true),
    ] {
        package(&pkg, "ok", guest("hello"), api);
        let inspected = plugins.inspect(&pkg).await;
        match accepted {
            true => assert!(inspected.unwrap().warnings.is_empty(), "{api}"),
            false => assert!(
                matches!(
                    refusal(inspected),
                    PackageError::Grant(GrantError::Api { .. })
                ),
                "{api}"
            ),
        }
    }
    package(&pkg, "ok", guest("hello"), "api = \"latest\"\n");
    assert!(matches!(
        refusal(plugins.inspect(&pkg).await),
        PackageError::Manifest(ManifestError::Api(_))
    ));
}

async fn install(
    registry: &Plugins,
    pkg: &Path,
    id: &str,
    extra: String,
) -> Result<proto::plugin::PluginInfo, PluginError> {
    package(pkg, id, guest("hello"), &extra);
    let digest = registry.inspect(pkg).await?.approval_digest;
    registry.install(pkg, &digest, Default::default()).await
}

#[tokio::test]
async fn plugins_start_in_dependency_order_and_stop_with_what_depends_on_them() {
    let home = tempfile::tempdir().unwrap();
    let registry = plugins(home.path(), vec![Arc::new(Core)]);
    let pkg = home.path().join("pkg");
    let needs = |id: &str| format!("[dependencies]\n{id} = \"^0.1\"\n");
    package(&pkg, "app", guest("hello"), &needs("ghost"));
    assert!(matches!(
        registry.inspect(&pkg).await,
        Err(PluginError::MissingDependency { dependency, .. }) if dependency == "ghost"
    ));
    install(&registry, &pkg, "base", String::new())
        .await
        .unwrap();
    package(
        &pkg,
        "app",
        guest("hello"),
        "[dependencies]\nbase = \"^1.0\"\n",
    );
    assert!(
        matches!(
            registry.inspect(&pkg).await,
            Err(PluginError::MissingDependency { .. })
        ),
        "an installed version the range rejects is a missing dependency"
    );

    install(&registry, &pkg, "app", needs("base"))
        .await
        .unwrap();
    install(&registry, &pkg, "top", needs("app")).await.unwrap();
    assert_eq!(
        registry
            .list()
            .iter()
            .find(|p| p.id == "top")
            .unwrap()
            .requires,
        ["app"]
    );

    assert!(matches!(
        registry.disable("base", false).await,
        Err(PluginError::Required { dependents, .. }) if dependents == ["top", "app"]
    ));
    assert!(matches!(
        registry.remove("base").await,
        Err(PluginError::Required { dependents, .. }) if dependents == ["app"]
    ));
    let core = format!("[dependencies]\ncore = \"^{}\"\n", common::app::VERSION);
    install(&registry, &pkg, "uses-core", core).await.unwrap();
    assert_eq!(
        registry
            .list()
            .iter()
            .find(|p| p.id == "uses-core")
            .unwrap()
            .requires,
        ["core"]
    );
    assert!(
        matches!(
            registry.disable("core", false).await,
            Err(PluginError::Required { dependents, .. }) if dependents == ["uses-core"]
        ),
        "a built-in a plugin declares is needed like any other"
    );
    registry.disable("core", true).await.unwrap();
    assert_eq!(state(&registry, "uses-core"), Some(PluginState::Disabled));
    assert!(registry.enable("uses-core").await.is_err(), "core is off");
    registry.enable("core").await.unwrap();
    registry.enable("uses-core").await.unwrap();

    registry.disable("base", true).await.unwrap();
    for id in ["base", "app", "top"] {
        assert_eq!(state(&registry, id), Some(PluginState::Disabled), "{id}");
    }

    registry.enable("top").await.unwrap();
    for id in ["base", "app", "top"] {
        assert_eq!(state(&registry, id), Some(PluginState::Enabled), "{id}");
    }

    let restarted = plugins(home.path(), vec![Arc::new(Core)]);
    restarted.restore().await;
    for id in ["base", "app", "top"] {
        assert_eq!(
            state(&restarted, id),
            Some(PluginState::Enabled),
            "{id} starts after what it needs, whatever the order on disk"
        );
    }

    std::fs::remove_dir_all(home.path().join("plugins").join("base")).unwrap();
    let orphaned = plugins(home.path(), vec![Arc::new(Core)]);
    orphaned.restore().await;
    assert_eq!(
        state(&orphaned, "app"),
        Some(PluginState::MissingDependency)
    );
    assert!(orphaned.enable("app").await.is_err());
}
