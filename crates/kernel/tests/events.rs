//! Topics and hooks a built-in's package defines, against the test built-in
//! `probe` and a component that exports both: delivery honours the receiver's
//! `entries` scope and the wake-up budget; a hook folds over plugins in id
//! order and only a veto stops it.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use kernel::probe::{Hooked, Probe, ProbeCalls, Vetoed};
use kernel::{Limits, Plugins, Sandbox, Services};
use proto::plugin::{Choice, EntryScope, PluginState};

#[allow(dead_code)]
mod fixture;
use fixture::{scratch, service_guest};

struct Files(PathBuf);

impl Services for Files {
    fn data_dir(&self, plugin: &str) -> Option<PathBuf> {
        Some(kernel::sandbox_root(&self.0).join(plugin))
    }

    fn state_dir(&self, plugin: &str) -> Option<PathBuf> {
        Some(self.0.join("plugins").join(plugin))
    }
}

fn plugins(home: &Path) -> Arc<Plugins> {
    let sandbox = Sandbox::new(
        scratch().join("plugin-cache"),
        &scratch().join("plugin-cache.key"),
        Limits::default(),
    )
    .expect("sandbox")
    .with_services(Arc::new(Files(home.to_owned())));
    sandbox.set_calls(Arc::new(ProbeCalls::default()));
    Arc::new(
        Plugins::new(home, vec![Arc::new(Probe)], Some(Arc::new(sandbox)))
            .unwrap()
            .with_developer(true),
    )
}

async fn install(plugins: &Plugins, home: &Path, id: &str, entries: EntryScope) {
    let dir = home.join("pkg").join(id);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("plugin.toml"),
        format!("id = \"{id}\"\nname = \"{id}\"\nversion = \"0.1.0\"\n"),
    )
    .unwrap();
    std::fs::copy(service_guest("events"), dir.join("plugin.wasm")).unwrap();
    let inspection = plugins.inspect(&dir).await.unwrap();
    let permissions: Vec<_> = inspection
        .grant
        .permissions
        .iter()
        .map(|p| (p.permission.as_str(), p.entries))
        .collect();
    assert_eq!(
        permissions,
        [("probe:entries", true)],
        "exporting a topic needs its read permission"
    );
    let choice = Choice {
        entries,
        ..Choice::default()
    };
    let info = plugins
        .install(&dir, &inspection.approval_digest, choice)
        .await
        .unwrap();
    assert_eq!(info.state, PluginState::Enabled);
}

fn only(ids: &[&str]) -> EntryScope {
    EntryScope {
        all: false,
        ids: ids.iter().map(|id| id.to_string()).collect(),
    }
}

fn heard(home: &Path, id: &str) -> Vec<String> {
    std::fs::read_to_string(kernel::sandbox_root(home).join(id).join("heard"))
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

#[tokio::test]
async fn an_event_reaches_only_the_plugins_whose_scope_holds_its_entry_and_spends_their_wakeups() {
    let home = tempfile::tempdir().unwrap();
    let plugins = plugins(home.path());
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

    plugins.entry_opened("x").await.unwrap();
    plugins.entry_opened("y").await.unwrap();
    assert_eq!(heard(home.path(), "ann"), ["x"]);
    assert_eq!(heard(home.path(), "bob"), ["x", "y"]);
    assert_eq!(heard(home.path(), "cyd"), ["y"]);

    for _ in 0..40 {
        plugins.entry_opened("z").await.unwrap();
    }
    let burst = heard(home.path(), "bob").len() - 2;
    assert!(
        (28..40).contains(&burst),
        "{burst} of 40 events were delivered past a budget of 30"
    );
    assert_eq!(heard(home.path(), "ann"), ["x"], "z is outside its scope");
}

#[tokio::test]
async fn a_hook_folds_in_id_order_skips_a_plugin_that_fails_and_stops_at_a_veto() {
    let home = tempfile::tempdir().unwrap();
    let plugins = plugins(home.path());
    install(&plugins, home.path(), "bob", EntryScope::default()).await;
    install(&plugins, home.path(), "ann", EntryScope::default()).await;

    assert_eq!(
        plugins.before_echo("hi").await,
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
            plugins.before_echo(skipped).await,
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
