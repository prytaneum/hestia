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

fn supervisor(data: &tempfile::TempDir) -> Arc<crate::process::ProcessSupervisor> {
    Arc::new(crate::process::ProcessSupervisor::new(
        data.path().join("processes"),
    ))
}

const PLUGIN_ERROR: &str = r#"(variant (case "permission-denied" string) (case "not-found" string) (case "invalid" string) (case "failed" string))"#;

/// A component exporting `lifecycle.init`; `body` runs first. With
/// `calls_host` it can call `host.call` and traps when that answers an error.
fn guest(calls_host: bool, body: &str) -> Vec<u8> {
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
    let text = format!(
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
                (data (i32.const 48) "{{}}")
                (func (export "run") (param i32 i32 i32 i32) (result i32)
                    (i32.store8 (i32.const 144) (i32.const 0))
                    (i32.store (i32.const 148) (i32.const 48))
                    (i32.store (i32.const 152) (i32.const 2))
                    (i32.const 144))
                (func (export "on-event") (param i32 i32 i32 i32))
                (func (export "on-settings-changed") (param i32 i32))
                (func (export "action") (param i32 i32 i32 i32 i32 i32 i32 i32) (result i32)
                    (i32.const 144))
                (func (export "before-launch") (param i32) (result i32)
                    (i32.const 160))
                (func (export "before-server-start")
                    (param i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32) (result i32)
                    (i32.const 160))
                (func (export "before-content-install")
                    (param i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32 i32)
                    (result i32)
                    (i32.const 160))
                (func (export "info") (param i32 i32) (result i32)
                    (i32.const 144))
                (func (export "three") (param i32 i32 i32 i32 i32 i32) (result i32)
                    (i32.const 144))
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
            (export "hestia:plugin/lifecycle@1.0.0" (instance $lifecycle))
            (func $run (param "command" string) (param "args" string)
                (result (result string (error $err)))
                (canon lift (core func $main "run")
                    (memory (core memory $libc "memory"))
                    (realloc (core func $libc "realloc"))))
            (instance $commands (export "run" (func $run)))
            (export "hestia:plugin/commands@1.0.0" (instance $commands))
            (func $on_event (param "topic" string) (param "payload" string)
                (canon lift (core func $main "on-event")
                    (memory (core memory $libc "memory"))
                    (realloc (core func $libc "realloc"))))
            (instance $events (export "on-event" (func $on_event)))
            (export "hestia:plugin/events@1.0.0" (instance $events))
            (func $on_settings_changed (param "key" string)
                (canon lift (core func $main "on-settings-changed")
                    (memory (core memory $libc "memory"))
                    (realloc (core func $libc "realloc"))))
            (instance $settings_changed (export "on-settings-changed" (func $on_settings_changed)))
            (export "hestia:plugin/settings-changed@1.0.0" (instance $settings_changed))
            (func $render (param "page" string) (param "context" string)
                (result (result string (error $err)))
                (canon lift (core func $main "run")
                    (memory (core memory $libc "memory"))
                    (realloc (core func $libc "realloc"))))
            (func $action (param "page" string) (param "action" string)
                (param "values" string) (param "context" string)
                (result (result string (error $err)))
                (canon lift (core func $main "action")
                    (memory (core memory $libc "memory"))
                    (realloc (core func $libc "realloc"))))
            (instance $ui (export "render" (func $render)) (export "action" (func $action)))
            (export "hestia:plugin/ui@1.0.0" (instance $ui))
            (type $launch_context (record
                (field "instance-id" string) (field "instance-name" string)
                (field "game-version" string) (field "flavor" string)
                (field "loader-version" (option string)) (field "java-path" string)
                (field "memory" (option string)) (field "quick-play" (option string))
                (field "profile-name" string) (field "profile-uuid" string)))
            (type $launch_patch (record
                (field "extra-jvm-args" (list string))
                (field "extra-game-args" (list string))))
            (func $before_launch (param "ctx" $launch_context)
                (result (result $launch_patch (error string)))
                (canon lift (core func $main "before-launch")
                    (memory (core memory $libc "memory"))
                    (realloc (core func $libc "realloc"))))
            (type $server_context (record
                (field "id" string) (field "name" string) (field "version" string)
                (field "flavor" string) (field "port" u16) (field "memory" (option string))))
            (func $before_server_start (param "ctx" $server_context)
                (result (result $launch_patch (error string)))
                (canon lift (core func $main "before-server-start")
                    (memory (core memory $libc "memory"))
                    (realloc (core func $libc "realloc"))))
            (instance $launch_hooks
                (export "launch-context" (type $launch_context))
                (export "launch-patch" (type $launch_patch))
                (export "server-context" (type $server_context))
                (export "before-launch" (func $before_launch))
                (export "before-server-start" (func $before_server_start)))
            (export "hestia:plugin/launch-hooks@1.0.0" (instance $launch_hooks))
            (type $install_context (record
                (field "entry-id" string) (field "entry-type" string)
                (field "source" string) (field "project-id" string)
                (field "version-id" string) (field "file-name" string)
                (field "sha1" string) (field "kind" string)))
            (func $before_content_install (param "ctx" $install_context)
                (result (result (error string)))
                (canon lift (core func $main "before-content-install")
                    (memory (core memory $libc "memory"))
                    (realloc (core func $libc "realloc"))))
            (instance $content_hooks
                (export "install-context" (type $install_context))
                (export "before-content-install" (func $before_content_install)))
            (export "hestia:plugin/content-hooks@1.0.0" (instance $content_hooks))
            (func $info (param "source" string)
                (result (result string (error $err)))
                (canon lift (core func $main "info")
                    (memory (core memory $libc "memory"))
                    (realloc (core func $libc "realloc"))))
            (func $parse_url (param "source" string) (param "args" string)
                (result (result string (error $err)))
                (canon lift (core func $main "run")
                    (memory (core memory $libc "memory"))
                    (realloc (core func $libc "realloc"))))
            (func $recognise_file_url (param "source" string) (param "args" string)
                (result (result string (error $err)))
                (canon lift (core func $main "run")
                    (memory (core memory $libc "memory"))
                    (realloc (core func $libc "realloc"))))
            (func $search (param "source" string) (param "args" string)
                (result (result string (error $err)))
                (canon lift (core func $main "run")
                    (memory (core memory $libc "memory"))
                    (realloc (core func $libc "realloc"))))
            (func $project (param "source" string) (param "args" string)
                (result (result string (error $err)))
                (canon lift (core func $main "run")
                    (memory (core memory $libc "memory"))
                    (realloc (core func $libc "realloc"))))
            (func $projects (param "source" string) (param "args" string)
                (result (result string (error $err)))
                (canon lift (core func $main "run")
                    (memory (core memory $libc "memory"))
                    (realloc (core func $libc "realloc"))))
            (func $versions (param "source" string) (param "args" string)
                (result (result string (error $err)))
                (canon lift (core func $main "run")
                    (memory (core memory $libc "memory"))
                    (realloc (core func $libc "realloc"))))
            (func $versions_by_id (param "source" string) (param "args" string)
                (result (result string (error $err)))
                (canon lift (core func $main "run")
                    (memory (core memory $libc "memory"))
                    (realloc (core func $libc "realloc"))))
            (func $identify (param "source" string) (param "args" string)
                (result (result string (error $err)))
                (canon lift (core func $main "run")
                    (memory (core memory $libc "memory"))
                    (realloc (core func $libc "realloc"))))
            (instance $content_source (export "info" (func $info)) (export "parse-url" (func $parse_url)) (export "recognise-file-url" (func $recognise_file_url)) (export "search" (func $search)) (export "project" (func $project)) (export "projects" (func $projects)) (export "versions" (func $versions)) (export "versions-by-id" (func $versions_by_id)) (export "identify" (func $identify)))
            (export "hestia:plugin/content-source@1.0.0" (instance $content_source))
            (func $flavor_info (param "flavor" string)
                (result (result string (error $err)))
                (canon lift (core func $main "info")
                    (memory (core memory $libc "memory"))
                    (realloc (core func $libc "realloc"))))
            (func $flavor_versions (param "flavor" string)
                (result (result string (error $err)))
                (canon lift (core func $main "info")
                    (memory (core memory $libc "memory"))
                    (realloc (core func $libc "realloc"))))
            (func $loader_versions (param "flavor" string) (param "args" string)
                (result (result string (error $err)))
                (canon lift (core func $main "run")
                    (memory (core memory $libc "memory"))
                    (realloc (core func $libc "realloc"))))
            (func $resolve_server (param "flavor" string) (param "args" string)
                (result (result string (error $err)))
                (canon lift (core func $main "run")
                    (memory (core memory $libc "memory"))
                    (realloc (core func $libc "realloc"))))
            (func $resolve_client (param "flavor" string) (param "args" string)
                (result (result string (error $err)))
                (canon lift (core func $main "run")
                    (memory (core memory $libc "memory"))
                    (realloc (core func $libc "realloc"))))
            (instance $flavor (export "info" (func $flavor_info)) (export "versions" (func $flavor_versions)) (export "loader-versions" (func $loader_versions)) (export "resolve-server" (func $resolve_server)) (export "resolve-client" (func $resolve_client)))
            (export "hestia:plugin/flavor@1.0.0" (instance $flavor))
            (func $format_info (param "format" string)
                (result (result string (error $err)))
                (canon lift (core func $main "info")
                    (memory (core memory $libc "memory"))
                    (realloc (core func $libc "realloc"))))
            (func $format_read (param "format" string) (param "manifest" (list u8)) (param "files" (list string))
                (result (result string (error $err)))
                (canon lift (core func $main "three")
                    (memory (core memory $libc "memory"))
                    (realloc (core func $libc "realloc"))))
            (func $format_land (param "format" string) (param "blueprint" string) (param "files" (list string))
                (result (result string (error $err)))
                (canon lift (core func $main "three")
                    (memory (core memory $libc "memory"))
                    (realloc (core func $libc "realloc"))))
            (func $format_write (param "format" string) (param "blueprint" string) (param "files" (list string))
                (result (result string (error $err)))
                (canon lift (core func $main "three")
                    (memory (core memory $libc "memory"))
                    (realloc (core func $libc "realloc"))))
            (instance $archive_format (export "info" (func $format_info)) (export "read" (func $format_read)) (export "land" (func $format_land)) (export "write" (func $format_write)))
            (export "hestia:plugin/archive-format@1.0.0" (instance $archive_format)))"#
    );
    wat::parse_str(text).expect("guest WAT")
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

