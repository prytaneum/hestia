//! The SDK driven end to end against a scripted daemon over an in-memory
//! duplex: what a call, a failure and a job settle to, without a socket.

mod fake;

use std::time::Duration;

use client::proto::error::{EntryKind, ErrorInfo};
use client::proto::java::{JavaInstallPhase, JavaInstallProgress, JavaRuntime};
use client::proto::Contract;
use client::{error_info, IpcError};
use fake::{event, Reply, Script};
use serde_json::json;

/// A job waits for its terminal event with no deadline of its own — that is
/// the point of it. So a test that scripts one wrong would hang the suite
/// forever; this turns that into a failure that says so.
async fn settles<T>(what: impl std::future::Future<Output = T>) -> T {
    match tokio::time::timeout(Duration::from_secs(5), what).await {
        Ok(outcome) => outcome,
        Err(_) => panic!("the job never settled: no scripted event matched its id or topic"),
    }
}

fn runtime() -> JavaRuntime {
    JavaRuntime {
        vendor: "temurin".into(),
        major: 21,
        release_name: "jdk-21".into(),
        home: "/java/21".into(),
        executable: "/java/21/bin/java".into(),
        in_use: false,
    }
}

#[tokio::test]
async fn a_daemon_error_arrives_as_a_typed_failure() {
    let client = Script::new()
        .on(
            "java.uninstall",
            Reply::Fail(ErrorInfo::EntryNotFound {
                entry: EntryKind::Server,
                reference: "smp".into(),
            }),
        )
        .serve();

    let error = client.java().uninstall(21).await.expect_err("not found");
    assert!(
        matches!(&error, IpcError::Daemon { code, .. } if code == proto::error::code::NOT_FOUND)
    );
    assert!(matches!(
        error_info(&error),
        ErrorInfo::EntryNotFound { .. }
    ));
}

#[tokio::test]
async fn try_call_turns_not_found_into_none() {
    let client = Script::new()
        .on(
            "config.get",
            Reply::Fail(ErrorInfo::ConfigKeyUnknown { key: "nope".into() }),
        )
        .on("config.get", Reply::Ok(json!({ "value": "on" })))
        .serve();

    assert_eq!(client.config().get("nope").await.expect("get"), None);
    assert_eq!(
        client.config().get("home").await.expect("get"),
        Some(json!("on"))
    );
}

#[tokio::test]
async fn a_silent_daemon_times_out_rather_than_hanging() {
    let client = Script::new().on("java.list", Reply::Silent).serve();

    struct Impatient;
    impl Contract for Impatient {
        const CHANNEL: &'static str = "java.list";
        const TIMEOUT: Duration = Duration::from_millis(50);
        type Params = client::proto::Empty;
        type Result = client::proto::Empty;
    }

    let error = client
        .session()
        .call::<Impatient>(&client::proto::Empty {})
        .await
        .expect_err("timeout");
    assert!(matches!(error, IpcError::Timeout(channel) if channel == "java.list"));
}

#[tokio::test]
async fn a_foreign_protocol_version_names_itself() {
    let client = Script::new()
        .on(
            "java.list",
            Reply::Frame(json!({ "v": 999, "ok": true, "payload": {} }).to_string()),
        )
        .serve();

    let error = client.java().list().await.expect_err("mismatch");
    assert!(matches!(
        error,
        IpcError::IncompatibleVersion { got: 999, .. }
    ));
}

