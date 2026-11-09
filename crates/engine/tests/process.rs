//! Stopping a supervised process stops what it spawned; a stdin endpoint
//! outlives the daemon that created it.

#![cfg(unix)]

use std::collections::BTreeMap;
use std::time::Duration;

use engine::ProcessSupervisor;
use proto::process::{LogSource, ProcessSpec, ProcessState, StopMethod};

/// A directory that removes itself when the test ends, named after the test so
/// a leftover under a debugger says which one left it.
fn temp_dir(tag: &str) -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(&format!("hestia-process-{tag}-"))
        .tempdir()
        .expect("temp dir")
}

fn alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

async fn wait_for(mut ready: impl FnMut() -> bool) -> bool {
    for _ in 0..100 {
        if ready() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    false
}

#[tokio::test]
async fn stopping_a_process_stops_its_children() {
    let dir = temp_dir("tree");
    let supervisor = ProcessSupervisor::new(dir.path().join("processes"));
    let child_pid_file = dir.path().join("child.pid");

    supervisor
        .start(ProcessSpec {
            id: "tree".into(),
            program: "/bin/sh".into(),
            args: vec![
                "-c".into(),
                format!(
                    "sleep 300 & echo $! > {} ; wait",
                    child_pid_file.to_string_lossy()
                ),
            ],
            cwd: Some(dir.path().to_path_buf()),
            env: BTreeMap::new(),
            log: LogSource::Capture,
            ..Default::default()
        })
        .await
        .expect("the process starts");

    assert!(
        wait_for(|| child_pid_file.is_file()).await,
        "the child records its pid"
    );
    let child: u32 = std::fs::read_to_string(&child_pid_file)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert!(alive(child), "the child is running before the stop");

    assert!(supervisor.stop("tree"), "the process is known");
    assert!(
        wait_for(|| supervisor
            .status("tree")
            .is_some_and(|p| p.state != ProcessState::Running))
        .await,
        "the process reaches a terminal state"
    );
    assert!(
        wait_for(|| !alive(child)).await,
        "the child goes with its parent — a stop reaches the whole tree"
    );
}

fn shell(id: &str, script: &str) -> ProcessSpec {
    ProcessSpec {
        id: id.into(),
        program: "/bin/sh".into(),
        args: vec!["-c".into(), script.into()],
        stdin: true,
        ..Default::default()
    }
}

fn logged(supervisor: &ProcessSupervisor, id: &str, needle: &str) -> bool {
    supervisor
        .logs(id, None)
        .is_some_and(|lines| lines.iter().any(|l| l.line.contains(needle)))
}

#[tokio::test]
async fn a_restarted_supervisor_reopens_the_stdin_endpoint() {
    let dir = temp_dir("stdin");
    let processes = dir.path().join("processes");
    let first = ProcessSupervisor::new(processes.clone());
    first.start(shell("cat", "cat")).await.expect("starts");

    first.write_line("cat", "before").await.expect("write");
    assert!(wait_for(|| logged(&first, "cat", "before")).await);

    let second = ProcessSupervisor::new(processes);
    second.recover();
    second
        .write_line("cat", "after")
        .await
        .expect("write after adoption");
    assert!(wait_for(|| logged(&second, "cat", "after")).await);

    second.stop("cat");
    assert!(wait_for(|| !second.any_running()).await);
    assert!(
        !processes_dir_has_stdin(dir.path(), "cat"),
        "the endpoint goes with the process"
    );
}

fn processes_dir_has_stdin(root: &std::path::Path, id: &str) -> bool {
    root.join("processes").join(id).join("stdin").exists()
}

#[tokio::test]
async fn writing_needs_a_running_process_with_an_endpoint() {
    let dir = temp_dir("stdin-errors");
    let supervisor = ProcessSupervisor::new(dir.path().join("processes"));
    let plain = ProcessSpec {
        stdin: false,
        ..shell("plain", "sleep 30")
    };
    supervisor.start(plain).await.expect("starts");
    assert!(matches!(
        supervisor.write_line("plain", "list").await,
        Err(engine::StdinError::NoEndpoint(_))
    ));
    assert!(matches!(
        supervisor.write_line("missing", "list").await,
        Err(engine::StdinError::NotRunning(_))
    ));
    supervisor.stop("plain");
    assert!(wait_for(|| !supervisor.any_running()).await);
}

#[tokio::test]
async fn a_stop_line_ends_the_process_without_a_signal() {
    let dir = temp_dir("stdin-stop");
    let supervisor = ProcessSupervisor::new(dir.path().join("processes"));
    let spec = ProcessSpec {
        stop: StopMethod::Line("quit".into()),
        ..shell("graceful", "read l; echo got:$l; exit 7")
    };
    supervisor.start(spec).await.expect("starts");

    supervisor.stop("graceful");
    assert!(wait_for(|| !supervisor.any_running()).await);
    let info = supervisor.status("graceful").expect("known");
    assert_eq!(
        info.exit_code,
        Some(7),
        "it exited by itself, not by signal"
    );
    assert!(logged(&supervisor, "graceful", "got:quit"));
}
