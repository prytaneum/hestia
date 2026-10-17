//! End-to-end test: spawn the real `hestiad` binary on an isolated socket and
//! data directory, then drive it through the client SDK exactly as a front-end
//! would. This exercises the full stack — transport, router, services, the
//! process supervisor — against a running daemon.
//!
//! Unix-only: it relies on a POSIX shell for the launched child and on the
//! domain-socket transport. Windows is validated through the win-VM flow.
#![cfg(unix)]

use std::process::{Child, Command};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use client::proto::content::{ContentAddItem, ContentAddSpec, ContentKind};
use client::proto::error::{EntryKind, EntryTarget, ErrorInfo};
use client::proto::process::{ProcessSpec, ProcessState};
use client::{Client, ProcessEvent};

/// A spawned daemon that is stopped and reaped on drop.
struct Daemon {
    child: Child,
    home: std::path::PathBuf,
    /// Held when this daemon owns its data home, so the directory outlives the
    /// process and is removed with it. A test driving two daemons over one home
    /// keeps the guard itself instead.
    _home: Option<tempfile::TempDir>,
}

impl Daemon {
    fn wait_exit(&mut self) {
        for _ in 0..100 {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("daemon did not exit within 5s");
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn temp_home() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("hestia-e2e-")
        .tempdir()
        .expect("temp dir")
}

async fn spawn_daemon() -> (Daemon, Client) {
    let home = temp_home();
    spawn_daemon_at(home.path().to_path_buf(), Some(home)).await
}

async fn spawn_daemon_at(
    home: std::path::PathBuf,
    guard: Option<tempfile::TempDir>,
) -> (Daemon, Client) {
    let sock = home.join("hestiad.sock");
    let child = Command::new(env!("CARGO_BIN_EXE_hestiad"))
        .arg("serve")
        .env("HESTIA_SOCK", &sock)
        .env("HESTIA_HOME", &home)
        .env("HESTIA_NO_TRAY", "1")
        .env("HESTIA_NO_PRESENCE", "1")
        .spawn()
        .expect("spawn hestiad");

    // Wait for the daemon to bind and accept connections.
    let mut client = None;
    for _ in 0..100 {
        if let Ok(c) = Client::connect_to(&sock).await {
            if c.app().ping().await.is_ok() {
                client = Some(c);
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let client = client.expect("daemon did not become reachable within 5s");
    (
        Daemon {
            child,
            home,
            _home: guard,
        },
        client,
    )
}

fn pid_alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

#[tokio::test]
async fn daemon_serves_the_full_client_surface() {
    let (daemon, client) = spawn_daemon().await;

    // Identity.
    let info = client.app().info().await.expect("app.info");
    assert_eq!(info.name, "Hestia");

    // The reserved `home` key round-trips the isolated data directory.
    let home = client.config().get("home").await.expect("config.get home");
    let home = home
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default();
    assert_eq!(
        std::path::Path::new(&home),
        daemon.home.as_path(),
        "the daemon should report the HESTIA_HOME data directory"
    );

    // An empty engine has no runtimes and an addressable cache.
    assert!(client.java().list().await.expect("java.list").is_empty());
    let cache = client.cache().info().await.expect("cache.info");
    assert!(cache.path.starts_with(&daemon.home));

    // Run a process to completion and capture its output and exit code.
    let captured = Arc::new(Mutex::new(Vec::<String>::new()));
    let sink = captured.clone();
    let exit = client
        .process()
        .run(
            ProcessSpec {
                program: "/bin/sh".into(),
                args: vec!["-c".into(), "echo hello; exit 3".into()],
                ..Default::default()
            },
            move |line| sink.lock().unwrap().push(line.line.clone()),
        )
        .await
        .expect("process.run");
    assert_eq!(exit.exit_code, Some(3));
    assert!(!exit.success);
    assert!(
        captured.lock().unwrap().iter().any(|l| l == "hello"),
        "captured output should contain the child's stdout line"
    );

    // Start a long-running process, observe it, then stop it.
    let started = client
        .process()
        .start(ProcessSpec {
            program: "/bin/sh".into(),
            args: vec![
                "-c".into(),
                "while true; do echo tick; sleep 1; done".into(),
            ],
            ..Default::default()
        })
        .await
        .expect("process.start");
    assert!(started.pid > 0);

    tokio::time::sleep(Duration::from_millis(1200)).await;
    let status = client
        .process()
        .status(&started.id)
        .await
        .expect("process.status");
    assert_eq!(status.state, ProcessState::Running);
    let logs = client
        .process()
        .logs(&started.id, None)
        .await
        .expect("process.logs");
    assert!(
        logs.iter().any(|l| l.line == "tick"),
        "buffered logs should hold output"
    );

    client
        .process()
        .stop(&started.id)
        .await
        .expect("process.stop");
    let mut killed = false;
    for _ in 0..40 {
        let s = client
            .process()
            .status(&started.id)
            .await
            .expect("process.status");
        if s.state == ProcessState::Killed {
            killed = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(killed, "the process should report Killed after stop");

    let missing = client
        .files()
        .list("ghost", "")
        .await
        .expect_err("a file listing needs a server");
    assert!(matches!(
        client::error_info(&missing),
        client::proto::error::ErrorInfo::EntryNotFound { .. }
    ));

    client.daemon().stop(false).await.expect("daemon.stop");
    drop(daemon);
}

#[tokio::test]
async fn supervised_processes_survive_a_daemon_restart() {
    // The home outlives both daemons, so the test holds the guard rather than
    // either of them.
    let home = temp_home();
    let path = home.path().to_path_buf();
    let (mut daemon, client) = spawn_daemon_at(path.clone(), None).await;

    let started = client
        .process()
        .start(ProcessSpec {
            id: "e2e-survivor".into(),
            program: "/bin/sh".into(),
            args: vec!["-c".into(), "sleep 60".into()],
            ..Default::default()
        })
        .await
        .expect("process.start");
    let pid = started.pid;
    assert!(pid_alive(pid));

    client.daemon().stop(false).await.expect("daemon.stop");
    drop(client);
    daemon.wait_exit();
    assert!(
        pid_alive(pid),
        "the process should outlive the daemon that spawned it"
    );

    let (daemon2, client) = spawn_daemon_at(path, None).await;
    let status = client
        .process()
        .status("e2e-survivor")
        .await
        .expect("process.status after restart");
    assert_eq!(status.state, ProcessState::Running);
    assert_eq!(status.pid, pid, "the adopted process keeps its pid");

    client
        .process()
        .stop("e2e-survivor")
        .await
        .expect("process.stop");
    let mut stopped = false;
    for _ in 0..100 {
        let s = client.process().status("e2e-survivor").await.unwrap();
        if s.state == ProcessState::Killed {
            stopped = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(stopped, "an adopted process should still be stoppable");
    assert!(!pid_alive(pid));

    drop(daemon2);
    drop(daemon);
}

/// Following an entry outlives the processes it runs: one subscription taken
/// against the entry key sees the current run's output, its exit, and the next
/// run's start and output — the CLI's `logs -f` and the desktop's log panel both
/// stay on the stream across a stop/start instead of ending with the process.
#[tokio::test]
async fn following_an_entry_survives_a_stop_and_start() {
    let (daemon, client) = spawn_daemon().await;
    let entry = client::proto::naming::server_process_id("e2e");

    let spec = |marker: &str| ProcessSpec {
        id: entry.clone(),
        program: "/bin/sh".into(),
        args: vec![
            "-c".into(),
            format!("while true; do echo {marker}; sleep 1; done"),
        ],
        ..Default::default()
    };

    client
        .process()
        .start(spec("first"))
        .await
        .expect("process.start");

    // Subscribed before anything happens, and never re-subscribed below.
    let mut events = client
        .process()
        .subscribe(&entry)
        .await
        .expect("process.subscribe");

    wait_for(
        &mut events,
        |e| matches!(e, ProcessEvent::Output(l) if l.line == "first"),
    )
    .await;

    client.process().stop(&entry).await.expect("process.stop");
    wait_for(&mut events, |e| matches!(e, ProcessEvent::Exit(_))).await;

    // Restarting under the same entry key resumes the *same* stream.
    client
        .process()
        .start(spec("second"))
        .await
        .expect("process.start after stop");
    wait_for(&mut events, |e| matches!(e, ProcessEvent::Started(_))).await;
    wait_for(
        &mut events,
        |e| matches!(e, ProcessEvent::Output(l) if l.line == "second"),
    )
    .await;

    client.process().stop(&entry).await.expect("process.stop");
    client.daemon().stop(true).await.expect("daemon.stop");
    drop(daemon);
}

/// An instance runs each launch under its own session key, so following the
/// instance means the subscription must cover the session keys beneath its
/// entry key — including sessions that start after the subscription.
#[tokio::test]
async fn following_an_instance_covers_its_later_sessions() {
    let (daemon, client) = spawn_daemon().await;
    let entry = client::proto::naming::instance_process_id("e2e");

    let mut events = client
        .process()
        .subscribe(&entry)
        .await
        .expect("process.subscribe");

    client
        .process()
        .start(ProcessSpec {
            id: client::proto::naming::instance_session_id("e2e", 1),
            program: "/bin/sh".into(),
            args: vec![
                "-c".into(),
                "while true; do echo session; sleep 1; done".into(),
            ],
            ..Default::default()
        })
        .await
        .expect("process.start");

    wait_for(&mut events, |e| matches!(e, ProcessEvent::Started(_))).await;
    wait_for(
        &mut events,
        |e| matches!(e, ProcessEvent::Output(l) if l.line == "session"),
    )
    .await;

    client.daemon().stop(true).await.expect("daemon.stop");
    drop(daemon);
}

/// Pull events until one satisfies `wanted`, failing the test on a timeout —
/// a stream that ended is exactly the defect under test.
async fn wait_for(
    events: &mut tokio::sync::mpsc::UnboundedReceiver<ProcessEvent>,
    wanted: impl Fn(&ProcessEvent) -> bool,
) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while let Ok(Some(event)) = tokio::time::timeout_at(deadline, events.recv()).await {
        if wanted(&event) {
            return;
        }
    }
    panic!("the event stream ended or timed out before the expected event");
}

#[tokio::test]
async fn daemon_stop_all_takes_workloads_down() {
    let (mut daemon, client) = spawn_daemon().await;

    let started = client
        .process()
        .start(ProcessSpec {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), "sleep 60".into()],
            ..Default::default()
        })
        .await
        .expect("process.start");

    client.daemon().stop(true).await.expect("daemon.stop");
    drop(client);
    daemon.wait_exit();
    for _ in 0..50 {
        if !pid_alive(started.pid) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("stop --all should take the workload down with the daemon");
}

/// A client that did not make the call hears of it on `changed`, and a read says
/// nothing.
#[tokio::test]
async fn a_successful_mutation_publishes_what_it_changed() {
    let (daemon, client) = spawn_daemon().await;
    let (seen, mut topics) = tokio::sync::mpsc::unbounded_channel();
    client
        .session()
        .set_event_callback(Some(Arc::new(move |event: &ipc::protocol::Event| {
            let _ = seen.send((event.topic.clone(), event.payload.clone()));
        })));
    client
        .session()
        .call::<client::proto::events::EventsSubscribe>(&Default::default())
        .await
        .expect("events.subscribe");

    client
        .config()
        .get("defaults.jvm-args")
        .await
        .expect("config.get");
    client
        .config()
        .set("defaults.jvm-args", serde_json::json!("-Xmx1G"))
        .await
        .expect("config.set");

    let (topic, payload) = tokio::time::timeout(Duration::from_secs(10), topics.recv())
        .await
        .expect("an event within the timeout")
        .expect("the stream is open");
    assert_eq!(topic, "changed");
    assert_eq!(payload, serde_json::json!({ "scope": "config" }));

    client.daemon().stop(true).await.expect("daemon.stop");
    drop(daemon);
}

fn sodium_spec() -> ContentAddSpec {
    ContentAddSpec {
        kind: ContentKind::Mod,
        items: vec![ContentAddItem {
            project: "sodium".into(),
            ..Default::default()
        }],
        ..Default::default()
    }
}

#[tokio::test]
async fn an_instance_target_is_gated_on_an_account_but_a_server_target_is_not() {
    let (_daemon, client) = spawn_daemon().await;
    let instance = EntryTarget::new(EntryKind::Instance, "nothing-here");
    let server = EntryTarget::new(EntryKind::Server, "nothing-here");

    let refused = client
        .content()
        .add(&instance, sodium_spec(), |_| {})
        .await
        .expect_err("content.add on an instance without an account");
    assert!(
        matches!(client::error_info(&refused), ErrorInfo::SignInRequired),
        "an instance target signed out must say to sign in, got {refused}"
    );

    let refused = client
        .modpack()
        .status(&instance)
        .await
        .expect_err("modpack.status on an instance without an account");
    assert!(matches!(
        client::error_info(&refused),
        ErrorInfo::SignInRequired
    ));

    let missing = client
        .content()
        .add(&server, sodium_spec(), |_| {})
        .await
        .expect_err("content.add on a server that does not exist");
    assert!(
        matches!(
            client::error_info(&missing),
            ErrorInfo::EntryNotFound {
                entry: EntryKind::Server,
                ..
            }
        ),
        "a server target is never refused for lack of an account, got {missing}"
    );

    let missing = client
        .modpack()
        .status(&server)
        .await
        .expect_err("modpack.status on a server that does not exist");
    assert!(matches!(
        client::error_info(&missing),
        ErrorInfo::EntryNotFound { .. }
    ));
}
