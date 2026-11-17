//! A built-in exposes a typed package: the test built-in `probe`, driven by a
//! component that imports it and fills its extension point.

use std::path::Path;
use std::sync::Arc;

use kernel::probe::{Probe, ProbeCalls, Recorded};
use kernel::{
    denials, Chain, ChainError, GuestError, Limits, PackageError, PluginError, Plugins, Sandbox,
    SandboxError, Shape, Table,
};
use proto::plugin::PluginState;

mod fixture;
use fixture::{guest, scratch, service_guest, wasm};

fn sandbox(calls: &Arc<ProbeCalls>) -> Arc<Sandbox> {
    let sandbox = Sandbox::new(
        scratch().join("plugin-cache"),
        &scratch().join("plugin-cache.key"),
        Limits::default(),
    )
    .expect("sandbox");
    sandbox.set_calls(calls.clone());
    Arc::new(sandbox)
}

fn package(dir: &Path, id: &str, component: std::path::PathBuf) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(
        dir.join("plugin.toml"),
        format!("id = \"{id}\"\nname = \"{id}\"\nversion = \"0.1.0\"\n"),
    )
    .unwrap();
    std::fs::copy(component, dir.join("plugin.wasm")).unwrap();
}

#[tokio::test]
async fn a_plugin_calls_a_built_ins_package_and_fills_its_extension_point() {
    let home = tempfile::tempdir().unwrap();
    let calls = Arc::new(ProbeCalls::default());
    let sandbox = sandbox(&calls);
    let plugins = Plugins::new(
        home.path(),
        vec![Arc::new(Probe)],
        Some(sandbox.clone()),
        Table::kernel(),
    )
    .unwrap()
    .with_developer(true);

    let pkg = home.path().join("pkg");
    package(&pkg, "consumer", service_guest("consumer"));
    let inspection = plugins.inspect(&pkg).await.unwrap();
    assert_eq!(inspection.grant.requires, ["hestia:probe"]);
    let permissions: Vec<_> = inspection
        .grant
        .permissions
        .iter()
        .map(|p| p.permission.as_str())
        .collect();
    assert_eq!(permissions, ["probe:read"]);

    plugins.disable("probe").await.unwrap();
    assert!(matches!(
        plugins.install(&pkg, &inspection.approval_digest, Default::default()).await,
        Err(PluginError::Package(PackageError::Unprovided(package))) if package == "hestia:probe"
    ));
    plugins.enable("probe").await.unwrap();
    let installed = plugins
        .install(&pkg, &inspection.approval_digest, Default::default())
        .await
        .unwrap();
    assert_eq!(installed.state, PluginState::Enabled);
    assert!(matches!(
        plugins.disable("probe").await,
        Err(PluginError::Required { dependents, .. }) if dependents == ["consumer"]
    ));
    let without_provider = Plugins::new(
        home.path(),
        Vec::new(),
        Some(sandbox.clone()),
        Table::kernel(),
    )
    .unwrap()
    .with_developer(true);
    without_provider.restore().await;
    assert_eq!(
        without_provider
            .list()
            .iter()
            .find(|p| p.id == "consumer")
            .map(|p| p.state),
        Some(PluginState::Failed),
        "a plugin whose provider is missing never starts"
    );

    let component = wasm(service_guest("consumer"));
    let shape = Shape::read(&component).unwrap();
    let opened = sandbox.open("consumer", component.clone()).await.unwrap();

    let granted = denials(plugins.table(), &shape, &["probe:read"]);
    let loaded = sandbox
        .link(&opened, &granted, &Default::default(), Default::default())
        .unwrap();
    let filled = sandbox.fill(&loaded, Chain::default(), "hi").await.unwrap();
    assert_eq!(filled.as_deref(), Some("hi/2/HI/2"));
    let seen = |channel| Recorded {
        chain: vec!["consumer".into()],
        channel,
    };
    assert_eq!(calls.seen(), [seen("probe.echo"), seen("probe.shout")]);

    let beside = Chain::default().enter("consumer").unwrap();
    assert!(matches!(
        sandbox.fill(&loaded, beside, "hi").await,
        Err(SandboxError::Chain(ChainError::Reentered(_)))
    ));
    assert_eq!(
        calls.seen().len(),
        2,
        "a plugin already in the chain is never run again"
    );

    let withheld = denials(plugins.table(), &shape, &[]);
    let loaded = sandbox
        .link(&opened, &withheld, &Default::default(), Default::default())
        .unwrap();
    match sandbox.fill(&loaded, Chain::default(), "hi").await {
        Err(SandboxError::Guest(GuestError::Denied(permission))) => {
            assert_eq!(permission, "probe:read")
        }
        other => panic!("expected denied(probe:read), got {other:?}"),
    }

    let hello = sandbox.load("hello", wasm(guest("hello"))).await.unwrap();
    assert_eq!(
        sandbox.fill(&hello, Chain::default(), "hi").await.unwrap(),
        None,
        "a component without the export does not provide it"
    );
}
