//! The plugin registry driven by test built-ins. Production registers none yet,
//! so these live here rather than in the release binary.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use kernel::{Builtin, Busy, Descriptor, PluginError, Plugins};
use proto::plugin::PluginState;

type Log = Arc<Mutex<Vec<&'static str>>>;

struct Fake {
    descriptor: Descriptor,
    log: Log,
    busy: AtomicBool,
    broken: bool,
}

fn fake(id: &'static str, requires: &'static [&'static str], log: &Log) -> Fake {
    Fake {
        descriptor: Descriptor {
            id,
            name: id,
            provides: None,
            requires,
        },
        log: log.clone(),
        busy: AtomicBool::new(false),
        broken: false,
    }
}

impl Builtin for Fake {
    fn descriptor(&self) -> Descriptor {
        self.descriptor
    }

    fn enable(&self) -> anyhow::Result<()> {
        if self.broken {
            anyhow::bail!("cannot start");
        }
        self.log.lock().unwrap().push(self.descriptor.id);
        Ok(())
    }

    fn disable(&self) -> Result<(), Busy> {
        assert!(!self.broken, "a plugin that never enabled is not disabled");
        match self.busy.load(Ordering::SeqCst) {
            true => Err(Busy),
            false => Ok(()),
        }
    }
}

fn open(home: &std::path::Path, builtins: Vec<Arc<dyn Builtin>>) -> Result<Plugins, PluginError> {
    Plugins::new(home, builtins, None)
}

fn states(plugins: &Plugins) -> Vec<(String, PluginState)> {
    plugins
        .list()
        .into_iter()
        .map(|p| (p.id, p.state))
        .collect()
}

#[tokio::test]
async fn enable_and_disable_follow_dependencies_and_persist() {
    let dir = tempfile::tempdir().unwrap();
    let log = Log::default();
    let build = |log: &Log| -> Vec<Arc<dyn Builtin>> {
        vec![
            Arc::new(fake("extra", &["core"], log)),
            Arc::new(fake("core", &[], log)),
        ]
    };

    let plugins = open(dir.path(), build(&log)).unwrap();
    assert_eq!(*log.lock().unwrap(), ["core", "extra"]);
    assert!(plugins.is_enabled("extra"));

    let error = plugins.disable("core").await.unwrap_err();
    assert!(matches!(&error, PluginError::Required { dependents, .. } if dependents == &["extra"]));

    plugins.disable("extra").await.unwrap();
    plugins.disable("core").await.unwrap();
    assert!(!plugins.is_enabled("core"));

    log.lock().unwrap().clear();
    plugins.enable("extra").await.unwrap();
    assert_eq!(*log.lock().unwrap(), ["core", "extra"]);

    plugins.disable("extra").await.unwrap();
    log.lock().unwrap().clear();
    let restarted = open(dir.path(), build(&log)).unwrap();
    assert_eq!(*log.lock().unwrap(), ["core"]);
    assert_eq!(
        states(&restarted),
        [
            ("core".into(), PluginState::Enabled),
            ("extra".into(), PluginState::Disabled),
        ]
    );

    assert!(matches!(
        restarted.enable("nope").await,
        Err(PluginError::NotFound(_))
    ));
}

#[tokio::test]
async fn start_up_never_enables_what_the_user_turned_off() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("plugins.json");
    let log = Log::default();
    let build = |log: &Log| -> Vec<Arc<dyn Builtin>> {
        vec![
            Arc::new(fake("core", &[], log)),
            Arc::new(fake("extra", &["core"], log)),
        ]
    };
    std::fs::write(&path, r#"{"disabled":["core"]}"#).unwrap();

    let plugins = open(dir.path(), build(&log)).unwrap();
    assert!(log.lock().unwrap().is_empty());
    assert_eq!(
        states(&plugins),
        [
            ("core".into(), PluginState::Disabled),
            ("extra".into(), PluginState::Failed),
        ]
    );

    plugins.disable("extra").await.unwrap();
    let restarted = open(dir.path(), build(&log)).unwrap();
    assert!(log.lock().unwrap().is_empty());
    assert_eq!(
        states(&restarted),
        [
            ("core".into(), PluginState::Disabled),
            ("extra".into(), PluginState::Disabled),
        ]
    );
}

#[tokio::test]
async fn a_busy_plugin_stays_enabled_and_a_failing_one_takes_its_dependents_down() {
    let dir = tempfile::tempdir().unwrap();
    let log = Log::default();
    let busy = Arc::new(fake("busy", &[], &log));
    busy.busy.store(true, Ordering::SeqCst);
    let mut broken = fake("broken", &[], &log);
    broken.broken = true;
    let plugins = open(
        dir.path(),
        vec![
            busy,
            Arc::new(broken),
            Arc::new(fake("child", &["broken"], &log)),
        ],
    )
    .unwrap();

    assert!(matches!(
        plugins.disable("busy").await,
        Err(PluginError::Busy(_))
    ));
    assert!(plugins.is_enabled("busy"));
    assert_eq!(
        states(&plugins),
        [
            ("busy".into(), PluginState::Enabled),
            ("broken".into(), PluginState::Failed),
            ("child".into(), PluginState::Failed),
        ]
    );

    plugins.disable("broken").await.unwrap();
    assert_eq!(states(&plugins)[1].1, PluginState::Disabled);
}
