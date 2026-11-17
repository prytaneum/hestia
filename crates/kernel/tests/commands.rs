//! Commands, schedules and background work of WASM plugins, against the
//! `tour` component built from `tests/guest-services/`.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use kernel::{
    Answer, GrantError, GuestError, Invocation, JobOutcome, Limits, ManifestError, PackageError,
    PluginError, Plugins, Sandbox, SandboxError, Services,
};
use proto::plugin::ArgKind;

#[allow(dead_code)]
mod fixture;
use fixture::{guest, scratch, service_guest};

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn plugins(home: &Path, host: Arc<Host>) -> Plugins {
    let limits = Limits::default().with_cancel_grace(Duration::from_millis(200));
    let sandbox = Sandbox::new(
        scratch().join("plugin-cache"),
        &scratch().join("plugin-cache.key"),
        limits,
    )
    .unwrap()
    .with_services(host);
    Plugins::new(home, Vec::new(), Some(Arc::new(sandbox)))
        .unwrap()
        .with_developer(true)
}

/// Mounts `/data` and records what the plugin asked of the host.
struct Host {
    home: PathBuf,
    seen: Mutex<Vec<String>>,
}

impl Host {
    fn new(home: &Path) -> Self {
        Host {
            home: home.to_owned(),
            seen: Mutex::default(),
        }
    }

    fn saw(&self, prefix: &str) -> Vec<String> {
        let seen = self.seen.lock().unwrap();
        seen.iter()
            .filter(|l| l.starts_with(prefix))
            .cloned()
            .collect()
    }

    fn record(&self, line: String) {
        self.seen.lock().unwrap().push(line);
    }
}

impl Services for Host {
    fn data_dir(&self, plugin: &str) -> Option<PathBuf> {
        Some(self.home.join("plugin-data").join(plugin))
    }

    fn state_dir(&self, plugin: &str) -> Option<PathBuf> {
        Some(self.home.join("plugins").join(plugin))
    }

    fn confirm<'a>(
        &'a self,
        plugin: &'a str,
        _job: Option<&'a str>,
        message: &'a str,
    ) -> Answer<'a> {
        self.record(format!("confirm {plugin}: {message}"));
        Box::pin(async { true })
    }

    fn open_url(&self, plugin: &str, _job: Option<&str>, url: &str) -> bool {
        self.record(format!("url {plugin}: {url}"));
        true
    }

    fn progress(&self, plugin: &str, job: &str, current: u64, total: u64, _message: &str) {
        self.record(format!("progress {plugin} {job}: {current}/{total}"));
    }

    fn start_job(&self, plugin: &str, command: &str, _args: &str) -> Result<String, GuestError> {
        let id = format!("job-{}", self.saw("started").len() + 1);
        self.record(format!("started {plugin} {command} {id}"));
        Ok(id)
    }
}

const MANIFEST: &str = r#"
[[commands]]
name = "echo"
args = [{ name = "name", kind = "string", required = true }, { name = "times", kind = "integer" }]
[[commands]]
name = "ask"
[[commands]]
name = "work"
[[commands]]
name = "spawn"
[[commands]]
name = "flood"
[[commands]]
name = "wait"
[[commands]]
name = "trap"
[[schedules]]
id = "nightly"
every = "1m"
"#;

fn package(dir: &Path, id: &str, component: PathBuf, extra: &str) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(
        dir.join("plugin.toml"),
        format!("id = \"{id}\"\nname = \"{id}\"\nversion = \"0.1.0\"\n{extra}"),
    )
    .unwrap();
    std::fs::copy(component, dir.join("plugin.wasm")).unwrap();
}

fn ran(home: &Path, file: &str) -> String {
    std::fs::read_to_string(home.join("plugin-data/tour").join(file)).unwrap_or_default()
}

fn reached(error: PluginError) -> SandboxError {
    match error {
        PluginError::Call { source, .. } => source,
        other => panic!("expected the plugin to answer, got {other:?}"),
    }
}

