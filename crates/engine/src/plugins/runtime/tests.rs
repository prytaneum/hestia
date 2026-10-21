use std::collections::BTreeSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use proto::plugin::{Grant, Permission};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::*;
use crate::checksum::hex;
use crate::plugins::{HostCalls, HostError};

const PLUGIN_ERROR: &str = r#"(variant (case "permission-denied" string) (case "not-found" string) (case "invalid" string) (case "failed" string))"#;

/// A component exporting `lifecycle.init`; `body` runs first. With
/// `calls_host` it can call `host.call` and traps when that answers an error.
fn guest(calls_host: bool, body: &str) -> String {
    let (import, core_import, with) = if calls_host {
        (
            format!(
                r#"(import "hestia:plugin/host@1.0.0" (instance $host
                    (type $e {PLUGIN_ERROR})
                    (export "plugin-error" (type $error (eq $e)))
                    (export "call" (func (param "channel" string) (param "payload" string)
                        (result (result string (error $error)))))))
                (core func $call (canon lower (func $host "call")
                    (memory (core memory $libc "memory"))
                    (realloc (core func $libc "realloc"))))"#
            ),
            r#"(import "host" "call" (func $call (param i32 i32 i32 i32 i32)))
               (data (i32.const 16) "chan") (data (i32.const 32) "{}")"#,
            r#"(with "host" (instance (export "call" (func $call))))"#,
        )
    } else {
        (String::new(), "", "")
    };
    format!(
        r#"(component
            (import "hestia:plugin/types@1.0.0" (instance $types
                (type $e {PLUGIN_ERROR})
                (export "plugin-error" (type (eq $e)))))
            (alias export $types "plugin-error" (type $err))
            (core module $libc
                (memory (export "memory") 1)
                (global $bump (mut i32) (i32.const 256))
                (func (export "realloc") (param i32 i32 i32 i32) (result i32)
                    (local $p i32)
                    (local.set $p (global.get $bump))
                    (global.set $bump (i32.add (global.get $bump) (local.get 3)))
                    (local.get $p)))
            (core instance $libc (instantiate $libc))
            {import}
            (core module $main
                (import "libc" "memory" (memory 1))
                {core_import}
                (func (export "init") (result i32)
                    {body}
                    (i32.store8 (i32.const 128) (i32.const 0))
                    (i32.const 128)))
            (core instance $main (instantiate $main (with "libc" (instance $libc)) {with}))
            (func $init (result (result (error $err)))
                (canon lift (core func $main "init")
                    (memory (core memory $libc "memory"))
                    (realloc (core func $libc "realloc"))))
            (instance $lifecycle (export "init" (func $init)))
            (export "hestia:plugin/lifecycle@1.0.0" (instance $lifecycle)))"#
    )
}

const CALL_HOST_OR_TRAP: &str = r#"
    (call $call (i32.const 16) (i32.const 4) (i32.const 32) (i32.const 2) (i32.const 64))
    (if (i32.load8_u (i32.const 64)) (then unreachable))"#;

#[derive(Default)]
struct TestHost {
    permission: Option<Permission>,
    reached: AtomicUsize,
    stall_first: bool,
    reenter: Mutex<Option<Arc<LoadedPlugin>>>,
    reentry: Mutex<Option<Result<(), CallError>>>,
}

#[async_trait]
impl HostCalls for TestHost {
    fn permission(&self, _channel: &str) -> Option<Permission> {
        self.permission
    }

