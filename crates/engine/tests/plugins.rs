//! The plugin sandbox against a real component built from
//! `tests/fixture/plugins/probe`, whose `init` misbehaves as its `mode` data
//! file says. The WAT-based unit tests in `runtime/tests.rs` cover the epoch
//! deadline, the timeout and the host-side permission check.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use engine::{
    command_args, CallChain, CallError, Cancel, CommandRun, GuestError, HostCalls, LoadedPlugin,
    PluginRecord, PluginRuntime, PluginSettings, ProcessSupervisor,
};
use proto::error::{ErrorInfo, Service};
use proto::plugin::{
    CommandArg, CommandArgKind, CommandContribution, CommandScope, Grant, PageContext, Permission,
    PermissionScope, PluginRunProgress, ScopeKind, Setting, SettingKind, Touched,
};
use proto::ui::{Action, Effect, UiNode};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::sync::watch;

use crate::fixture::probe;

const CALL_BUDGET: Duration = Duration::from_secs(30);

struct ReadChannels {
    setting_changes: Arc<AtomicUsize>,
}

#[async_trait]
impl HostCalls for ReadChannels {
    fn setting_changed(&self, _plugin: &str) {
        self.setting_changes.fetch_add(1, Ordering::Relaxed);
    }

    fn permission(&self, channel: &str) -> Option<Permission> {
        matches!(
            channel,
            "server.list" | "instance.list" | "server.files.read"
        )
        .then_some(Permission::EntryRead)
    }

    fn touched(&self, channel: &str, payload: &Value) -> Touched {
        match channel {
            "server.files.read" => {
                <proto::files::ServerFilesRead as proto::Contract>::touched(payload)
            }
            _ => Touched::Unscoped,
        }
    }

    async fn call(
        &self,
        _plugin: &str,
        _chain: &CallChain,
        channel: &str,
        _payload: Value,
    ) -> Result<Value, ErrorInfo> {
        match channel {
            "instance.list" => Err(ErrorInfo::Offline { service: None }),
            _ => Ok(json!([])),
        }
    }

    async fn run_job(
        &self,
        plugin: &str,
        chain: &CallChain,
        channel: &str,
        payload: Value,
    ) -> Result<Value, ErrorInfo> {
        self.call(plugin, chain, channel, payload).await
    }
}

fn declared_settings() -> Vec<Setting> {
    vec![Setting {
        key: "api-key".into(),
        kind: SettingKind::Secret,
        ..Setting::default()
    }]
}

struct Probe {
    plugin: LoadedPlugin,
    data: tempfile::TempDir,
    #[cfg_attr(not(unix), allow(dead_code))]
    supervisor: Arc<ProcessSupervisor>,
    changes: Arc<AtomicUsize>,
}

impl Probe {
    fn load(permissions: &[Permission]) -> Probe {
        Probe::load_with_network(permissions, &[])
    }

    fn load_with_network(permissions: &[Permission], network: &[&str]) -> Probe {
        let grant = Grant {
            permissions: permissions
                .iter()
                .map(|permission| {
                    let scope = match permission {
                        Permission::NetworkConnect => {
                            PermissionScope::of(ScopeKind::Hosts, network.iter().copied())
                        }
                        _ => PermissionScope::default(),
                    };
                    (*permission, scope)
                })
                .collect(),
            ..Grant::default()
        };
        Probe::load_with(grant, BTreeMap::new())
    }

    fn load_with(grant: Grant, program_paths: BTreeMap<String, PathBuf>) -> Probe {
        let wasm = std::fs::read(probe::component()).unwrap();
        let data = tempfile::tempdir().unwrap();
        let supervisor = Arc::new(ProcessSupervisor::new(data.path().join("processes")));
        let changes = Arc::new(AtomicUsize::new(0));
        let runtime = PluginRuntime::new(
            Arc::new(ReadChannels {
                setting_changes: changes.clone(),
            }),
            data.path().join("cache"),
            supervisor.clone(),
        )
        .unwrap()
        .with_download_cache(Arc::new(engine::Cache::new(data.path().join("downloads"))));
        let record = PluginRecord {
            id: "probe".into(),
            wasm_sha256: Some(
                Sha256::digest(&wasm)
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect(),
            ),
            grant,
            program_paths,
            ..PluginRecord::default()
        };
        let dir = data.path().join("data");
        std::fs::create_dir_all(&dir).unwrap();
        let settings = PluginSettings::new(data.path().join("settings.json"), declared_settings());
        let plugin = runtime.load(&record, &wasm, dir, settings).unwrap();
        Probe {
            plugin,
            data,
            supervisor,
            changes,
        }
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
        self.run_within(command, args, cancel, grace, CALL_BUDGET)
            .await
    }