#[tokio::test]
async fn a_plugin_runs_declared_commands_for_a_person_and_wakes_itself_within_its_budget() {
    let home = tempfile::tempdir().unwrap();
    let host = Arc::new(Host::new(home.path()));
    let plugins = Arc::new(plugins(home.path(), host.clone()));

    let pkg = home.path().join("pkg");
    package(&pkg, "tour", service_guest("tour"), MANIFEST);
    std::fs::create_dir_all(home.path().join("plugin-data/tour")).unwrap();
    std::fs::write(home.path().join("plugin-data/tour/mode"), "free").unwrap();
    let approved = plugins.inspect(&pkg).await.unwrap().approval_digest;
    plugins
        .install(&pkg, &approved, Default::default())
        .await
        .unwrap();

    let listed = plugins.list().into_iter().find(|p| p.id == "tour").unwrap();
    let echo = &listed.commands[0];
    assert_eq!((echo.name.as_str(), echo.args.len()), ("echo", 2));
    assert_eq!(echo.args[0].kind, ArgKind::String);
    assert!(echo.args[0].required && !echo.args[1].required);

    for (command, args) in [
        ("echo", "{}"),
        ("echo", r#"{"name":"a","other":1}"#),
        ("echo", r#"{"name":1}"#),
        ("echo", r#"{"name":"a","times":"x"}"#),
        ("echo", "[1]"),
        ("ask", r#"{"name":"a"}"#),
    ] {
        assert!(
            matches!(
                plugins.check_command("tour", command, args),
                Err(PluginError::Arguments(_))
            ),
            "{command} {args}"
        );
    }
    assert!(matches!(
        plugins.check_command("tour", "nope", "{}"),
        Err(PluginError::NoCommand { .. })
    ));

    let person = || Invocation {
        person: true,
        ..Invocation::default()
    };
    plugins
        .run_command("tour", "echo", r#"{"name":"a","times":2}"#, person())
        .await
        .unwrap();
    assert_eq!(
        ran(home.path(), "ran").trim(),
        r#"echo {"name":"a","times":2}"#
    );

    let declined = plugins
        .run_command("tour", "ask", "{}", Invocation::default())
        .await;
    assert!(
        matches!(
            reached(declined.unwrap_err()),
            SandboxError::Guest(GuestError::Unsupported)
        ),
        "only a person's call may ask"
    );
    plugins
        .run_command("tour", "ask", "{}", person())
        .await
        .unwrap();
    assert_eq!(host.saw("confirm"), ["confirm tour: proceed?"]);
    assert_eq!(
        host.saw("url").len(),
        2,
        "the declined call drew its link too"
    );

    let in_job = |cancelled: bool| Invocation {
        job: Some("plugin-tour-1".into()),
        cancelled: Arc::new(AtomicBool::new(cancelled)),
        ..person()
    };
    plugins
        .run_command("tour", "work", "{}", in_job(false))
        .await
        .unwrap();
    assert_eq!(host.saw("progress tour plugin-tour-1").len(), 3);
    let cancelled = plugins
        .run_command("tour", "work", "{}", in_job(true))
        .await;
    assert!(
        matches!(reached(cancelled.unwrap_err()), SandboxError::Guest(GuestError::Failed(m)) if m == "cancelled"),
        "a cancelled job is seen by the plugin"
    );

    plugins
        .run_command("tour", "spawn", "{}", Invocation::default())
        .await
        .unwrap();
    assert_eq!(host.saw("started"), ["started tour echo job-1"]);
    assert_eq!(ran(home.path(), "spawned").trim(), "job-1");
    plugins
        .job_done("tour", "job-1", &JobOutcome::Failed("boom".into()))
        .await;
    assert_eq!(
        ran(home.path(), "done").trim(),
        r#"job-1 Outcome::Failed("boom")"#
    );

    let start = now_unix();
    plugins.run_due(start).await;
    plugins.run_due(start + 59).await;
    assert_eq!(ran(home.path(), "ticks"), "", "not due before its interval");
    plugins.run_due(start + 61).await;
    plugins.run_due(start + 62).await;
    assert_eq!(ran(home.path(), "ticks").trim(), "nightly");
    plugins.run_due(start + 3600).await;
    plugins.run_due(start + 3601).await;
    assert_eq!(
        ran(home.path(), "ticks").lines().count(),
        2,
        "an hour down is one run, not sixty"
    );
    plugins.run_due(start).await;
    plugins.run_due(start + 59).await;
    assert_eq!(
        ran(home.path(), "ticks").lines().count(),
        2,
        "a clock set back is counted from its new time"
    );
    plugins.run_due(start + 60).await;
    assert_eq!(ran(home.path(), "ticks").lines().count(), 3);

    plugins
        .run_command("tour", "flood", "{}", Invocation::default())
        .await
        .unwrap();
    let started: usize = ran(home.path(), "flood").trim().parse().unwrap();
    assert!(
        (25..=31).contains(&started),
        "a burst of 30 wake-ups, less what the plugin already spent: {started}"
    );
    plugins.run_due(start + 120).await;
    assert_eq!(
        ran(home.path(), "ticks").lines().count(),
        3,
        "a plugin out of wake-ups is not ticked"
    );
}

#[tokio::test]
async fn a_command_that_ignores_its_cancel_is_dropped_and_one_running_keeps_its_plugin_in_place() {
    let home = tempfile::tempdir().unwrap();
    let plugins = plugins(home.path(), Arc::new(Host::new(home.path())));
    let pkg = home.path().join("pkg");
    let optional = format!(
        "{MANIFEST}[permissions.\"secrets\"]\noptional = true\n[dependencies]\nbase = \"^0.1\"\n"
    );
    package(&pkg, "base", guest("hello"), "");
    let approved = plugins.inspect(&pkg).await.unwrap().approval_digest;
    plugins
        .install(&pkg, &approved, Default::default())
        .await
        .unwrap();
    package(&pkg, "tour", service_guest("tour"), &optional);
    std::fs::create_dir_all(home.path().join("plugin-data/tour")).unwrap();
    std::fs::write(home.path().join("plugin-data/tour/mode"), "free").unwrap();
    let approved = plugins.inspect(&pkg).await.unwrap().approval_digest;
    plugins
        .install(&pkg, &approved, Default::default())
        .await
        .unwrap();

    let cancelled = Invocation {
        cancelled: Arc::new(AtomicBool::new(true)),
        ..Invocation::default()
    };
    let dropped = plugins.run_command("tour", "wait", "{}", cancelled).await;
    assert!(
        matches!(reached(dropped.unwrap_err()), SandboxError::Stopped),
        "a plugin that never looks at the flag is stopped after the grace"
    );

    let mut waiting =
        std::pin::pin!(plugins.run_command("tour", "wait", "{}", Invocation::default()));
    tokio::select! {
        biased;
        _ = &mut waiting => panic!("it waits for ever"),
        () = async {} => {}
    }
    assert!(matches!(
        plugins.disable("tour", false).await,
        Err(PluginError::Busy(_))
    ));
    assert!(matches!(
        plugins.remove("tour").await,
        Err(PluginError::Busy(_))
    ));
    assert!(
        matches!(
            plugins.disable("base", true).await,
            Err(PluginError::Busy(busy)) if busy == "tour"
        ),
        "a cascade waits for a dependent that has work running"
    );
    assert!(
        matches!(
            plugins.set_permission("tour", "secrets", true).await,
            Err(PluginError::Busy(_))
        ),
        "a grant waits for the work to end"
    );
    let (stopped, changed) = tokio::join!(&mut waiting, plugins.set_developer(false));
    assert!(matches!(
        reached(stopped.unwrap_err()),
        SandboxError::Stopped
    ));
    assert_eq!(changed, ["base", "tour"]);
    plugins.set_developer(true).await;

    plugins
        .set_permission("tour", "secrets", true)
        .await
        .unwrap();
    let mut waiting =
        std::pin::pin!(plugins.run_command("tour", "wait", "{}", Invocation::default()));
    tokio::select! {
        biased;
        _ = &mut waiting => panic!("it waits for ever"),
        () = async {} => {}
    }
    let (stopped, revoked) = tokio::join!(
        &mut waiting,
        plugins.set_permission("tour", "secrets", false)
    );
    assert!(
        matches!(reached(stopped.unwrap_err()), SandboxError::Stopped),
        "a revoke narrows, so it stops the running command instead of waiting"
    );
    assert!(revoked.unwrap().optional.is_empty());

    for attempt in 1..=3 {
        let trapped = plugins
            .run_command("tour", "trap", "{}", Invocation::default())
            .await;
        let trapped = reached(trapped.unwrap_err());
        assert_eq!(
            matches!(trapped, SandboxError::TrapLimit(_)),
            attempt == 3,
            "{trapped}"
        );
    }
    let state = plugins.list().into_iter().find(|p| p.id == "tour").unwrap();
    assert_eq!(state.state, proto::plugin::PluginState::Disabled);
}

#[tokio::test]
async fn a_manifest_may_not_shadow_a_cli_verb_or_declare_what_the_component_lacks() {
    let home = tempfile::tempdir().unwrap();
    let plugins = Plugins::new(home.path(), Vec::new(), None)
        .unwrap()
        .with_developer(true);
    let pkg = home.path().join("pkg");

    package(&pkg, "server", guest("hello"), "");
    assert!(matches!(
        plugins.inspect(&pkg).await,
        Err(PluginError::Package(PackageError::Manifest(
            ManifestError::Verb(_)
        )))
    ));

    package(&pkg, "quiet", guest("hello"), MANIFEST);
    assert!(matches!(
        plugins.inspect(&pkg).await,
        Err(PluginError::Package(PackageError::Grant(
            GrantError::NotExported("commands")
        )))
    ));

    for bad in [
        "[[commands]]\nname = \"Echo\"\n",
        "[[commands]]\nname = \"a\"\n[[commands]]\nname = \"a\"\n",
        "[[schedules]]\nid = \"s\"\nevery = \"30s\"\n",
        "[[schedules]]\nid = \"s\"\nevery = \"0m\"\n",
        "[[schedules]]\nid = \"s\"\nevery = \"1\u{e9}\"\n",
        "[[schedules]]\nid = \"s\"\nevery = \"+5m\"\n",
        "[[commands]]\nname = \"help\"\n",
        "[[commands]]\nname = \"a\"\nargs = [{ name = \"help\" }]\n",
        (0..9)
            .map(|n| format!("[[schedules]]\nid = \"s{n}\"\nevery = \"1d\"\n"))
            .collect::<String>()
            .as_str(),
    ] {
        package(&pkg, "quiet", guest("hello"), bad);
        assert!(
            matches!(
                plugins.inspect(&pkg).await,
                Err(PluginError::Package(PackageError::Manifest(
                    ManifestError::Declaration(_)
                )))
            ),
            "{bad}"
        );
    }
}
