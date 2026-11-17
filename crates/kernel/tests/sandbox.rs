//! The sandbox against real components built from `tests/guest/`.

use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_util::future::join_all;
use kernel::{Answer, Limits, NoticeLevel, Sandbox, SandboxError, Services};
use sha2::{Digest, Sha256};
use tokio::sync::Notify;

#[allow(dead_code)]
mod fixture;
use fixture::{guest, scratch, service_guest, wasm};

static LOG: Mutex<Vec<u8>> = Mutex::new(Vec::new());

struct Capture;

impl Write for Capture {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        LOG.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn captured() -> String {
    String::from_utf8_lossy(&LOG.lock().unwrap()).into_owned()
}

/// The log entries of one plugin: an entry starts at a timestamp line and may
/// span several, and tests of this binary run concurrently into one capture.
fn captured_for(plugin: &str) -> String {
    let marker = format!("plugin=\"{plugin}\"");
    let log = captured();
    let mut entries: Vec<String> = Vec::new();
    for line in log.lines() {
        match line.starts_with("20") {
            true => entries.push(line.to_owned()),
            false => match entries.last_mut() {
                Some(entry) => {
                    entry.push('\n');
                    entry.push_str(line);
                }
                None => entries.push(line.to_owned()),
            },
        }
    }
    entries
        .into_iter()
        .filter(|entry| entry.contains(&marker))
        .collect::<Vec<_>>()
        .join("\n")
}

fn cache_dir() -> PathBuf {
    scratch().join("plugin-cache")
}

fn sandbox(limits: Limits) -> Sandbox {
    let _ = tracing_subscriber::fmt()
        .with_writer(|| Capture)
        .with_ansi(false)
        .with_max_level(tracing::Level::INFO)
        .try_init();
    Sandbox::new(cache_dir(), &scratch().join("plugin-cache.key"), limits).expect("sandbox")
}

fn sandbox_services(limits: Limits, host: Arc<Host>) -> Sandbox {
    sandbox(limits).with_services(host)
}

fn quick() -> Limits {
    Limits::default().with_timeout(Duration::from_millis(200))
}

fn cache_entry(plugin: &str) -> PathBuf {
    let name: String = Sha256::digest(plugin)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    cache_dir().join(format!("{name}.cwasm"))
}

#[tokio::test]
async fn a_plugin_loads_logs_and_runs_init_inside_the_wasi_rule() {
    let sandbox = sandbox(Limits::default());

    let hello = sandbox.load("hello", wasm(guest("hello"))).await.unwrap();
    assert!(hello.provides_lifecycle());
    sandbox.init(&hello).await.unwrap();
    assert!(captured().contains(&format!(
        "hello from the guest, {} {}",
        common::app::NAME,
        common::app::VERSION
    )));

    let bare = sandbox.load("bare", wasm(guest("bare"))).await.unwrap();
    assert!(!bare.provides_lifecycle());
    sandbox.init(&bare).await.unwrap();

    let chatty = sandbox.load("chatty", wasm(guest("chatty"))).await.unwrap();
    sandbox.init(&chatty).await.unwrap();
    let log = captured();
    assert_eq!(
        log.matches("chatty line").count(),
        1000,
        "lines past the cap are dropped"
    );
    assert!(log.contains("are dropped"));

    let probe = sandbox.load("probe", wasm(guest("probe"))).await.unwrap();
    let outcome = sandbox.init(&probe).await;
    assert!(
        outcome.is_ok(),
        "no environment, filesystem, socket or lookup: {outcome:?}"
    );
}

#[tokio::test]
async fn a_cached_component_is_reused_and_a_swapped_one_is_never_run() {
    let sandbox = sandbox(Limits::default());
    sandbox.load("cached", wasm(guest("hello"))).await.unwrap();
    sandbox.load("other", wasm(guest("spin"))).await.unwrap();
    let entry = cache_entry("cached");
    let written = std::fs::metadata(&entry).unwrap().modified().unwrap();

    let again = sandbox.load("cached", wasm(guest("hello"))).await.unwrap();
    assert_eq!(
        std::fs::metadata(&entry).unwrap().modified().unwrap(),
        written,
        "a valid entry is loaded, not recompiled"
    );
    sandbox.init(&again).await.unwrap();

    std::fs::copy(cache_entry("other"), &entry).unwrap();
    let swapped = sandbox.load("cached", wasm(guest("hello"))).await.unwrap();
    let started = Instant::now();
    sandbox.init(&swapped).await.unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "the entry of another plugin is recompiled, never run"
    );
}