    async fn run_within(
        &self,
        command: &str,
        args: Value,
        cancel: &Cancel,
        grace: Duration,
        budget: Duration,
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
            .run_command(&CallChain::default(), command, &args, control, budget)
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
    assert!(
        matches!(&unknown, Err(CallError::Unsupported(message)) if message.contains("nope")),
        "{unknown:?}"
    );
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
    let Err(CallError::Unsupported(message)) = unknown else {
        panic!("{unknown:?}");
    };
    assert!(message.contains("nope"), "{message}");
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

    let Err(CallError::Guest(error)) = outcome else {
        panic!("{outcome:?}");
    };
    let message = error.to_string();
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
fn a_words_argument_takes_a_list_of_strings() {
    let command = CommandContribution {
        id: "players allow".into(),
        args: vec![CommandArg {
            name: "names".into(),
            kind: CommandArgKind::Words,
            required: true,
            help: String::new(),
        }],
        ..CommandContribution::default()
    };
    let check = |args| command_args("demo", &command, None, &args);

    assert_eq!(
        check(json!({ "names": ["a", "b"] })).unwrap(),
        json!({ "names": ["a", "b"] })
    );
    assert!(refused(check(json!({}))).contains("`names` is required"));
    assert!(refused(check(json!({ "names": "a" }))).contains("must be a list of strings"));
    assert!(refused(check(json!({ "names": ["a", 1] }))).contains("must be a list of strings"));
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
    let Err(CallError::Guest(error)) = denied else {
        panic!("{denied:?}");
    };
    assert!(matches!(&error, GuestError::Denied(_)), "{error:?}");
    let message = error.to_string();
    assert!(message.contains("entry:read"), "{message}");

    probe.set_mode("idle");
    probe.init().await.unwrap();
}

async fn http_refusal(probe: &Probe, url: &str) -> String {
    probe.set_mode(&format!("get {url}"));
    match probe.init().await {
        Err(CallError::Guest(error)) => error.to_string(),
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

fn reading(files: &[&str]) -> Probe {
    Probe::load_with(
        Grant {
            permissions: [(
                Permission::EntryRead,
                PermissionScope::of(ScopeKind::Files, files.iter().copied()),
            )]
            .into(),
            ..Grant::default()
        },
        BTreeMap::new(),
    )
}

#[tokio::test]
async fn a_narrowed_channel_serves_only_what_the_scope_covers() {
    let probe = reading(&["ops.json"]);
    probe.set_mode("read ops.json");
    probe.init().await.expect("a listed file");

    for refused in [
        "server.properties",
        "whitelist.json",
        "../ops.json",
        "ops.json/",
    ] {
        probe.set_mode(&format!("read {refused}"));
        let outcome = probe.init().await;
        assert!(
            matches!(outcome, Err(CallError::Guest(GuestError::Denied(_)))),
            "{refused}: {outcome:?}"
        );
    }
}

#[tokio::test]
async fn a_narrowed_channel_is_closed_to_a_plugin_whose_scope_names_nothing() {
    let probe = reading(&[]);
    probe.set_mode("read ops.json");

    let outcome = probe.init().await;

    assert!(
        matches!(outcome, Err(CallError::Guest(GuestError::Denied(_)))),
        "{outcome:?}"
    );
}

#[tokio::test]
async fn a_typed_daemon_failure_reaches_the_guest_as_its_own_case() {
    let probe = Probe::load(&[Permission::EntryRead]);
    probe.set_mode("list-instances");

    let outcome = probe.init().await;

    let Err(CallError::Guest(GuestError::Offline(detail))) = outcome else {
        panic!("{outcome:?}");
    };
    assert!(detail.is_some());
}

#[tokio::test]
async fn a_command_failure_keeps_its_case_for_the_host() {
    let probe = Probe::load(&[]);
    let cases = [
        ("offline", GuestError::Offline(Some("no route".into()))),
        (
            "upstream",
            GuestError::Upstream {
                status: Some(503),
                retry_after: Some(30),
                message: "the service is down".into(),
            },
        ),
        ("rate-limited", GuestError::RateLimited(Some(7))),
        ("sign-in", GuestError::SignInRequired),
        (
            "not-configured",
            GuestError::NotConfigured("api-key".into()),
        ),
        (
            "missing",
            GuestError::MissingRequirement {
                name: "git".into(),
                url: Some("https://git-scm.com".into()),
            },
        ),
        ("busy", GuestError::Busy("the entry is claimed".into())),
        ("not-found", GuestError::NotFound("no such thing".into())),
    ];
    for (kind, expected) in cases {
        let (outcome, _) = probe
            .run("fail", json!({ "kind": kind }), &Cancel::new(), GRACE)
            .await;
        let Err(CallError::Guest(error)) = outcome else {
            panic!("{kind}: {outcome:?}");
        };
        assert_eq!(error, expected, "{kind}");
    }
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

#[tokio::test]
async fn a_set_setting_reaches_the_guest_and_only_its_own_plugin_reads_it() {
    let probe = Probe::load(&[]);
    let path = probe.data.path().join("settings.json");
    let own = PluginSettings::new(path.clone(), declared_settings());
    own.set("api-key", json!("hunter2")).unwrap();
    probe
        .plugin
        .on_settings_changed(&CallChain::default(), "api-key", CALL_BUDGET)
        .await
        .unwrap();

    let recorded = std::fs::read(probe.data.path().join("data/settings-changed.json")).unwrap();
    let recorded: Value = serde_json::from_slice(&recorded).unwrap();
    assert_eq!(recorded, json!({ "key": "api-key", "value": "hunter2" }));

    let other = PluginSettings::new(probe.data.path().join("other.json"), declared_settings());
    assert_eq!(other.get("api-key"), None);
    assert!(own.list()[0].value.is_none());
}

#[tokio::test]
async fn a_plugin_sets_its_own_declared_settings_and_is_not_told_of_its_own_write() {
    let probe = Probe::load(&[]);
    async fn call(probe: &Probe, key: &str, value: Value) -> Result<Value, CallError> {
        let args = json!({ "key": key, "value": value });
        probe
            .run("set-setting", args, &Cancel::new(), GRACE)
            .await
            .0
    }
    let own = PluginSettings::new(probe.data.path().join("settings.json"), declared_settings());

    call(&probe, "api-key", json!("s3cret")).await.unwrap();
    assert_eq!(own.get("api-key"), Some(json!("s3cret")));
    assert!(own.list()[0].set && own.list()[0].value.is_none());
    assert_eq!(probe.changes.load(Ordering::Relaxed), 1);

    let wrong_kind = call(&probe, "api-key", json!(7)).await;
    assert!(
        matches!(wrong_kind, Err(CallError::Guest(GuestError::Invalid(_)))),
        "{wrong_kind:?}"
    );
    let undeclared = call(&probe, "nope", json!(1)).await;
    assert!(
        matches!(undeclared, Err(CallError::Guest(GuestError::NotFound(_)))),
        "{undeclared:?}"
    );
    assert_eq!(own.get("api-key"), Some(json!("s3cret")));
    assert_eq!(probe.changes.load(Ordering::Relaxed), 1);

    call(&probe, "api-key", Value::Null).await.unwrap();
    assert_eq!(own.get("api-key"), None);
    assert!(!probe
        .data
        .path()
        .join("data/settings-changed.json")
        .exists());
}

mod locking {
    use super::*;

    async fn bump(probe: &Probe, args: Value) -> Result<Value, CallError> {
        probe.run("bump", args, &Cancel::new(), GRACE).await.0
    }

    fn counter(probe: &Probe) -> Value {
        let bytes = std::fs::read(probe.data.path().join("data/counter.json")).unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn overlapping_updates_of_one_key_all_land() {
        let probe = Probe::load(&[]);
        let args = json!({ "name": "counter", "pauseMs": 20 });
        let calls = (0..8).map(|_| bump(&probe, args.clone()));

        let outcomes = futures_util::future::join_all(calls).await;

        let mut seen: Vec<u64> = outcomes
            .into_iter()
            .map(|outcome| outcome.unwrap().as_u64().unwrap())
            .collect();
        seen.sort_unstable();
        assert_eq!(seen, (1..=8).collect::<Vec<_>>());
        assert_eq!(counter(&probe), json!(8));
    }

    #[tokio::test]
    async fn different_keys_do_not_wait_for_each_other() {
        let probe = Probe::load(&[]);
        let slow = bump(&probe, json!({ "name": "slow", "pauseMs": 400 }));
        let quick = async {
            tokio::time::sleep(Duration::from_millis(100)).await;
            let started = std::time::Instant::now();
            bump(&probe, json!({ "name": "counter" })).await.unwrap();
            started.elapsed()
        };
        let (slow, quick) = tokio::join!(slow, quick);
        slow.unwrap();
        assert!(quick < Duration::from_millis(300), "{quick:?}");
    }

    #[tokio::test]
    async fn a_trap_releases_the_lock() {
        let probe = Probe::load(&[]);
        let trapped = bump(&probe, json!({ "name": "counter", "trap": true })).await;
        assert!(matches!(trapped, Err(CallError::Trap(_))), "{trapped:?}");

        let started = std::time::Instant::now();
        let next = bump(&probe, json!({ "name": "counter" })).await.unwrap();
        assert_eq!(next, json!(1));
        assert!(started.elapsed() < Duration::from_secs(3));
    }

    #[tokio::test]
    async fn a_timeout_releases_the_lock() {
        let probe = Probe::load(&[]);
        let (hung, _) = probe
            .run_within(
                "bump",
                json!({ "name": "counter", "hang": true }),
                &Cancel::new(),
                GRACE,
                Duration::from_millis(300),
            )
            .await;
        assert!(matches!(hung, Err(CallError::Timeout(_))), "{hung:?}");

        let started = std::time::Instant::now();
        let next = bump(&probe, json!({ "name": "counter" })).await.unwrap();
        assert_eq!(next, json!(1));
        assert!(started.elapsed() < Duration::from_secs(3));
    }

    #[tokio::test]
    async fn a_lock_name_that_is_no_data_key_is_refused() {
        let probe = Probe::load(&[]);
        let refused = bump(&probe, json!({ "name": "../x" })).await;
        assert!(
            matches!(refused, Err(CallError::Guest(GuestError::Invalid(_)))),
            "{refused:?}"
        );
    }
}

#[cfg(unix)]
mod pushed {
    use tokio::io::AsyncWriteExt;
    use tokio::net::UnixListener;

    use super::*;

    #[tokio::test]
    async fn a_connections_readable_and_closed_events_reach_the_guest() {
        let name = format!(
            "probe-ipc-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let path = std::path::Path::new("/tmp").join(&name);
        let listener = UnixListener::bind(&path).unwrap();
        let grant = Grant {
            permissions: [(
                Permission::IpcConnect,
                PermissionScope::of(
                    ScopeKind::Endpoints,
                    [format!("{}*", &name[..name.len() - 1])],
                ),
            )]
            .into(),
            contributions: PermissionScope::of(ScopeKind::Events, ["ipc.*"]),
            ..Grant::default()
        };
        let probe = Probe::load_with(grant, BTreeMap::new());
        let mut pushed = probe.plugin.pushed();

        let cancel = Cancel::new();
        let (id, accepted) = tokio::join!(
            probe.run("ipc-open", json!({ "endpoint": name }), &cancel, GRACE),
            listener.accept()
        );
        let id = id.0.unwrap();
        let (mut peer, _) = accepted.unwrap();
        peer.write_all(b"hello").await.unwrap();

        let next = async {
            tokio::time::timeout(CALL_BUDGET, pushed.recv())
                .await
                .unwrap()
        };
        let readable = next.await.unwrap();
        assert_eq!(readable.topic, "ipc.readable");
        assert_eq!(readable.payload, json!({ "connection": id }));
        probe
            .plugin
            .on_event(
                &CallChain::default(),
                readable.topic,
                &readable.payload,
                CALL_BUDGET,
            )
            .await
            .unwrap();
        let received = std::fs::read(probe.data.path().join("data/ipc-received.json")).unwrap();
        assert_eq!(serde_json::from_slice::<Value>(&received).unwrap(), "hello");

        drop(peer);
        let closed = tokio::time::timeout(CALL_BUDGET, pushed.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(closed.topic, "ipc.closed");
        assert_eq!(closed.payload, json!({ "connection": id }));
        std::fs::remove_file(path).unwrap();
    }
}

#[cfg(unix)]
mod programs {
    use super::*;
    use engine::{owner_of, resolve_on_path, target_triple};
    use proto::plugin::Program;

    fn spawning(programs: Vec<Program>) -> Grant {
        let mut scope = PermissionScope::default();
        scope.extend(ScopeKind::Programs, programs.iter().map(Program::entry));
        Grant {
            permissions: [(Permission::ProcessSpawn, scope)].into(),
            programs: programs
                .into_iter()
                .map(|program| (program.id.clone(), program))
                .collect(),
            ..Grant::default()
        }
    }

    fn on_path(id: &str, command: &str, args: &[&str], env: &[&str]) -> Program {
        Program {
            id: id.into(),
            command: command.into(),
            args: args.iter().map(|arg| arg.to_string()).collect(),
            env: env.iter().map(|name| name.to_string()).collect(),
            ..Program::default()
        }
    }

    fn pinned(programs: &[&Program]) -> BTreeMap<String, PathBuf> {
        programs
            .iter()
            .map(|program| {
                let path = resolve_on_path(&program.command).expect("on PATH");
                (program.id.clone(), path)
            })
            .collect()
    }

    async fn call(probe: &Probe, command: &str, args: Value) -> Result<Value, CallError> {
        probe.run(command, args, &Cancel::new(), GRACE).await.0
    }

    fn refused(outcome: Result<Value, CallError>) -> String {
        match outcome {
            Err(CallError::Guest(error)) => error.to_string(),
            other => panic!("expected a refusal: {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_declared_program_missing_from_this_machine_is_a_missing_requirement() {
        let echo = on_path("echo", "echo", &["hello", "*"], &[]);
        let probe = Probe::load_with(spawning(vec![echo]), BTreeMap::new());

        let outcome = call(
            &probe,
            "exec",
            json!({ "program": "echo", "args": ["hello", "world"] }),
        )
        .await;

        let Err(CallError::Guest(GuestError::MissingRequirement { name, url: None })) = outcome
        else {
            panic!("{outcome:?}");
        };
        assert_eq!(name, "echo");
    }

    #[tokio::test]
    async fn a_declared_program_runs_and_anything_else_is_refused() {
        let echo = on_path("echo", "echo", &["hello", "*"], &[]);
        let probe = Probe::load_with(spawning(vec![echo.clone()]), pinned(&[&echo]));

        let ran = call(
            &probe,
            "exec",
            json!({ "program": "echo", "args": ["hello", "world"] }),
        )
        .await
        .unwrap();
        assert_eq!(ran["status"], json!(0));
        assert_eq!(ran["stdout"], json!("hello world\n"));

        let undeclared = call(&probe, "exec", json!({ "program": "ls", "args": [] })).await;
        assert!(refused(undeclared).contains("not a program"));
        let outside = call(
            &probe,
            "exec",
            json!({ "program": "echo", "args": ["bye"] }),
        )
        .await;
        assert!(refused(outside).contains("outside"));
    }

    #[tokio::test]
    async fn without_the_permission_nothing_runs() {
        let echo = on_path("echo", "echo", &["**"], &[]);
        let grant = Grant {
            programs: [("echo".to_string(), echo.clone())].into(),
            ..Grant::default()
        };
        let probe = Probe::load_with(grant, pinned(&[&echo]));

        let denied = call(&probe, "exec", json!({ "program": "echo", "args": [] })).await;
        assert!(refused(denied).contains("process:spawn"));
    }

    #[tokio::test]
    async fn an_undeclared_environment_name_is_refused() {
        let sleep = on_path("sleeper", "sleep", &["*"], &["ALLOWED"]);
        let probe = Probe::load_with(spawning(vec![sleep.clone()]), pinned(&[&sleep]));

        let outcome = call(
            &probe,
            "spawn",
            json!({ "program": "sleeper", "args": ["30"], "env": { "SECRET": "x" } }),
        )
        .await;
        assert!(refused(outcome).contains("SECRET"));
        assert!(probe.supervisor.list().is_empty());
    }

    fn shell(path: &[&str]) -> Program {
        Program {
            path: path.iter().map(|id| id.to_string()).collect(),
            ..on_path("shell", "sh", &["**"], &["ALLOWED"])
        }
    }

    fn gone(pid: u32) -> bool {
        match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
            Ok(stat) => stat
                .rsplit_once(')')
                .is_some_and(|(_, rest)| rest.trim_start().starts_with('Z')),
            Err(_) => true,
        }
    }

    async fn pid_of(file: &std::path::Path) -> u32 {
        for _ in 0..200 {
            if let Some(pid) = std::fs::read_to_string(file)
                .ok()
                .and_then(|text| text.trim().parse().ok())
            {
                return pid;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("the program never wrote its grandchild's pid");
    }

    const SPAWN_GRANDCHILD: &str =
        "sleep 300 & echo $! > ${data}/grandchild; echo $$ > ${data}/child; wait";

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn a_cancelled_call_kills_the_program_and_what_it_started() {
        let shell = shell(&[]);
        let probe = Probe::load_with(spawning(vec![shell.clone()]), pinned(&[&shell]));
        let cancel = Cancel::new();
        let args =
            json!({ "program": "shell", "args": ["-c", SPAWN_GRANDCHILD], "timeoutMs": 120_000 });
        let data = probe.data.path().join("data");

        let (outcome, ()) = tokio::join!(
            async { probe.run("exec", args, &cancel, GRACE).await.0 },
            async {
                pid_of(&data.join("child")).await;
                cancel.cancel();
            }
        );
        assert!(outcome.is_err());
        let pids = [
            pid_of(&data.join("child")).await,
            pid_of(&data.join("grandchild")).await,
        ];
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(pids.into_iter().all(gone), "{pids:?} outlived the call");
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn a_program_is_killed_with_its_tree_when_the_call_budget_ends() {
        let shell = shell(&[]);
        let probe = Probe::load_with(spawning(vec![shell.clone()]), pinned(&[&shell]));
        let args =
            json!({ "program": "shell", "args": ["-c", SPAWN_GRANDCHILD], "timeoutMs": 600_000 });
        let started = std::time::Instant::now();

        let (outcome, _) = probe
            .run_within("exec", args, &Cancel::new(), GRACE, Duration::from_secs(1))
            .await;

        assert!(outcome.is_err());
        assert!(started.elapsed() < Duration::from_secs(10));
        let data = probe.data.path().join("data");
        let pids = [
            pid_of(&data.join("child")).await,
            pid_of(&data.join("grandchild")).await,
        ];
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(pids.into_iter().all(gone), "{pids:?} outlived the call");
    }

    #[tokio::test]
    async fn a_program_path_holds_only_the_directories_of_the_programs_it_lists() {
        let tools = tempfile::tempdir().unwrap();
        let tool = tools.path().join("mytool");
        std::fs::copy(resolve_on_path("true").unwrap(), &tool).unwrap();
        let mytool = on_path("mytool", "mytool", &["**"], &[]);
        let shell = shell(&["mytool"]);
        let mut paths = pinned(&[]);
        paths.extend([
            ("mytool".to_string(), tool),
            ("shell".to_string(), resolve_on_path("sh").unwrap()),
        ]);
        let probe = Probe::load_with(spawning(vec![shell, mytool]), paths);

        let ran = call(
            &probe,
            "exec",
            json!({ "program": "shell", "args": ["-c", "echo $PATH"] }),
        )
        .await
        .unwrap();
        assert_eq!(
            ran["stdout"],
            json!(format!("{}\n", tools.path().display()))
        );
    }

    #[tokio::test]
    async fn exec_refuses_an_undeclared_environment_name_and_passes_a_declared_one() {
        let shell = shell(&[]);
        let probe = Probe::load_with(spawning(vec![shell.clone()]), pinned(&[&shell]));
        let echo =
            |env: Value| json!({ "program": "shell", "args": ["-c", "echo $ALLOWED"], "env": env });

        let refused_name = call(&probe, "exec", echo(json!({ "SECRET": "x" }))).await;
        assert!(refused(refused_name).contains("SECRET"));
        let refused_path = call(&probe, "exec", echo(json!({ "PATH": "/tmp" }))).await;
        assert!(refused(refused_path).contains("PATH"));
        let ran = call(&probe, "exec", echo(json!({ "ALLOWED": "yes" })))
            .await
            .unwrap();
        assert_eq!(ran["stdout"], json!("yes\n"));
    }

    #[tokio::test]
    async fn a_mount_placeholder_expands_for_a_mount_the_call_has_and_is_refused_otherwise() {
        let echo = on_path("echo", "echo", &["**"], &[]);
        let probe = Probe::load_with(spawning(vec![echo.clone()]), pinned(&[&echo]));

        let ran = call(
            &probe,
            "exec",
            json!({ "program": "echo", "args": ["${data}/x", "$HOME"] }),
        )
        .await
        .unwrap();
        let data = probe.data.path().join("data");
        assert_eq!(
            ran["stdout"],
            json!(format!("{}/x $HOME\n", data.display()))
        );
        for mount in ["${vanilla}", "${build}"] {
            let outcome = call(
                &probe,
                "exec",
                json!({ "program": "echo", "args": [mount] }),
            )
            .await;
            assert!(refused(outcome).contains("no `"), "{mount}");
        }
    }

    fn data_program(sha256: &str) -> Program {
        Program {
            id: "agent".into(),
            command: "bin/agent".into(),
            args: vec!["**".into()],
            sha256: [(target_triple(), sha256.to_string())].into(),
            ..Program::default()
        }
    }

    fn sha256_of(command: &str) -> String {
        let bytes = std::fs::read(resolve_on_path(command).unwrap()).unwrap();
        Sha256::digest(&bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    /// Puts a copy of the `PATH` program `command` at the plugin's `data/bin/agent`.
    fn plant(probe: &Probe, command: &str) -> PathBuf {
        let planted = probe.data.path().join("data/bin/agent");
        std::fs::create_dir_all(planted.parent().unwrap()).unwrap();
        std::fs::copy(resolve_on_path(command).unwrap(), &planted).unwrap();
        planted
    }

    #[tokio::test]
    async fn a_data_program_whose_hash_differs_is_refused() {
        let probe = Probe::load_with(
            spawning(vec![data_program(&"0".repeat(64))]),
            BTreeMap::new(),
        );
        plant(&probe, "echo");

        let outcome = call(
            &probe,
            "exec",
            json!({ "program": "agent", "args": ["hi"] }),
        )
        .await;
        assert!(refused(outcome).contains("sha256"));
    }

    #[tokio::test]
    async fn rewriting_the_data_program_after_it_ran_changes_nothing() {
        let probe = Probe::load_with(
            spawning(vec![data_program(&sha256_of("echo"))]),
            BTreeMap::new(),
        );
        let planted = plant(&probe, "echo");

        let first = call(
            &probe,
            "exec",
            json!({ "program": "agent", "args": ["hi"] }),
        )
        .await
        .unwrap();
        assert_eq!(first["stdout"], json!("hi\n"));

        std::fs::copy(resolve_on_path("true").unwrap(), &planted).unwrap();
        let second = call(
            &probe,
            "exec",
            json!({ "program": "agent", "args": ["hi"] }),
        )
        .await
        .unwrap();
        assert_eq!(
            second["stdout"],
            json!("hi\n"),
            "the host's verified copy runs"
        );
    }

    #[tokio::test]
    async fn a_spawned_agent_is_supervised_found_again_and_stopped_with_its_owner() {
        let sleep = on_path("sleeper", "sleep", &["*"], &[]);
        let probe = Probe::load_with(spawning(vec![sleep.clone()]), pinned(&[&sleep]));

        let id = call(
            &probe,
            "spawn",
            json!({ "program": "sleeper", "args": ["30"] }),
        )
        .await
        .unwrap();
        let id = id.as_str().unwrap().to_string();
        let listed = probe.supervisor.status(&id).expect("supervised");
        assert_eq!(listed.owner.as_deref(), Some(owner_of("probe").as_str()));
        assert!(
            listed.args.is_empty(),
            "a plugin's arguments are never recorded"
        );

        let running = call(&probe, "running", json!({})).await.unwrap();
        assert_eq!(running, json!([id.clone()]), "a fresh call finds it");

        let stopped = probe.supervisor.stop_owned(&owner_of("probe"));
        assert_eq!(stopped, std::slice::from_ref(&id));
        assert!(probe.supervisor.stop_and_wait(&stopped).await);
        let running = call(&probe, "running", json!({})).await.unwrap();
        assert_eq!(running, json!([]));
    }
}

async fn java_releases_through(
    network: &[&str],
    url: &str,
) -> Result<Vec<proto::java::JavaRelease>, anyhow::Error> {
    java_releases_in_mode(network, &format!("get {url}")).await
}

async fn java_releases_in_mode(
    network: &[&str],
    mode: &str,
) -> Result<Vec<proto::java::JavaRelease>, anyhow::Error> {
    let (engine, _home, _data) = register_java_probe_in_mode(network, mode, false).await;
    engine.java().releases("probe.reach").await
}

async fn register_java_probe(
    network: &[&str],
    url: &str,
    official: bool,
) -> (engine::Engine, tempfile::TempDir, tempfile::TempDir) {
    register_java_probe_in_mode(network, &format!("get {url}"), official).await
}

async fn register_java_probe_in_mode(
    network: &[&str],
    mode: &str,
    official: bool,
) -> (engine::Engine, tempfile::TempDir, tempfile::TempDir) {
    let probe = Probe::load_with_network(&[Permission::NetworkConnect], network);
    probe.set_mode(mode);
    let Probe {
        plugin,
        data,
        supervisor: _supervisor,
        changes: _changes,
    } = probe;
    let manifest = proto::plugin::Manifest {
        id: "probe".into(),
        name: "Probe Net".into(),
        contributes: proto::plugin::Contributions {
            providers: vec![proto::plugin::ProviderContribution {
                kind: "java".into(),
                id: "reach".into(),
            }],
            ..Default::default()
        },
        ..Default::default()
    };
    let home = tempfile::tempdir().unwrap();
    let engine = crate::fixture::engine(home.path(), vec![], vec![]);
    engine::Contributions::connect(&Arc::new(plugin), &manifest, official)
        .await
        .register(&engine, "probe");
    (engine, home, data)
}

#[tokio::test]
async fn an_official_plugins_provider_registers_under_its_bare_id() {
    let (engine, _home, _data) =
        register_java_probe(&["example.com"], "https://example.com/", true).await;
    assert!(engine.java().provider("reach").is_ok());
    assert!(engine.java().provider("probe.reach").is_err());
}

#[tokio::test]
async fn a_third_party_plugins_provider_stays_prefixed() {
    let (engine, _home, _data) =
        register_java_probe(&["example.com"], "https://example.com/", false).await;
    assert!(engine.java().provider("probe.reach").is_ok());
    assert!(engine.java().provider("reach").is_err());
}

#[tokio::test]
async fn a_provider_that_cannot_reach_its_host_fails_as_an_upstream_service() {
    let error = java_releases_through(&["unreachable.invalid"], "https://unreachable.invalid/")
        .await
        .expect_err("the host does not resolve");

    let Some(ErrorInfo::Offline {
        service: Some(Service::Provider(provider)),
    }) = error.downcast_ref::<ErrorInfo>()
    else {
        panic!("{error:#}");
    };
    assert_eq!(provider.id, "probe.reach");
    assert_eq!(provider.name, "Probe Net");
}

fn provider_service() -> Service {
    Service::Provider(proto::provider::Provider::new("probe.reach", "Probe Net"))
}

#[tokio::test]
async fn a_provider_that_answers_offline_is_attributed_to_itself() {
    let error = java_releases_in_mode(&[], "fail offline")
        .await
        .expect_err("the guest says it is offline");

    assert_eq!(
        error.downcast_ref::<ErrorInfo>(),
        Some(&ErrorInfo::Offline {
            service: Some(provider_service())
        }),
        "{error:#}"
    );
}

#[tokio::test]
async fn a_provider_that_meets_an_upstream_failure_is_named_in_it() {
    let upstream = java_releases_in_mode(&[], "fail upstream")
        .await
        .expect_err("the guest reports a 503");
    let Some(ErrorInfo::Upstream { service, detail }) = upstream.downcast_ref::<ErrorInfo>() else {
        panic!("{upstream:#}");
    };
    assert_eq!(service, &provider_service());
    assert!(detail.contains("the service is down"), "{detail}");

    let limited = java_releases_in_mode(&[], "fail rate-limited")
        .await
        .expect_err("the guest is rate limited");
    assert!(
        matches!(
            limited.downcast_ref::<ErrorInfo>(),
            Some(ErrorInfo::Upstream { service, .. }) if service == &provider_service()
        ),
        "{limited:#}"
    );
}

#[tokio::test]
async fn a_provider_that_needs_a_sign_in_or_a_setting_says_so() {
    let sign_in = java_releases_in_mode(&[], "fail sign-in")
        .await
        .expect_err("the guest needs a sign-in");
    assert_eq!(
        sign_in.downcast_ref::<ErrorInfo>(),
        Some(&ErrorInfo::SignInRequired)
    );

    let unset = java_releases_in_mode(&[], "fail not-configured")
        .await
        .expect_err("the guest needs a setting");
    assert!(
        matches!(
            unset.downcast_ref::<ErrorInfo>(),
            Some(ErrorInfo::PluginSettingInvalid { plugin, key, .. })
                if plugin == "probe" && key == "api-key"
        ),
        "{unset:#}"
    );
}

#[tokio::test]
async fn a_provider_that_fails_for_its_own_reasons_is_still_a_plugin_failure() {
    let error = java_releases_through(&["example.com"], "https://other.example.org/")
        .await
        .expect_err("the host is outside the allowlist");

    assert!(
        matches!(
            error.downcast_ref::<ErrorInfo>(),
            Some(ErrorInfo::PluginCallFailed { .. })
        ),
        "{error:#}"
    );
}

mod manifest_table {
    use proto::plugin::{Manifest, Permission, ScopeKind};

    const HEAD: &str =
        "id = \"demo\"\nversion = \"1.0.0\"\napi = \"^1.0.0\"\nwasm = \"plugin.wasm\"\n";

    fn parse(permissions: &str) -> Result<Manifest, toml::de::Error> {
        toml::from_str(&format!("{HEAD}{permissions}"))
    }

    #[test]
    fn a_permission_is_a_key_and_its_scope_a_table() {
        assert!(
            parse("[permissions]\nnetwork = { hosts = [\"a.example.com\"] }\n").is_err(),
            "`network` is not a permission name"
        );

        let manifest = parse(
            r#"
[permissions]
"content:read" = {}
"network:connect" = { hosts = ["a.example.com"], unhashed = ["a.example.com"] }
"entry:read" = { files = ["ops.json"] }
"#,
        )
        .expect("parses");
        manifest.validate().expect("valid");
        let grant = manifest.grant();
        assert!(grant.holds(Permission::ContentRead));
        assert_eq!(grant.hosts().collect::<Vec<_>>(), ["a.example.com"]);
        assert!(grant.covers(Permission::EntryRead, ScopeKind::Files, "ops.json"));
        assert!(!grant.covers(Permission::EntryRead, ScopeKind::Files, "ops.json "));
    }

    #[test]
    fn an_unknown_scope_kind_or_a_bare_list_is_refused() {
        assert!(parse("[permissions]\n\"entry:read\" = { everything = [\"*\"] }\n").is_err());
        assert!(parse("[permissions]\n\"entry:read\" = true\n").is_err());
        assert!(parse("permissions = [\"entry:read\"]\n").is_err());
    }

    #[test]
    fn the_old_top_level_lists_grant_nothing() {
        let manifest = parse("network = [\"a.example.com\"]\nfiles = [\"ops.json\"]\n")
            .expect("unknown keys are ignored");
        let grant = manifest.grant();
        assert_eq!(grant.hosts().count(), 0);
        assert!(grant.permissions.is_empty());
    }
}

mod downloads {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use super::*;

    const BODY: &str = "hello world\n";
    const SHA1: &str = "22596363b3de40b06f981fb85d82312e8c0ed511";
    const SHA256: &str = "a948904f2f0f479b8f8197694b30184b0d2ed1c1cd2a1ec0fb85d299a192a447";

    struct Origin {
        port: u16,
        served: Arc<AtomicUsize>,
    }

    impl Origin {
        async fn start() -> Origin {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let port = listener.local_addr().unwrap().port();
            let served = Arc::new(AtomicUsize::new(0));
            let counter = served.clone();
            tokio::spawn(async move {
                loop {
                    let Ok((mut socket, _)) = listener.accept().await else {
                        return;
                    };
                    let counter = counter.clone();
                    tokio::spawn(async move {
                        let mut request = vec![0u8; 2048];
                        let read = socket.read(&mut request).await.unwrap_or(0);
                        let request = String::from_utf8_lossy(&request[..read]).into_owned();
                        let path = request.split_whitespace().nth(1).unwrap_or("/");
                        counter.fetch_add(1, Ordering::SeqCst);
                        let reply = match path {
                            "/file.bin" => format!(
                                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{BODY}",
                                BODY.len()
                            ),
                            "/hop" => format!(
                                "HTTP/1.1 302 Found\r\nLocation: http://localhost:{port}/file.bin\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                            ),
                            "/stall" => "HTTP/1.1 200 OK\r\nContent-Length: 1000000\r\nConnection: close\r\n\r\n".to_string(),
                            "/busy" => "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string(),
                            _ => "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string(),
                        };
                        let _ = socket.write_all(reply.as_bytes()).await;
                        let _ = socket.flush().await;
                        if path == "/stall" {
                            tokio::time::sleep(Duration::from_secs(60)).await;
                        }
                    });
                }
            });
            Origin { port, served }
        }

        fn url(&self, path: &str) -> String {
            format!("http://127.0.0.1:{}{path}", self.port)
        }

        fn requests(&self) -> usize {
            self.served.load(Ordering::SeqCst)
        }
    }

    fn fetcher(hosts: &[&str], unhashed: &[&str]) -> Probe {
        let mut network = PermissionScope::of(ScopeKind::Hosts, hosts.iter().copied());
        network.extend(ScopeKind::Unhashed, unhashed.iter().copied());
        Probe::load_with(
            Grant {
                permissions: [(Permission::NetworkConnect, network)].into(),
                ..Grant::default()
            },
            BTreeMap::new(),
        )
    }

    async fn download(probe: &Probe, args: Value) -> Result<Value, GuestError> {
        match probe.run("download", args, &Cancel::new(), GRACE).await.0 {
            Ok(value) => Ok(value),
            Err(CallError::Guest(error)) => Err(error),
            Err(other) => panic!("{other:?}"),
        }
    }

    fn data_file(probe: &Probe, name: &str) -> PathBuf {
        probe.data.path().join("data").join(name)
    }

    fn left_in_data(probe: &Probe) -> Vec<std::ffi::OsString> {
        std::fs::read_dir(probe.data.path().join("data"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect()
    }

    #[tokio::test]
    async fn a_download_with_its_checksum_lands_whole_and_reports_progress() {
        let origin = Origin::start().await;
        let probe = fetcher(&["127.0.0.1"], &[]);

        let (outcome, reports) = probe
            .run(
                "download",
                json!({ "url": origin.url("/file.bin"), "dest": "/data/tool.jar", "sha1": SHA1 }),
                &Cancel::new(),
                GRACE,
            )
            .await;

        assert_eq!(
            outcome.unwrap(),
            json!({ "size": BODY.len(), "present": true })
        );
        assert_eq!(
            std::fs::read_to_string(data_file(&probe, "tool.jar")).unwrap(),
            BODY
        );
        let latest = reports.borrow().clone().unwrap();
        assert_eq!(
            (latest.fraction, latest.message.as_str()),
            (Some(1.0), "downloading tool.jar")
        );
    }

    #[tokio::test]
    async fn a_checksum_the_engine_already_holds_is_served_from_its_cache() {
        let origin = Origin::start().await;
        let probe = fetcher(&["127.0.0.1"], &[]);
        let args =
            |dest: &str| json!({ "url": origin.url("/file.bin"), "dest": dest, "sha256": SHA256 });

        download(&probe, args("/data/one.bin")).await.unwrap();
        download(&probe, args("/data/two.bin")).await.unwrap();

        assert_eq!(origin.requests(), 1);
        assert_eq!(
            std::fs::read_to_string(data_file(&probe, "two.bin")).unwrap(),
            BODY
        );
    }

    #[tokio::test]
    async fn a_checksum_mismatch_fails_and_leaves_nothing() {
        let origin = Origin::start().await;
        let probe = fetcher(&["127.0.0.1"], &[]);

        let error = download(
            &probe,
            json!({ "url": origin.url("/file.bin"), "dest": "/data/tool.jar", "sha1": "0".repeat(40) }),
        )
        .await
        .unwrap_err();

        assert!(matches!(error, GuestError::Failed(_)), "{error:?}");
        assert!(error.to_string().contains("checksum mismatch"), "{error}");
        assert!(left_in_data(&probe).is_empty());
    }

    #[tokio::test]
    async fn a_malformed_checksum_is_invalid() {
        let origin = Origin::start().await;
        let probe = fetcher(&["127.0.0.1"], &[]);

        let error = download(
            &probe,
            json!({ "url": origin.url("/file.bin"), "dest": "/data/tool.jar", "sha1": "abc" }),
        )
        .await
        .unwrap_err();

        assert!(matches!(error, GuestError::Invalid(_)), "{error:?}");
        assert_eq!(origin.requests(), 0);
    }

    #[tokio::test]
    async fn no_checksum_needs_a_host_the_grant_declares_unhashed() {
        let origin = Origin::start().await;
        let args = json!({ "url": origin.url("/file.bin"), "dest": "/data/generated.bin" });

        let refused = download(&fetcher(&["127.0.0.1"], &[]), args.clone())
            .await
            .unwrap_err();
        assert!(matches!(refused, GuestError::Denied(_)), "{refused:?}");
        assert!(refused.to_string().contains("unhashed"), "{refused}");
        assert_eq!(origin.requests(), 0);

        let probe = fetcher(&["127.0.0.1"], &["127.0.0.1"]);
        download(&probe, args).await.unwrap();
        assert!(data_file(&probe, "generated.bin").is_file());
    }

    #[tokio::test]
    async fn a_destination_outside_a_writable_mount_is_denied() {
        let origin = Origin::start().await;
        let probe = fetcher(&["127.0.0.1"], &[]);

        for dest in [
            "/etc/tool.jar",
            "/build/tool.jar",
            "/vanilla/tool.jar",
            "/data/../escape.jar",
            "/data/missing/tool.jar",
            "/data",
            "tool.jar",
        ] {
            let error = download(
                &probe,
                json!({ "url": origin.url("/file.bin"), "dest": dest, "sha1": SHA1 }),
            )
            .await
            .unwrap_err();
            assert!(matches!(error, GuestError::Denied(_)), "{dest}: {error:?}");
        }
        assert_eq!(origin.requests(), 0);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_symlink_at_the_staging_name_is_not_written_through() {
        let origin = Origin::start().await;
        let probe = fetcher(&["127.0.0.1"], &[]);
        let outside = probe.data.path().join("secret.txt");
        std::fs::write(&outside, "secret").unwrap();
        std::os::unix::fs::symlink(&outside, data_file(&probe, "tool.jar.part")).unwrap();

        download(
            &probe,
            json!({ "url": origin.url("/file.bin"), "dest": "/data/tool.jar", "sha1": SHA1 }),
        )
        .await
        .unwrap();

        assert_eq!(std::fs::read_to_string(&outside).unwrap(), "secret");
        assert_eq!(
            std::fs::read_to_string(data_file(&probe, "tool.jar")).unwrap(),
            BODY
        );
    }

    #[tokio::test]
    async fn a_redirect_to_a_host_outside_the_grant_is_refused() {
        let origin = Origin::start().await;
        let probe = fetcher(&["127.0.0.1"], &[]);

        let error = download(
            &probe,
            json!({ "url": origin.url("/hop"), "dest": "/data/tool.jar", "sha1": SHA1 }),
        )
        .await
        .unwrap_err();

        assert!(matches!(error, GuestError::Denied(_)), "{error:?}");
        assert!(error.to_string().contains("localhost"), "{error}");
        assert!(!data_file(&probe, "tool.jar").exists());

        let both = fetcher(&["127.0.0.1", "localhost"], &[]);
        download(
            &both,
            json!({ "url": origin.url("/hop"), "dest": "/data/tool.jar", "sha1": SHA1 }),
        )
        .await
        .expect("granted on every hop");
    }

    #[tokio::test]
    async fn failing_statuses_are_typed() {
        let origin = Origin::start().await;
        let probe = fetcher(&["127.0.0.1"], &[]);
        let fetch =
            |path: &str| json!({ "url": origin.url(path), "dest": "/data/tool.jar", "sha1": SHA1 });

        let missing = download(&probe, fetch("/nothing")).await.unwrap_err();
        assert!(matches!(missing, GuestError::NotFound(_)), "{missing:?}");

        let failing = download(&probe, fetch("/busy")).await.unwrap_err();
        assert!(
            matches!(
                failing,
                GuestError::Upstream {
                    status: Some(500),
                    ..
                }
            ),
            "{failing:?}"
        );
    }

    #[tokio::test]
    async fn without_the_network_permission_nothing_is_fetched() {
        let origin = Origin::start().await;
        let probe = Probe::load(&[]);

        let error = download(
            &probe,
            json!({ "url": origin.url("/file.bin"), "dest": "/data/tool.jar", "sha1": SHA1 }),
        )
        .await
        .unwrap_err();

        assert!(matches!(error, GuestError::Denied(_)), "{error:?}");
        assert_eq!(origin.requests(), 0);
    }

    #[tokio::test]
    async fn a_cancelled_call_stops_the_transfer_and_leaves_nothing() {
        let origin = Origin::start().await;
        let probe = fetcher(&["127.0.0.1"], &[]);
        let cancel = Cancel::new();
        let asker = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(300)).await;
            asker.cancel();
        });

        let started = std::time::Instant::now();
        let (outcome, _) = probe
            .run(
                "download",
                json!({ "url": origin.url("/stall"), "dest": "/data/tool.jar", "sha1": SHA1 }),
                &cancel,
                CALL_BUDGET,
            )
            .await;

        assert!(outcome.is_err(), "{outcome:?}");
        assert!(started.elapsed() < Duration::from_secs(10));
        assert!(left_in_data(&probe).is_empty());
    }

    #[tokio::test]
    async fn a_call_out_of_time_stops_the_transfer() {
        let origin = Origin::start().await;
        let probe = fetcher(&["127.0.0.1"], &[]);

        let started = std::time::Instant::now();
        let (outcome, _) = probe
            .run_within(
                "download",
                json!({ "url": origin.url("/stall"), "dest": "/data/tool.jar", "sha1": SHA1 }),
                &Cancel::new(),
                GRACE,
                Duration::from_millis(600),
            )
            .await;

        assert!(outcome.is_err(), "{outcome:?}");
        assert!(started.elapsed() < Duration::from_secs(10));
    }
}
