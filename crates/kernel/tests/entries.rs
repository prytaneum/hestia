//! What the user decides within a grant: optional permissions (declined unless
//! granted, granted and revoked later) and the entries scope the registry keeps,
//! against the test built-in `probe` and a component that reports what it holds.
//! The gate over the entries a call touches is the daemon's router, tested there.

use std::path::Path;
use std::sync::Arc;

use kernel::probe::{Probe, ProbeCalls};
use kernel::{
    GrantError, GuestError, Limits, PackageError, PluginError, Plugins, Sandbox, SandboxError,
};
use proto::error::{EntryKind, EntryTarget};
use proto::plugin::{Choice, EntryScope};

#[allow(dead_code)]
mod fixture;
use fixture::{scratch, service_guest};

const OPTIONAL_READ: &str = "[permissions.\"probe:read\"]\noptional = true\n";

fn plugins(home: &Path) -> Plugins {
    plugins_over(home, Arc::new(ProbeCalls::default()))
}

fn plugins_over(home: &Path, calls: Arc<ProbeCalls>) -> Plugins {
    let sandbox = Sandbox::new(
        scratch().join("plugin-cache"),
        &scratch().join("plugin-cache.key"),
        Limits::default(),
    )
    .expect("sandbox");
    sandbox.set_calls(calls);
    Plugins::new(home, vec![Arc::new(Probe)], Some(Arc::new(sandbox)))
        .unwrap()
        .with_developer(true)
}

fn package(home: &Path, manifest_extra: &str) -> std::path::PathBuf {
    let dir = home.join("pkg");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("plugin.toml"),
        format!("id = \"acting\"\nname = \"acting\"\nversion = \"0.1.0\"\n{manifest_extra}"),
    )
    .unwrap();
    std::fs::copy(service_guest("entries"), dir.join("plugin.wasm")).unwrap();
    dir
}

fn only(ids: &[&str]) -> EntryScope {
    EntryScope {
        all: false,
        ids: ids.iter().map(|id| id.to_string()).collect(),
    }
}

async fn fill(plugins: &Plugins, text: &str) -> Result<String, String> {
    match plugins.fill("acting", text).await {
        Ok(text) => Ok(text.expect("the plugin fills the extension point")),
        Err(PluginError::Call {
            source: SandboxError::Guest(GuestError::Denied(permission)),
            ..
        }) => Err(format!("denied({permission})")),
        Err(other) => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn an_optional_permission_arrives_declined_and_is_granted_and_revoked_later() {
    let home = tempfile::tempdir().unwrap();
    let registry = plugins(home.path());
    let dir = package(home.path(), OPTIONAL_READ);

    let inspection = registry.inspect(&dir).await.unwrap();
    let marks: Vec<_> = inspection
        .grant
        .permissions
        .iter()
        .map(|p| (p.permission.as_str(), p.optional, p.entries))
        .collect();
    assert_eq!(
        marks,
        [("probe:entries", false, true), ("probe:read", true, false)]
    );
    let digest = &inspection.approval_digest;

    let not_optional = Choice {
        optional: vec!["probe:entries".into()],
        ..Choice::default()
    };
    assert!(matches!(
        registry.install(&dir, digest, not_optional).await,
        Err(PluginError::Package(PackageError::Grant(
            GrantError::NotOptional(_)
        )))
    ));

    registry
        .install(&dir, digest, Choice::default())
        .await
        .unwrap();
    assert_eq!(
        fill(&registry, "granted:probe:entries").await.unwrap(),
        "true"
    );
    assert_eq!(
        fill(&registry, "granted:probe:read").await.unwrap(),
        "false"
    );
    assert_eq!(
        fill(&registry, "echo:hi").await.unwrap_err(),
        "denied(probe:read)"
    );

    assert!(matches!(
        registry
            .set_permission("acting", "probe:entries", false)
            .await,
        Err(PluginError::Package(PackageError::Grant(
            GrantError::NotOptional(_)
        )))
    ));
    registry
        .set_permission("acting", "probe:read", true)
        .await
        .unwrap();
    assert_eq!(fill(&registry, "granted:probe:read").await.unwrap(), "true");
    assert_eq!(fill(&registry, "echo:hi").await.unwrap(), "hi");

    let restarted = plugins(home.path());
    restarted.restore().await;
    assert_eq!(
        fill(&restarted, "echo:hi").await.unwrap(),
        "hi",
        "a grant survives a restart"
    );
    restarted
        .set_permission("acting", "probe:read", false)
        .await
        .unwrap();
    assert_eq!(
        fill(&restarted, "echo:hi").await.unwrap_err(),
        "denied(probe:read)"
    );
}

#[tokio::test]
async fn the_entries_scope_is_what_the_user_chose_plus_what_the_plugin_created_across_restarts() {
    let home = tempfile::tempdir().unwrap();
    let registry = plugins(home.path());
    let dir = package(home.path(), "");
    let digest = registry.inspect(&dir).await.unwrap().approval_digest;

    let chosen = Choice {
        entries: only(&["a"]),
        ..Choice::default()
    };
    registry.install(&dir, &digest, chosen).await.unwrap();
    assert_eq!(fill(&registry, "scope").await.unwrap(), "false:a");
    assert_eq!(registry.refused_entry("acting", &["a".into()]), None);
    assert_eq!(
        registry.refused_entry("acting", &["a".into(), "b".into()]),
        Some("b".into())
    );
    assert_eq!(
        registry.refused_entry("nobody", &["a".into()]),
        Some("a".into()),
        "a plugin the registry does not list reaches nothing"
    );

    registry.entries_created("acting", &["c".into()]).await;
    assert_eq!(fill(&registry, "scope").await.unwrap(), "false:a,c");

    let restarted = plugins(home.path());
    restarted.restore().await;
    assert_eq!(
        fill(&restarted, "scope").await.unwrap(),
        "false:a,c",
        "a created entry stays in scope across a restart"
    );

    let info = restarted.set_entries("acting", only(&["z"])).await.unwrap();
    assert_eq!(info.entries, Some(only(&["z"])));
    let everything = EntryScope {
        all: true,
        ids: vec!["ignored".into()],
    };
    restarted.set_entries("acting", everything).await.unwrap();
    assert_eq!(fill(&restarted, "scope").await.unwrap(), "true:");

    let again = plugins(home.path());
    again.restore().await;
    assert_eq!(
        fill(&again, "scope").await.unwrap(),
        "true:",
        "a scope the user changed is kept"
    );
}

#[tokio::test]
async fn an_adapter_states_what_a_call_touches_and_what_it_made_whether_a_call_or_a_job() {
    let home = tempfile::tempdir().unwrap();
    let calls = Arc::new(ProbeCalls::default());
    let registry = plugins_over(home.path(), calls.clone());
    let dir = package(home.path(), "");
    let digest = registry.inspect(&dir).await.unwrap().approval_digest;
    registry
        .install(&dir, &digest, Choice::default())
        .await
        .unwrap();

    assert_eq!(fill(&registry, "open:a").await.unwrap(), "a");
    assert_eq!(fill(&registry, "create:b").await.unwrap(), "b");
    assert_eq!(fill(&registry, "build:c").await.unwrap(), "c");
    assert_eq!(fill(&registry, "echo:hi").await.unwrap(), "hi");

    let server = |id: &str| EntryTarget::new(EntryKind::Server, id);
    assert_eq!(calls.touched(), [server("a")]);
    assert_eq!(calls.created(), [server("b"), server("c")]);
}