fn record(wasm: &[u8], permissions: &[Permission]) -> PluginRecord {
    PluginRecord {
        id: "probe".into(),
        wasm_sha256: Some(hex(&Sha256::digest(wasm))),
        grant: Grant {
            permissions: permissions.iter().copied().collect::<BTreeSet<_>>(),
            ..Grant::default()
        },
        ..PluginRecord::default()
    }
}

fn no_settings(dir: &tempfile::TempDir) -> crate::plugins::PluginSettings {
    crate::plugins::PluginSettings::new(dir.path().join("settings.json"), Vec::new())
}

fn load(
    host: &Arc<TestHost>,
    wasm: &[u8],
    permissions: &[Permission],
) -> (Arc<LoadedPlugin>, tempfile::TempDir) {
    let data = tempfile::tempdir().unwrap();
    let runtime = PluginRuntime::new(
        host.clone(),
        data.path().join("cache"),
        supervisor(&data),
        Arc::new(crate::presence::Presence::over_discord()),
    )
    .unwrap();
    let plugin = runtime
        .load(
            &record(wasm, permissions),
            wasm,
            data.path().into(),
            no_settings(&data),
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
    let runtime = PluginRuntime::new(
        host,
        data.path().join("cache"),
        supervisor(&data),
        Arc::new(crate::presence::Presence::over_discord()),
    )
    .unwrap();
    let recorded = guest(false, "");
    let tampered = guest(false, "unreachable");

    let outcome = runtime.load(
        &record(&recorded, &[]),
        &tampered,
        data.path().into(),
        no_settings(&data),
    );

    assert!(matches!(outcome, Err(LoadError::HashMismatch { .. })));
}

#[tokio::test]
async fn a_page_answer_outside_the_ui_vocabulary_is_a_typed_error_naming_the_page() {
    let host = Arc::new(TestHost::default());
    let (plugin, _data) = load(&host, &guest(false, ""), &[]);
    let chain = CallChain::default();

    let rendered = plugin
        .render_page(&chain, "overview", &PageContext::default())
        .await;
    assert!(
        matches!(&rendered, Err(CallError::InvalidUi(page)) if page == "overview"),
        "{rendered:?}"
    );
    assert!(!rendered.unwrap_err().to_string().contains("expected"));

    let acted = plugin
        .page_action(
            &chain,
            "overview",
            &Action::default(),
            &json!({}),
            &PageContext::default(),
        )
        .await;
    assert!(matches!(acted, Err(CallError::InvalidUi(_))), "{acted:?}");
}
