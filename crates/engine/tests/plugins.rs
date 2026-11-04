//! The plugin sandbox against a real component built from
//! `tests/fixture/plugins/probe`, whose `init` misbehaves as its `mode` data
//! file says. The WAT-based unit tests in `runtime/tests.rs` cover the epoch
//! deadline, the timeout and the host-side permission check.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use async_trait::async_trait;
use engine::{
    CallChain, CallError, HostCalls, HostError, LoadedPlugin, PluginRecord, PluginRuntime,
};
use proto::plugin::{Grant, Permission};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const CALL_BUDGET: Duration = Duration::from_secs(30);

fn probe_component() -> &'static Path {
    static COMPONENT: OnceLock<PathBuf> = OnceLock::new();
    COMPONENT.get_or_init(|| {
        let manifest =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixture/plugins/probe/Cargo.toml");
        let target_dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("plugins");
        let output = Command::new(env!("CARGO"))
            .args(["build", "--release", "--target", "wasm32-wasip2"])
            .arg("--manifest-path")
            .arg(&manifest)
            .arg("--target-dir")
            .arg(&target_dir)
            .output()
            .expect("run cargo");
        assert!(
            output.status.success(),
            "building the probe plugin failed (is the target installed? \
             `rustup target add wasm32-wasip2`):\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        target_dir.join("wasm32-wasip2/release/probe.wasm")
    })
}

struct ReadChannels;

#[async_trait]
impl HostCalls for ReadChannels {
    fn permission(&self, channel: &str) -> Option<Permission> {
        (channel == "server.list").then_some(Permission::EntryRead)
    }

    async fn call(
        &self,
        _plugin: &str,
        _chain: &CallChain,
        _channel: &str,
        _payload: Value,
    ) -> Result<Value, HostError> {
        Ok(json!([]))
    }

    async fn run_job(
        &self,
        plugin: &str,
        chain: &CallChain,
        channel: &str,
        payload: Value,
    ) -> Result<Value, HostError> {
        self.call(plugin, chain, channel, payload).await
    }
}

struct Probe {
    plugin: LoadedPlugin,
    data: tempfile::TempDir,
}

impl Probe {
    fn load(permissions: &[Permission]) -> Probe {
        let wasm = std::fs::read(probe_component()).unwrap();
        let data = tempfile::tempdir().unwrap();
        let runtime =
            PluginRuntime::new(Arc::new(ReadChannels), data.path().join("cache")).unwrap();
        let record = PluginRecord {
            id: "probe".into(),
            wasm_sha256: Some(
                Sha256::digest(&wasm)
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect(),
            ),
            grant: Grant {
                permissions: permissions.iter().copied().collect(),
                ..Grant::default()
            },
            ..PluginRecord::default()
        };
        let dir = data.path().join("data");
        std::fs::create_dir_all(&dir).unwrap();
        let plugin = runtime.load(&record, &wasm, dir).unwrap();
        Probe { plugin, data }
    }

    fn set_mode(&self, mode: &str) {
        std::fs::write(
            self.data.path().join("data/mode.json"),
            serde_json::to_string(mode).unwrap(),
        )
        .unwrap();
    }

    async fn init(&self) -> Result<(), CallError> {
        self.plugin.init(&CallChain::default(), CALL_BUDGET).await
    }
}

#[tokio::test]
async fn a_channel_outside_the_grant_answers_permission_denied_and_the_plugin_still_loads() {
    let probe = Probe::load(&[]);

    probe.set_mode("list-servers");
    let denied = probe.init().await;
    let Err(CallError::Guest(message)) = denied else {
        panic!("{denied:?}");
    };
    assert!(message.contains("entry:read"), "{message}");

    probe.set_mode("idle");
    probe.init().await.unwrap();
}

#[tokio::test]
async fn a_granted_channel_reaches_the_host() {
    let probe = Probe::load(&[Permission::EntryRead]);
    probe.set_mode("list-servers");

    probe.init().await.unwrap();
}

#[tokio::test]
async fn memory_past_the_limit_traps_the_call_and_the_next_one_runs_fresh() {
    let probe = Probe::load(&[]);

    probe.set_mode("exhaust-memory");
    let trapped = probe.init().await;
    assert!(matches!(trapped, Err(CallError::Trap(_))), "{trapped:?}");

    probe.set_mode("idle");
    probe.init().await.unwrap();
}