#[tokio::test]
async fn a_call_past_its_time_budget_is_stopped_even_inside_a_host_call() {
    let sandbox = sandbox(quick());
    let spin = sandbox.load("spin", wasm(guest("spin"))).await.unwrap();
    let hang = sandbox.load("hang", wasm(guest("hang"))).await.unwrap();
    let started = Instant::now();
    let (spun, hung) = tokio::join!(sandbox.init(&spin), sandbox.init(&hang));
    assert!(matches!(spun, Err(SandboxError::Timeout)), "{spun:?}");
    assert!(matches!(hung, Err(SandboxError::Timeout)), "{hung:?}");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "stopped after {:?}",
        started.elapsed()
    );
}

#[tokio::test]
async fn memory_up_to_the_limit_is_granted_and_past_it_the_call_is_stopped() {
    let sandbox = sandbox(Limits::default());
    let fit = sandbox.load("fit", wasm(guest("fit"))).await.unwrap();
    sandbox.init(&fit).await.unwrap();

    let grow = sandbox.load("grow", wasm(guest("grow"))).await.unwrap();
    assert!(matches!(
        sandbox.init(&grow).await,
        Err(SandboxError::Memory)
    ));
}

#[tokio::test]
async fn the_third_trap_in_the_window_reports_the_trap_limit() {
    let sandbox = sandbox(Limits::default());
    let trap = sandbox.load("trap", wasm(guest("trap"))).await.unwrap();
    for _ in 0..2 {
        assert!(matches!(
            sandbox.init(&trap).await,
            Err(SandboxError::Trapped(_))
        ));
    }
    assert!(matches!(
        sandbox.init(&trap).await,
        Err(SandboxError::TrapLimit(failure)) if matches!(*failure, SandboxError::Trapped(_))
    ));
}

#[tokio::test]
async fn calls_past_the_concurrency_bounds_wait_for_a_slot() {
    async fn naps(sandbox: &Sandbox, plugins: &[&str], calls: usize) -> Duration {
        let mut loaded = Vec::new();
        for id in plugins {
            let plugin = sandbox.load(id, wasm(guest("nap"))).await.unwrap();
            loaded.extend(std::iter::repeat_n(plugin, calls));
        }
        let started = Instant::now();
        let results = join_all(loaded.iter().map(|plugin| sandbox.init(plugin))).await;
        assert!(results.iter().all(Result::is_ok), "{results:?}");
        started.elapsed()
    }

    let sandbox = sandbox(Limits::default());
    let one_plugin = naps(&sandbox, &["napper"], 5).await;
    assert!(
        one_plugin >= Duration::from_millis(100),
        "the fifth nap of one plugin waits for one of four: {one_plugin:?}"
    );
    let overall = naps(&sandbox, &["a", "b", "c", "d", "e"], 4).await;
    assert!(
        overall >= Duration::from_millis(100),
        "naps past sixteen overall wait: {overall:?}"
    );
}

/// A host for the service tests: files under a scratch root, a catalogue of
/// one message, and a person who answers `confirm` after `answer_after`.
#[derive(Default)]
struct Host {
    root: PathBuf,
    confirm: bool,
    answer_after: Duration,
    locked: Notify,
    seen: Mutex<Vec<String>>,
}

impl Host {
    fn new(name: &str, confirm: bool) -> Arc<Self> {
        Self::slow(name, confirm, Duration::ZERO)
    }

    fn slow(name: &str, confirm: bool, answer_after: Duration) -> Arc<Self> {
        let root = scratch().join("services").join(name);
        let _ = std::fs::remove_dir_all(&root);
        Arc::new(Host {
            root,
            confirm,
            answer_after,
            ..Host::default()
        })
    }

    fn data(&self, plugin: &str) -> PathBuf {
        self.root.join("data").join(plugin)
    }

    fn state(&self, plugin: &str) -> PathBuf {
        self.root.join("state").join(plugin)
    }

    fn mode(&self, plugin: &str, mode: &str) {
        std::fs::create_dir_all(self.data(plugin)).unwrap();
        std::fs::write(self.data(plugin).join("mode"), mode).unwrap();
    }

    fn saw(&self, prefix: &str) -> Vec<String> {
        let seen = self.seen.lock().unwrap();
        seen.iter()
            .filter(|line| line.starts_with(prefix))
            .cloned()
            .collect()
    }

