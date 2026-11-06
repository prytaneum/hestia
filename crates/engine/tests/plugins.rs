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
    command_args, CallChain, CallError, Cancel, CommandRun, HostCalls, HostError, LoadedPlugin,
    PluginRecord, PluginRuntime,
};
use proto::error::ErrorInfo;
use proto::plugin::{
    CommandArg, CommandArgKind, CommandContribution, CommandScope, Grant, PageContext, Permission,
    PluginRunProgress,
};
use proto::ui::{Action, Effect, UiNode};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::sync::watch;

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
        Probe::load_with_network(permissions, &[])
    }

    fn load_with_network(permissions: &[Permission], network: &[&str]) -> Probe {
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
                network: network.iter().map(|host| host.to_string()).collect(),
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

    async fn run(
        &self,
        command: &str,
        args: Value,
        cancel: &Cancel,
        grace: Duration,
    ) -> (
        Result<Value, CallError>,
        watch::Receiver<Option<PluginRunProgress>>,
    ) {
        let (progress, reports) = watch::channel(None);
        let control = CommandRun {
            cancel: cancel.clone(),
            progress,
            grace,
        };
        let outcome = self
            .plugin
            .run_command(&CallChain::default(), command, &args, control, CALL_BUDGET)
            .await;
        (outcome, reports)
    }
}

const GRACE: Duration = Duration::from_millis(300);

#[tokio::test]
async fn a_page_renders_a_tree_that_decodes_and_carries_its_context() {
    let probe = Probe::load(&[]);
    let context = PageContext {
        entry: Some("srv-1".into()),
    };

    let tree = probe
        .plugin
        .render_page(&CallChain::default(), "overview", &context)
        .await
        .unwrap();

    let UiNode::Stack { children, .. } = tree else {
        panic!("{tree:?}");
    };
    assert_eq!(children.len(), 3);
    assert!(matches!(&children[1], UiNode::Text { text, .. } if text == "srv-1"));

    let unknown = probe
        .plugin
        .render_page(&CallChain::default(), "nope", &PageContext::default())
        .await;
    assert!(matches!(unknown, Err(CallError::Guest(_))), "{unknown:?}");
}

#[tokio::test]
async fn a_page_action_answers_effects() {
    let probe = Probe::load(&[]);
    let action = Action {
        id: "go".into(),
        payload: Value::Null,
    };

    let effects = probe
        .plugin
        .page_action(
            &CallChain::default(),
            "overview",
            &action,
            &json!({ "name": "smp" }),
            &PageContext::default(),
        )
        .await
        .unwrap();

    assert_eq!(effects.len(), 2);
    assert!(matches!(&effects[0], Effect::Toast { text, .. } if text == r#"go {"name":"smp"}"#));
    assert!(matches!(effects[1], Effect::Refresh));
}

#[tokio::test]
async fn a_command_runs_and_answers_its_output() {
    let probe = Probe::load(&[]);

    let (outcome, _) = probe
        .run("echo", json!({ "name": "smp" }), &Cancel::new(), GRACE)
        .await;
    assert_eq!(outcome.unwrap(), json!({ "name": "smp" }));

    let (unknown, _) = probe.run("nope", json!({}), &Cancel::new(), GRACE).await;
    let Err(CallError::Guest(message)) = unknown else {
        panic!("{unknown:?}");
    };
    assert!(message.contains("unknown command"), "{message}");
}

#[tokio::test]
async fn a_command_reports_progress_to_whoever_started_it() {
    let probe = Probe::load(&[]);

    let (outcome, reports) = probe
        .run("progress", json!({}), &Cancel::new(), GRACE)
        .await;
    assert_eq!(outcome.unwrap(), json!({ "cancelled": false }));

    let latest = reports.borrow().clone().unwrap();
    assert_eq!(
        (latest.fraction, latest.message.as_str()),
        (None, "finishing")
    );
}

#[tokio::test]
async fn a_cancelled_command_sees_it_and_stops_on_its_own() {
    let probe = Probe::load(&[]);
    let cancel = Cancel::new();
    let asker = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(200)).await;
        asker.cancel();
    });

    let (outcome, _) = probe.run("wait", json!({}), &cancel, CALL_BUDGET).await;

    let Err(CallError::Guest(message)) = outcome else {
        panic!("{outcome:?}");
    };
    assert_eq!(message, "stopped");
}