#[tokio::test]
async fn a_job_settles_on_its_done_event() {
    let client = Script::new()
        .on(
            "java.install",
            Reply::OkThen(
                json!({ "id": "j1" }),
                vec![
                    event(
                        "job",
                        json!({
                            "id": "j1", "channel": "java.install", "state": "progress",
                            "phase": "downloading", "current": 5, "total": 10,
                        }),
                    ),
                    event(
                        "job",
                        json!({
                            "id": "j1", "channel": "java.install", "state": "done",
                            "runtime": runtime(), "alreadyInstalled": true,
                        }),
                    ),
                ],
            ),
        )
        .serve();

    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let observed = seen.clone();
    let (installed, already) = settles(client.java().install(
        21,
        false,
        "j1",
        move |p: &JavaInstallProgress| {
            observed.lock().unwrap().push(p.phase);
        },
    ))
    .await
    .expect("install");

    assert_eq!(installed.major, 21);
    // The done event is camelCase on the wire, like every other payload.
    assert!(already, "alreadyInstalled must survive the wire");
    assert_eq!(*seen.lock().unwrap(), vec![JavaInstallPhase::Downloading]);
}

#[tokio::test]
async fn a_job_settles_on_its_error_event() {
    let client = Script::new()
        .on(
            "java.install",
            Reply::OkThen(
                json!({ "id": "j2" }),
                vec![event(
                    "job",
                    json!({
                        "id": "j2",
                        "channel": "java.install",
                        "state": "failed",
                        "error": ErrorInfo::InvalidValue {
                            field: client::proto::error::Field::JavaVersion,
                            reason: client::proto::error::Reason::JavaMajor,
                        },
                    }),
                )],
            ),
        )
        .serve();

    let error = settles(client.java().install(21, false, "j2", |_| {}))
        .await
        .expect_err("failed");
    assert!(matches!(error_info(&error), ErrorInfo::InvalidValue { .. }));
}

#[tokio::test]
async fn a_cancelled_job_is_not_reported_as_a_failure() {
    let client = Script::new()
        .on(
            "java.install",
            Reply::OkThen(
                json!({ "id": "j3" }),
                vec![event(
                    "job",
                    json!({ "id": "j3", "channel": "java.install", "state": "cancelled" }),
                )],
            ),
        )
        .serve();

    let error = settles(client.java().install(21, false, "j3", |_| {}))
        .await
        .expect_err("cancelled");
    assert!(
        matches!(error, IpcError::Cancelled),
        "a cancelled job is its own outcome, not an error"
    );
}

#[tokio::test]
async fn a_job_ignores_events_belonging_to_another_job() {
    let client = Script::new()
        .on(
            "java.install",
            Reply::OkThen(
                json!({ "id": "mine" }),
                vec![
                    event(
                        "job",
                        json!({
                            "id": "theirs", "channel": "java.install", "state": "done",
                            "runtime": runtime(),
                        }),
                    ),
                    event(
                        "job",
                        json!({
                            "id": "mine", "channel": "java.install", "state": "done",
                            "runtime": runtime(),
                        }),
                    ),
                ],
            ),
        )
        .serve();

    let (_, already) = settles(client.java().install(21, false, "mine", |_| {}))
        .await
        .expect("install");
    assert!(!already);
}

#[tokio::test]
async fn a_job_subscribes_before_it_starts() {
    let script = Script::new().on(
        "java.install",
        Reply::OkThen(
            json!({ "id": "j4" }),
            vec![event(
                "job",
                json!({
                    "id": "j4", "channel": "java.install", "state": "done",
                    "runtime": runtime(),
                }),
            )],
        ),
    );
    let seen = script.seen();
    let client = script.serve();

    settles(client.java().install(21, false, "j4", |_| {}))
        .await
        .expect("install");

    assert_eq!(
        *seen.lock().unwrap(),
        vec!["events.subscribe".to_string(), "java.install".to_string()],
        "a job that finishes instantly must not slip its terminal event past us"
    );
}

#[tokio::test]
async fn a_dropped_daemon_wakes_its_waiters() {
    let (daemon, ours) = tokio::io::duplex(1024);
    let client = client::Client::over(ipc::Connection::from_io(ours));
    drop(daemon);
    tokio::task::yield_now().await;

    let error = client.java().list().await.expect_err("connection lost");
    assert!(
        matches!(error, IpcError::ConnectionLost),
        "a torn-down connection fails its waiters rather than timing them out, got {error:?}"
    );
}
