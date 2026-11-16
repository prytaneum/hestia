//! Manifest, derived grant, install and start-up of WASM plugins, against real
//! components built from `tests/guest/` and `tests/guest-services/`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use kernel::{
    denials, Builtin, Busy, Descriptor, GrantError, GuestError, Limits, ManifestError,
    PackageError, Permission, PluginError, Plugins, Sandbox, SandboxError, Services, Shape,
    ShapeError, Table,
};
use proto::plugin::{PluginKind, PluginState, ScopeKind, Tier};
use sha2::{Digest, Sha256};

mod fixture;
use fixture::{guest, scratch, service_guest, wasm};

/// Mounts `/data` and keeps settings and secrets beside it, under one home.
struct Files(PathBuf);

impl Services for Files {
    fn data_dir(&self, plugin: &str) -> Option<PathBuf> {
        Some(self.0.join("plugin-data").join(plugin))
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

fn gated() -> Table {
    Table::new(
        vec![
            ("hestia:kernel/types", None),
            ("hestia:kernel/log", Some("log:sink")),
            ("hestia:kernel/app", Some("app:read")),
            ("hestia:kernel/lifecycle", None),
        ],
        vec![
            Permission {
                name: "app:read",
                tier: Tier::Unrestricted,
                scope: None,
                prompt: "Read the app info",
            },
            Permission {
                name: "log:sink",
                tier: Tier::Sandboxed,
                scope: Some(ScopeKind::Hosts),
                prompt: "Send its log lines",
            },
        ],
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

fn plugins(home: &Path, table: Table, builtins: Vec<Arc<dyn Builtin>>) -> Plugins {
    Plugins::new(home, builtins, Some(sandbox(home)), table).unwrap()
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
    let plugins = plugins(home.path(), gated(), Vec::new());
    let pkg = home.path().join("pkg");
    let hosts = |hosts: &str| format!("[permissions.\"log:sink\"]\nhosts = [{hosts}]\n");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::copy(guest("hello"), pkg.join("plugin.wasm")).unwrap();

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
                "{}[permissions.\"app:read\"]\nhosts = [\"a.example\"]\n",
                hosts("\"a.example\"")
            ),
            |e| matches!(e, PackageError::Grant(GrantError::ScopeNotNeeded(_))),
        ),
        (
            "ok",
            "[permissions.\"log:sink\"]\npaths = [\"x\"]\n".into(),
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
    let plugins = plugins(home.path(), gated(), Vec::new());
    let pkg = home.path().join("pkg");
    let scoped = |hosts: &str| format!("[permissions.\"log:sink\"]\nhosts = [{hosts}]\n");

    package(
        &pkg,
        "grant-hello",
        guest("hello"),
        &scoped("\"a.example\""),
    );
    let inspection = plugins.inspect(&pkg).await.unwrap();
    let names: Vec<_> = inspection
        .grant
        .permissions
        .iter()
        .map(|p| p.permission.as_str())
        .collect();
    assert_eq!(names, ["app:read", "log:sink"]);
    assert_eq!(inspection.grant.permissions[0].tier, Tier::Unrestricted);
    let scope = inspection.grant.permissions[1].scope.as_ref().unwrap();
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
        state(&plugins, "grant-hello").is_none(),
        "inspecting installs nothing"
    );

    let reworded = Table::new(
        vec![
            ("hestia:kernel/types", None),
            ("hestia:kernel/log", Some("log:sink")),
            ("hestia:kernel/app", Some("app:read")),
            ("hestia:kernel/lifecycle", None),
        ],
        vec![
            Permission {
                name: "app:read",
                tier: Tier::Unrestricted,
                scope: None,
                prompt: "Hestia's new wording",
            },
            Permission {
                name: "log:sink",
                tier: Tier::Sandboxed,
                scope: Some(ScopeKind::Hosts),
                prompt: "Other wording too",
            },
        ],
    );
    let after_update = self::plugins(home.path(), reworded, Vec::new());
    assert_eq!(
        after_update.inspect(&pkg).await.unwrap().approval_digest,
        inspection.approval_digest,
        "rewording Hestia's prompt keeps every approval"
    );

    for widened in [
        scoped("\"b.example\""),
        format!("{}# a comment\n", scoped("\"a.example\"")),
    ] {
        package(&pkg, "grant-hello", guest("hello"), &widened);
        let changed = plugins.inspect(&pkg).await.unwrap();
        assert_ne!(changed.approval_digest, inspection.approval_digest);
    }
}

#[tokio::test]
async fn an_ungranted_interface_answers_denied_naming_its_permission() {
    let home = tempfile::tempdir().unwrap();
    let sandbox = sandbox(home.path());

    let tour = wasm(service_guest("tour"));
    let shape = Shape::read(&tour).unwrap();
    let denied = denials(&Table::kernel(), &shape, &[]);
    assert_eq!(denied.values().collect::<Vec<_>>(), ["secrets"]);
    let opened = sandbox.open("grant-denied", tour).await.unwrap();
    let loaded = sandbox.link(&opened, &denied).unwrap();
    let data = home.path().join("plugin-data/grant-denied");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(data.join("mode"), "secret").unwrap();
    match sandbox.init(&loaded).await {
        Err(SandboxError::Guest(GuestError::Denied(permission))) => {
            assert_eq!(permission, "secrets")
        }
        other => panic!("expected denied(secrets), got {other:?}"),
    }

    let hello = wasm(guest("hello"));
    let shape = Shape::read(&hello).unwrap();
    let denied = denials(&gated(), &shape, &["log:sink"]);
    let opened = sandbox.open("grant-undeniable", hello).await.unwrap();
    assert!(
        matches!(sandbox.link(&opened, &denied), Err(SandboxError::Load(_))),
        "a function with no error to answer cannot be denied, only refused"
    );
}

struct Core(Option<&'static str>);

impl Builtin for Core {
    fn descriptor(&self) -> Descriptor {
        Descriptor {
            id: "core",
            name: "core",
            provides: self.0,
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
async fn a_package_another_plugin_must_provide_installs_only_beside_it() {
    let home = tempfile::tempdir().unwrap();
    let pkg = home.path().join("pkg");
    package(&pkg, "grant-needs", guest("hello"), "");
    let unaware = || Table::new(Vec::new(), Vec::new());

    let alone = plugins(home.path(), unaware(), Vec::new());
    let inspection = alone.inspect(&pkg).await.unwrap();
    assert_eq!(inspection.grant.requires, ["hestia:kernel"]);
    assert!(matches!(
        alone.install(&pkg, &inspection.approval_digest).await,
        Err(PluginError::Package(PackageError::Unprovided(_)))
    ));

    let provided = plugins(
        home.path(),
        unaware(),
        vec![Arc::new(Core(Some("hestia:kernel")))],
    );
    let installed = provided
        .install(&pkg, &inspection.approval_digest)
        .await
        .unwrap();
    assert_eq!(installed.state, PluginState::Enabled);
}

#[tokio::test]
async fn an_installed_plugin_survives_restarts_and_removal_takes_its_cache() {
    let home = tempfile::tempdir().unwrap();
    let pkg = home.path().join("pkg");
    let build = || plugins(home.path(), Table::kernel(), vec![Arc::new(Core(None))]);
    let registry = build();

    package(&pkg, "grant-keep", guest("hello"), "");
    let approved = registry.inspect(&pkg).await.unwrap().approval_digest;
    assert!(matches!(
        registry.install(&pkg, "stale").await,
        Err(PluginError::Changed(_))
    ));
    let (first, second) = tokio::join!(
        registry.install(&pkg, &approved),
        registry.install(&pkg, &approved)
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
        registry.install(&pkg, &digest).await,
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
    restarted.disable("grant-keep").await.unwrap();
    let again = build();
    again.restore().await;
    assert_eq!(state(&again, "grant-keep"), Some(PluginState::Disabled));
    assert_eq!(
        again.enable("grant-keep").await.unwrap().state,
        PluginState::Enabled
    );

    let blind = Plugins::new(home.path(), Vec::new(), None, Table::kernel()).unwrap();
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
    tampered.disable("grant-keep").await.unwrap();
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
    let registry = plugins(home.path(), Table::kernel(), Vec::new());
    for (id, component) in [
        ("grant-asks", service_guest("tour")),
        ("grant-quiet", guest("hello")),
    ] {
        let pkg = home.path().join(id);
        package(&pkg, id, component, "");
        let data = home.path().join("plugin-data").join(id);
        std::fs::create_dir_all(&data).unwrap();
        std::fs::write(data.join("mode"), "free").unwrap();
        let inspection = registry.inspect(&pkg).await.unwrap();
        registry
            .install(&pkg, &inspection.approval_digest)
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
    let registry = plugins(home.path(), Table::kernel(), Vec::new());

    package(&pkg, "grant-trap", guest("trap"), "");
    let approved = registry.inspect(&pkg).await.unwrap().approval_digest;
    let installed = registry.install(&pkg, &approved).await.unwrap();
    assert_eq!(installed.state, PluginState::Failed);
    assert!(matches!(
        registry.enable("grant-trap").await,
        Err(PluginError::Failed { .. })
    ));
    assert_eq!(state(&registry, "grant-trap"), Some(PluginState::Failed));
    assert!(registry.enable("grant-trap").await.is_err());
    assert_eq!(state(&registry, "grant-trap"), Some(PluginState::Disabled));

    let restarted = plugins(home.path(), Table::kernel(), Vec::new());
    restarted.restore().await;
    assert_eq!(state(&restarted, "grant-trap"), Some(PluginState::Disabled));
}
