//! Topics and hooks a built-in's package defines, against the test built-in
//! `probe` and a component that exports both: delivery honours the receiver's
//! `entries` scope and the wake-up budget; a hook folds over plugins in id
//! order and only a veto stops it.

use std::path::Path;
use std::sync::Arc;

use kernel::probe::{Hooked, Probe, ProbeCalls, Vetoed};
use kernel::{Chain, Limits, Plugins, Sandbox};
use proto::plugin::{Choice, EntryScope, PluginState};

#[allow(dead_code)]
mod fixture;
use fixture::{scratch, service_guest, Files};

const OPTIONAL_READ: &str = "[permissions.\"probe:read\"]\noptional = true\n";
const OPTIONAL_BOTH: &str =
    "[permissions.\"probe:read\"]\noptional = true\n[permissions.\"probe:entries\"]\noptional = true\n";

fn plugins(home: &Path) -> (Arc<Plugins>, Arc<Sandbox>) {
    plugins_with(home, Limits::default())
}

fn plugins_with(home: &Path, limits: Limits) -> (Arc<Plugins>, Arc<Sandbox>) {
    let sandbox = Sandbox::new(
        scratch().join("plugin-cache"),
        &scratch().join("plugin-cache.key"),
        limits,
    )
    .expect("sandbox")
    .with_services(Arc::new(Files(home.to_owned())));
    sandbox.set_calls(Arc::new(ProbeCalls::default()));
    let sandbox = Arc::new(sandbox);
    let plugins = Plugins::new(home, vec![Arc::new(Probe)], Some(sandbox.clone()))
        .unwrap()
        .with_developer(true);
    (Arc::new(plugins), sandbox)
}

async fn install_guest(
    plugins: &Plugins,
    home: &Path,
    id: &str,
    guest: &str,
    manifest_extra: &str,
    choice: Choice,
) {
    let dir = home.join("pkg").join(id);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("plugin.toml"),
        format!("id = \"{id}\"\nname = \"{id}\"\nversion = \"0.1.0\"\n{manifest_extra}"),
    )
    .unwrap();
    std::fs::copy(service_guest(guest), dir.join("plugin.wasm")).unwrap();
    let inspection = plugins.inspect(&dir).await.unwrap();
    assert_eq!(
        inspection.grant.requires,
        ["hestia:probe"],
        "exporting a package needs its provider, as importing does"
    );
    let info = plugins
        .install(&dir, &inspection.approval_digest, choice)
        .await
        .unwrap();
    assert_eq!(info.state, PluginState::Enabled);
}

async fn install(plugins: &Plugins, home: &Path, id: &str, entries: EntryScope) {
    let choice = Choice {
        entries,
        ..Choice::default()
    };
    install_guest(plugins, home, id, "events", OPTIONAL_READ, choice).await;
}

fn only(ids: &[&str]) -> EntryScope {
    EntryScope {
        all: false,
        ids: ids.iter().map(|id| id.to_string()).collect(),
    }
}

fn heard(home: &Path, id: &str) -> Vec<String> {
    heard_in(home, id, "heard")
}