    fn record(&self, line: String) {
        self.seen.lock().unwrap().push(line);
    }
}

impl Services for Host {
    fn data_dir(&self, plugin: &str) -> Option<PathBuf> {
        Some(self.data(plugin))
    }

    fn state_dir(&self, plugin: &str) -> Option<PathBuf> {
        Some(self.state(plugin))
    }

    fn message(&self, _plugin: &str, key: &str) -> Option<String> {
        (key == "greeting").then(|| "Hello {name}".to_owned())
    }

    fn notify(&self, plugin: &str, _job: Option<&str>, _level: NoticeLevel, message: &str) {
        self.record(format!("notify {plugin}: {message}"));
        if message == "locked" {
            self.locked.notify_one();
        }
    }

    fn confirm<'a>(
        &'a self,
        plugin: &'a str,
        _job: Option<&'a str>,
        message: &'a str,
    ) -> Answer<'a> {
        self.record(format!("confirm {plugin}: {message}"));
        Box::pin(async move {
            tokio::time::sleep(self.answer_after).await;
            self.confirm
        })
    }

    fn open_url(&self, plugin: &str, _job: Option<&str>, url: &str) -> bool {
        self.record(format!("url {plugin}: {url}"));
        true
    }
}

#[tokio::test]
async fn a_plugin_keeps_files_settings_and_secrets_and_draws_only_through_the_host() {
    let host = Host::new("tour", true);
    let sandbox = sandbox_services(Limits::default(), host.clone());
    let tour = sandbox
        .load("tour", wasm(service_guest("tour")))
        .await
        .unwrap();

    host.mode("tour", "tour");
    sandbox.init(&tour).await.unwrap();
    assert_eq!(
        std::fs::read_to_string(host.data("tour").join("note.txt")).unwrap(),
        "kept"
    );
    let settings = std::fs::read_to_string(host.state("tour").join("settings.json")).unwrap();
    assert!(settings.contains("\"volume\": 7"), "{settings}");
    assert_eq!(host.saw("notify tour: notice").len(), 5, "budget is five");
    assert_eq!(
        host.saw("url"),
        ["url tour: https://example.com/docs".to_owned()]
    );
    assert!(host.saw("confirm").is_empty(), "no person started the call");

    host.mode("tour", "secret");
    sandbox.init(&tour).await.unwrap();
    let secrets = host.state("tour").join("secrets.json");
    assert!(std::fs::read_to_string(&secrets)
        .unwrap()
        .contains("s3cret"));
    assert!(
        !captured().contains("s3cret"),
        "a secret never reaches a log"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&secrets).unwrap().permissions().mode();
        assert_eq!(mode & 0o077, 0, "secrets are owner-only");
    }

    std::fs::write(host.state("tour").join("settings.json"), "{ not json").unwrap();
    let fresh = sandbox_services(Limits::default(), host.clone());
    let tour = fresh
        .load("tour", wasm(service_guest("tour")))
        .await
        .unwrap();
    host.mode("tour", "tour");
    fresh.init(&tour).await.unwrap();
    let aside = std::fs::read_dir(host.state("tour"))
        .unwrap()
        .filter_map(|entry| entry.ok())
        .any(|entry| entry.file_name().to_string_lossy().contains("unreadable"));
    assert!(
        aside,
        "an unreadable settings file is set aside, not written over"
    );
}

#[tokio::test(start_paused = true)]
async fn a_changed_setting_reaches_the_plugin_which_may_wait_on_the_user_past_its_budget() {
    let host = Host::slow("changed", true, Duration::from_secs(30));
    let sandbox = sandbox_services(quick(), host.clone());
    let tour = sandbox
        .load("tour", wasm(service_guest("tour")))
        .await
        .unwrap();
    assert!(tour.provides_settings_changed());

    sandbox.change_setting(&tour, "volume", "9").await.unwrap();
    assert_eq!(
        std::fs::read_to_string(host.data("tour").join("changed")).unwrap(),
        "volume=9"
    );
    assert_eq!(
        host.saw("confirm"),
        ["confirm tour: apply volume?".to_owned()]
    );

    assert!(matches!(
        sandbox.change_setting(&tour, "volume", "{").await,
        Err(SandboxError::Setting(_))
    ));

    let declining = Host::new("declined", false);
    let sandbox = sandbox_services(Limits::default(), declining.clone());
    let tour = sandbox
        .load("tour", wasm(service_guest("tour")))
        .await
        .unwrap();
    assert!(matches!(
        sandbox.change_setting(&tour, "volume", "3").await,
        Err(SandboxError::Guest(_))
    ));
    let settings = std::fs::read_to_string(declining.state("tour").join("settings.json")).unwrap();
    assert!(settings.contains("\"volume\": 3"), "{settings}");
}