#[tokio::test]
async fn a_command_that_ignores_its_cancel_is_dropped_after_the_grace() {
    let probe = Probe::load(&[]);
    let cancel = Cancel::new();
    cancel.cancel();

    let started = std::time::Instant::now();
    let (outcome, _) = probe.run("stubborn", json!({}), &cancel, GRACE).await;

    assert!(matches!(outcome, Err(CallError::Cancelled)), "{outcome:?}");
    assert!(started.elapsed() < Duration::from_secs(10));
    probe.init().await.unwrap();
    assert_eq!(probe.plugin.stats().traps, 0);
}

fn declared(scope: CommandScope) -> CommandContribution {
    let arg = |name: &str, kind, required| CommandArg {
        name: name.into(),
        kind,
        required,
        help: String::new(),
    };
    CommandContribution {
        id: "go".into(),
        scope,
        args: vec![
            arg("name", CommandArgKind::String, true),
            arg("count", CommandArgKind::Number, false),
            arg("loud", CommandArgKind::Bool, false),
        ],
        ..CommandContribution::default()
    }
}

fn refused(result: Result<Value, ErrorInfo>) -> String {
    match result {
        Err(ErrorInfo::PluginCommandInvalid { detail, .. }) => detail,
        other => panic!("{other:?}"),
    }
}

#[test]
fn arguments_are_checked_against_the_declaration_before_the_guest_runs() {
    let top = declared(CommandScope::Top);
    let check = |args| command_args("demo", &top, None, &args);

    assert_eq!(
        check(json!({ "name": "a", "count": 2, "loud": true })).unwrap(),
        json!({ "name": "a", "count": 2, "loud": true })
    );
    assert_eq!(
        check(json!({ "name": "a", "count": null })).unwrap(),
        json!({ "name": "a" })
    );
    assert!(refused(check(json!({}))).contains("`name` is required"));
    assert!(refused(check(json!({ "name": 1 }))).contains("`name` must be a string"));
    assert!(refused(check(json!({ "name": "a", "extra": 1 }))).contains("unknown argument `extra`"));
    assert!(refused(check(json!(["a"]))).contains("must be an object"));
}

#[test]
fn a_scoped_command_needs_its_entry_and_a_top_one_refuses_it() {
    let args = json!({ "name": "a" });
    let server = declared(CommandScope::Server);
    let top = declared(CommandScope::Top);

    assert!(refused(command_args("demo", &server, None, &args)).contains("needs an entry"));
    assert!(refused(command_args("demo", &top, Some("srv-1"), &args)).contains("no entry"));
    assert_eq!(
        command_args("demo", &server, Some("srv-1"), &args).unwrap(),
        json!({ "name": "a", "entry": "srv-1" })
    );
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

async fn http_refusal(probe: &Probe, url: &str) -> String {
    probe.set_mode(&format!("get {url}"));
    match probe.init().await {
        Err(CallError::Guest(message)) => message,
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn http_without_the_network_permission_is_refused_and_the_plugin_still_loads() {
    let probe = Probe::load_with_network(&[], &["example.com"]);

    let message = http_refusal(&probe, "https://example.com/").await;
    assert!(message.contains("HttpRequestDenied"), "{message}");
}

#[tokio::test]
async fn http_to_a_host_outside_the_allowlist_is_refused() {
    let probe = Probe::load_with_network(&[Permission::NetworkConnect], &["example.com"]);

    let message = http_refusal(&probe, "https://other.example.org/").await;
    assert!(message.contains("HttpRequestDenied"), "{message}");
}

#[tokio::test]
async fn http_to_an_ip_address_is_refused_even_when_listed() {
    let probe = Probe::load_with_network(&[Permission::NetworkConnect], &["127.0.0.1"]);

    let message = http_refusal(&probe, "https://127.0.0.1/").await;
    assert!(message.contains("DestinationIpProhibited"), "{message}");
}

#[tokio::test]
async fn http_to_a_granted_name_that_resolves_to_this_machine_is_refused() {
    let probe = Probe::load_with_network(&[Permission::NetworkConnect], &["localhost"]);

    let message = http_refusal(&probe, "https://localhost/").await;
    assert!(message.contains("DestinationIpProhibited"), "{message}");
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

#[tokio::test]
async fn an_event_reaches_the_guest_on_a_fresh_store() {
    let probe = Probe::load(&[]);
    let chain = CallChain::default();
    for n in 0..2 {
        probe
            .plugin
            .on_event(&chain, "server.created", &json!({ "n": n }), CALL_BUDGET)
            .await
            .unwrap();
    }
    let path = probe.data.path().join("data/events.json");
    let recorded: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(
        recorded,
        json!([
            { "topic": "server.created", "payload": { "n": 0 } },
            { "topic": "server.created", "payload": { "n": 1 } },
        ])
    );
}