fn heard_in(home: &Path, id: &str, file: &str) -> Vec<String> {
    std::fs::read_to_string(kernel::sandbox_root(home).join(id).join(file))
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

#[tokio::test]
async fn an_event_reaches_only_the_plugins_whose_scope_holds_its_entry() {
    let home = tempfile::tempdir().unwrap();
    let (plugins, _) = plugins(home.path());
    install(&plugins, home.path(), "ann", only(&["x"])).await;
    install(
        &plugins,
        home.path(),
        "bob",
        EntryScope {
            all: true,
            ids: Vec::new(),
        },
    )
    .await;
    install(&plugins, home.path(), "cyd", only(&["y"])).await;

    plugins.entry_opened("x");
    plugins.entry_opened("y");
    plugins.entry_opened("z");
    plugins.settle().await;
    assert_eq!(heard(home.path(), "ann"), ["x"]);
    assert_eq!(heard(home.path(), "bob"), ["x", "y", "z"]);
    assert_eq!(heard(home.path(), "cyd"), ["y"]);
}

#[tokio::test]
async fn events_over_the_budget_wait_for_a_refill_and_spare_the_wakeups() {
    let home = tempfile::tempdir().unwrap();
    let (plugins, sandbox) = plugins_with(
        home.path(),
        Limits::default().with_event_budget(30.0, 1000.0),
    );
    install(
        &plugins,
        home.path(),
        "bob",
        EntryScope {
            all: true,
            ids: Vec::new(),
        },
    )
    .await;

    for _ in 0..31 {
        plugins.entry_opened("x");
    }
    plugins.settle().await;
    assert_eq!(heard(home.path(), "bob").len(), 31, "none was dropped");
    assert!(
        sandbox.spend_wakeup("bob"),
        "events spend a budget of their own"
    );
}

#[tokio::test]
async fn a_plugin_hears_only_the_topics_whose_permission_it_holds_and_exports() {
    let home = tempfile::tempdir().unwrap();
    let (plugins, _) = plugins(home.path());
    let all = || EntryScope {
        all: true,
        ids: Vec::new(),
    };
    let choice = |optional: &[&str]| Choice {
        entries: all(),
        optional: optional.iter().map(|p| p.to_string()).collect(),
    };
    install_guest(
        &plugins,
        home.path(),
        "ann",
        "events",
        OPTIONAL_BOTH,
        choice(&[]),
    )
    .await;
    install_guest(
        &plugins,
        home.path(),
        "bob",
        "events",
        OPTIONAL_BOTH,
        choice(&["probe:read"]),
    )
    .await;
    install_guest(
        &plugins,
        home.path(),
        "cyd",
        "entries",
        OPTIONAL_READ,
        choice(&[]),
    )
    .await;

    let publish = |plugins: &Arc<Plugins>| {
        plugins.entry_opened("x");
        plugins.echoed("t");
    };
    let hears = |id: &str| {
        (
            heard(home.path(), id),
            heard_in(home.path(), id, "heard-echoed"),
        )
    };
    for _ in 0..3 {
        publish(&plugins);
        plugins.settle().await;
    }
    assert_eq!(hears("ann"), (vec![], vec![]), "both declined");
    assert_eq!(
        hears("bob"),
        (vec![], vec!["t".to_string(); 3]),
        "only the topic its permission gates"
    );
    assert_eq!(hears("cyd"), (vec![], vec![]), "it exports neither");
    assert!(plugins
        .list()
        .iter()
        .all(|plugin| plugin.id == "probe" || plugin.state == PluginState::Enabled));

    plugins
        .set_permission("ann", "probe:entries", true)
        .await
        .unwrap();
    publish(&plugins);
    plugins.settle().await;
    assert_eq!(hears("ann"), (vec!["x".to_string()], vec![]));
}

#[tokio::test]
async fn a_hook_folds_in_id_order_skips_a_plugin_that_fails_and_stops_at_a_veto() {
    let home = tempfile::tempdir().unwrap();
    let (plugins, _) = plugins(home.path());
    install(&plugins, home.path(), "bob", EntryScope::default()).await;
    install(&plugins, home.path(), "ann", EntryScope::default()).await;
    let chain = Chain::default();

    assert_eq!(
        plugins.before_echo("hi", &chain).await,
        Hooked {
            text: "hi!".into(),
            vetoed: Some(Vetoed {
                plugin: "bob".into(),
                reason: "no shouting".into()
            })
        },
        "ann runs first and bob sees her text"
    );
    for skipped in ["fail", "trap"] {
        assert_eq!(
            plugins.before_echo(skipped, &chain).await,
            Hooked {
                text: skipped.into(),
                vetoed: None
            }
        );
    }
    assert!(plugins
        .list()
        .iter()
        .all(|plugin| plugin.id == "probe" || plugin.state == PluginState::Enabled));
}

#[tokio::test]
async fn a_hook_does_not_run_a_plugin_already_in_the_chain_that_triggered_it() {
    let home = tempfile::tempdir().unwrap();
    let (plugins, _) = plugins(home.path());
    install(&plugins, home.path(), "ann", EntryScope::default()).await;
    install(&plugins, home.path(), "bob", EntryScope::default()).await;

    let from_ann = Chain::default().enter("ann").unwrap();
    assert_eq!(
        plugins.before_echo("hi", &from_ann).await,
        Hooked {
            text: "hi!".into(),
            vetoed: None
        },
        "ann is refused as a re-entry and only bob runs"
    );
}
