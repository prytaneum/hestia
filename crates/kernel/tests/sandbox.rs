//! The sandbox against real components built from `tests/guest/`.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use futures_util::future::join_all;
use kernel::{Limits, Sandbox, SandboxError};
use sha2::{Digest, Sha256};

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

fn scratch() -> PathBuf {
    PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
}

fn guest(name: &str) -> PathBuf {
    static BUILT: OnceLock<PathBuf> = OnceLock::new();
    let dir = BUILT.get_or_init(|| {
        let target = scratch().join("guest");
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/guest/Cargo.toml");
        let output = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
            .args([
                "build",
                "--release",
                "--locked",
                "--target",
                "wasm32-wasip2",
                "--manifest-path",
            ])
            .arg(manifest)
            .env("CARGO_TARGET_DIR", &target)
            .output()
            .expect("run cargo for the guest plugins");
        assert!(
            output.status.success(),
            "guest plugins failed to build:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        target.join("wasm32-wasip2/release")
    });
    dir.join(format!("{name}.wasm"))
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

fn quick() -> Limits {
    Limits::default().with_init_timeout(Duration::from_millis(200))
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

    let hello = sandbox.load("hello", &guest("hello")).await.unwrap();
    assert!(hello.provides_lifecycle());
    sandbox.init(&hello).await.unwrap();
    assert!(captured().contains(&format!(
        "hello from the guest, {} {}",
        common::app::NAME,
        common::app::VERSION
    )));

    let bare = sandbox.load("bare", &guest("bare")).await.unwrap();
    assert!(!bare.provides_lifecycle());
    sandbox.init(&bare).await.unwrap();

    let probe = sandbox.load("probe", &guest("probe")).await.unwrap();
    let outcome = sandbox.init(&probe).await;
    assert!(
        outcome.is_ok(),
        "no environment, filesystem, socket or lookup: {outcome:?}"
    );
}

#[tokio::test]
async fn a_cached_component_is_reused_and_a_swapped_one_is_never_run() {
    let sandbox = sandbox(Limits::default());
    sandbox.load("cached", &guest("hello")).await.unwrap();
    sandbox.load("other", &guest("spin")).await.unwrap();
    let entry = cache_entry("cached");
    let written = std::fs::metadata(&entry).unwrap().modified().unwrap();

    let again = sandbox.load("cached", &guest("hello")).await.unwrap();
    assert_eq!(
        std::fs::metadata(&entry).unwrap().modified().unwrap(),
        written,
        "a valid entry is loaded, not recompiled"
    );
    sandbox.init(&again).await.unwrap();

    std::fs::copy(cache_entry("other"), &entry).unwrap();
    let swapped = sandbox.load("cached", &guest("hello")).await.unwrap();
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
    let spin = sandbox.load("spin", &guest("spin")).await.unwrap();
    let hang = sandbox.load("hang", &guest("hang")).await.unwrap();
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
    let fit = sandbox.load("fit", &guest("fit")).await.unwrap();
    sandbox.init(&fit).await.unwrap();

    let grow = sandbox.load("grow", &guest("grow")).await.unwrap();
    assert!(matches!(
        sandbox.init(&grow).await,
        Err(SandboxError::Memory)
    ));
}

#[tokio::test]
async fn the_third_trap_in_the_window_reports_the_trap_limit() {
    let sandbox = sandbox(Limits::default());
    let trap = sandbox.load("trap", &guest("trap")).await.unwrap();
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
            let plugin = sandbox.load(id, &guest("nap")).await.unwrap();
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