#[tokio::test]
async fn a_named_lock_excludes_other_calls_until_its_holder_ends_even_by_timeout() {
    let host = Host::new("locks", true);
    let sandbox = sandbox_services(quick(), host.clone());
    let tour = sandbox
        .load("tour", wasm(service_guest("tour")))
        .await
        .unwrap();

    host.mode("tour", "hold");
    let holding = sandbox.init(&tour);
    let meanwhile = async {
        host.locked.notified().await;
        host.mode("tour", "contend");
        sandbox.init(&tour).await
    };
    let (held, contended) = tokio::join!(holding, meanwhile);
    assert!(matches!(held, Err(SandboxError::Timeout)), "{held:?}");
    contended.unwrap();

    host.mode("tour", "free");
    sandbox.init(&tour).await.unwrap();
}

#[tokio::test]
async fn developer_mode_logs_the_backtrace_of_a_trap_and_the_function_of_a_denial() {
    let sandbox = sandbox(Limits::default());
    let trap = sandbox.load("dev-trap", wasm(guest("trap"))).await.unwrap();
    let error = sandbox.init(&trap).await.unwrap_err();
    let log = captured_for("dev-trap");
    assert!(
        !log.contains("trapped\n") && !log.contains("wasm backtrace"),
        "quiet outside developer mode: {log}"
    );
    assert!(
        !error.to_string().contains("backtrace"),
        "the error a caller sees is the root cause: {error}"
    );
    sandbox.set_developer(true);
    let _ = sandbox.init(&trap).await;
    let log = captured_for("dev-trap");
    let trapped = log.split("trapped\n").nth(1).expect("trap logged");
    assert!(trapped.contains("wasm backtrace"), "{trapped}");
    assert!(trapped.contains("trap.wasm!"), "{trapped}");

    let host = Host::new("dev-denied", true);
    let sandbox = sandbox_services(Limits::default(), host.clone());
    host.mode("dev-denied", "secret");
    let denied = kernel::Denials::from([("hestia:kernel/secrets@0.1.0".into(), "secrets".into())]);
    let opened = sandbox
        .open("dev-denied", wasm(service_guest("tour")))
        .await
        .unwrap();
    let loaded = sandbox
        .link(&opened, &denied, &Default::default(), Default::default())
        .unwrap();
    let _ = sandbox.init(&loaded).await;
    assert!(
        !captured_for("dev-denied").contains("denied: hestia:kernel/secrets#"),
        "quiet outside developer mode"
    );
    sandbox.set_developer(true);
    let _ = sandbox.init(&loaded).await;
    assert!(
        captured_for("dev-denied").contains("denied: hestia:kernel/secrets#set"),
        "{}",
        captured_for("dev-denied")
    );
}

#[tokio::test]
async fn plugins_set_developer_reaches_the_sandbox_of_a_dev_installed_plugin() {
    let home = tempfile::tempdir().unwrap();
    let host = Host::new("dev-flow", true);
    let plugins = kernel::Plugins::new(
        home.path(),
        Vec::new(),
        Some(Arc::new(sandbox_services(Limits::default(), host.clone()))),
    )
    .unwrap();
    host.mode("devflow", "free");
    plugins.set_developer(true).await;

    let pkg = home.path().join("pkg");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(
        pkg.join("plugin.toml"),
        "id = \"devflow\"\nname = \"devflow\"\nversion = \"0.1.0\"\n[[commands]]\nname = \"trap\"\n",
    )
    .unwrap();
    std::fs::copy(service_guest("tour"), pkg.join("plugin.wasm")).unwrap();
    let approved = plugins.inspect(&pkg).await.unwrap().approval_digest;
    plugins
        .install(&pkg, &approved, Default::default())
        .await
        .unwrap();
    let _ = plugins
        .run_command("devflow", "trap", "{}", kernel::Invocation::default())
        .await;
    assert!(captured_for("devflow").contains("trapped\n"));
}