    async fn call(
        &self,
        _plugin: &str,
        chain: &CallChain,
        _channel: &str,
        _payload: Value,
    ) -> Result<Value, HostError> {
        let nth = self.reached.fetch_add(1, Ordering::SeqCst);
        if self.stall_first && nth == 0 {
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
        let plugin = self.reenter.lock().unwrap().clone();
        if let Some(plugin) = plugin {
            let outcome = plugin.init(chain, Duration::from_secs(5)).await;
            *self.reentry.lock().unwrap() = Some(outcome);
        }
        Ok(json!({}))
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

fn record(wat: &str, permissions: &[Permission]) -> PluginRecord {
    PluginRecord {
        id: "probe".into(),
        wasm_sha256: Some(hex(&Sha256::digest(wat.as_bytes()))),
        grant: Grant {
            permissions: permissions.iter().copied().collect::<BTreeSet<_>>(),
            ..Grant::default()
        },
        ..PluginRecord::default()
    }
}

fn load(
    host: &Arc<TestHost>,
    wat: &str,
    permissions: &[Permission],
) -> (Arc<LoadedPlugin>, tempfile::TempDir) {
    let data = tempfile::tempdir().unwrap();
    let runtime = PluginRuntime::new(host.clone(), data.path().join("cache")).unwrap();
    let plugin = runtime
        .load(
            &record(wat, permissions),
            wat.as_bytes(),
            data.path().into(),
        )
        .unwrap();
    (Arc::new(plugin), data)
}

const BRIEF: Duration = Duration::from_millis(300);

#[tokio::test]
async fn a_call_past_its_timeout_is_cut_off_and_the_next_one_runs_fresh() {
    let host = Arc::new(TestHost {
        permission: Some(Permission::EntryRead),
        stall_first: true,
        ..TestHost::default()
    });
    let (plugin, _data) = load(
        &host,
        &guest(true, CALL_HOST_OR_TRAP),
        &[Permission::EntryRead],
    );

    let first = plugin.init(&CallChain::default(), BRIEF).await;
    assert!(matches!(first, Err(CallError::Timeout(_))), "{first:?}");

    plugin
        .init(&CallChain::default(), Duration::from_secs(5))
        .await
        .unwrap();
    let stats = plugin.stats();
    assert_eq!((stats.calls, stats.timeouts, stats.traps), (2, 1, 0));
}

#[tokio::test]
async fn a_spinning_guest_neither_starves_the_runtime_nor_outlives_its_timeout() {
    let host = Arc::new(TestHost::default());
    let (plugin, _data) = load(&host, &guest(false, "(loop $spin (br $spin))"), &[]);

    let timer = tokio::spawn(async {
        let started = Instant::now();
        tokio::time::sleep(Duration::from_millis(50)).await;
        started.elapsed()
    });
    let started = Instant::now();
    let outcome = plugin.init(&CallChain::default(), BRIEF).await;

    assert!(matches!(outcome, Err(CallError::Timeout(_))), "{outcome:?}");
    assert!(started.elapsed() < Duration::from_secs(2));
    assert!(timer.await.unwrap() < BRIEF);
}

#[tokio::test]
async fn a_plugin_reached_again_through_its_own_host_call_is_refused() {
    let host = Arc::new(TestHost {
        permission: Some(Permission::EntryRead),
        ..TestHost::default()
    });
    let (plugin, _data) = load(
        &host,
        &guest(true, CALL_HOST_OR_TRAP),
        &[Permission::EntryRead],
    );
    *host.reenter.lock().unwrap() = Some(plugin.clone());

    plugin
        .init(&CallChain::default(), Duration::from_secs(5))
        .await
        .unwrap();

    let inner = host.reentry.lock().unwrap().take().unwrap();
    assert!(
        matches!(inner, Err(CallError::Chain(ChainError::Reentry(_)))),
        "{inner:?}"
    );
}

#[tokio::test]
async fn a_channel_the_plugin_lacks_the_permission_for_never_reaches_the_host() {
    for (offered, granted) in [
        (Some(Permission::EntryRead), &[][..]),
        (None, &[Permission::EntryRead][..]),
    ] {
        let host = Arc::new(TestHost {
            permission: offered,
            ..TestHost::default()
        });
        let (plugin, _data) = load(&host, &guest(true, CALL_HOST_OR_TRAP), granted);

        let outcome = plugin.init(&CallChain::default(), BRIEF).await;

        assert!(matches!(outcome, Err(CallError::Trap(_))), "{outcome:?}");
        assert_eq!(host.reached.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn three_traps_take_the_plugin_out_of_service() {
    let host = Arc::new(TestHost::default());
    let (plugin, _data) = load(&host, &guest(false, "unreachable"), &[]);

    for _ in 0..3 {
        let outcome = plugin.init(&CallChain::default(), BRIEF).await;
        assert!(matches!(outcome, Err(CallError::Trap(_))), "{outcome:?}");
    }

    assert!(plugin.is_disabled());
    let outcome = plugin.init(&CallChain::default(), BRIEF).await;
    assert!(matches!(outcome, Err(CallError::Disabled)), "{outcome:?}");
}

#[tokio::test]
async fn a_component_that_differs_from_its_recorded_hash_is_not_compiled() {
    let host = Arc::new(TestHost::default());
    let data = tempfile::tempdir().unwrap();
    let runtime = PluginRuntime::new(host, data.path().join("cache")).unwrap();
    let recorded = guest(false, "");
    let tampered = guest(false, "unreachable");

    let outcome = runtime.load(
        &record(&recorded, &[]),
        tampered.as_bytes(),
        data.path().into(),
    );

    assert!(matches!(outcome, Err(LoadError::HashMismatch { .. })));
}
