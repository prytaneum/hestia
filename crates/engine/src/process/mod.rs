//! Process supervision, decoupled from the daemon's lifetime: a launched
//! process runs in its own process group with its output on disk, is recorded
//! under `<data_home>/processes/<id>/`, and keeps running when the daemon
//! stops. The next daemon re-adopts it from the record (pid + start-time
//! token, so pid reuse cannot be mistaken for the old process). A launched
//! process runs as the same user the daemon runs as, so this is no more
//! privileged than the user spawning it directly.
//!
//! The engine does not know a socket exists, so where lifecycle events go
//! arrives from outside through [`ProcessEvents`].

mod identity;
mod records;
mod secrets;
mod tail;
mod task;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use proto::process::{
    LogSource, LogStream, ProcessExitEvent, ProcessInfo, ProcessLogLine, ProcessSpec,
    ProcessStartedEvent, ProcessState,
};
use tokio::process::{Child, Command};
use tokio::sync::Notify;

use records::ProcessRecord;

/// Lines `process.logs` returns when no explicit tail is given.
const DEFAULT_LOG_LINES: usize = 2000;

/// How many finished processes keep their directory (logs included) for
/// post-mortem. Count-based like `backup prune`, and for the same reason:
/// retention has to be stated, or "keep the logs" means "grow forever".
const FINISHED_KEEP: usize = 20;
/// How long a stop waits between the polite signal and the hard kill.
const STOP_GRACE: Duration = Duration::from_secs(10);
const STOP_WAIT: Duration = Duration::from_secs(120);
const STOP_POLL: Duration = Duration::from_millis(250);
/// How often an adopted (non-child) process is checked for exit.
const ADOPTED_POLL: Duration = Duration::from_secs(1);

/// Where the supervisor's lifecycle events go. One method, since the topic name
/// comes from `proto` and the sink never needs to know the payload shapes.
pub trait ProcessEvents: Send + Sync {
    fn publish(&self, topic: &str, payload: serde_json::Value);
}

pub(crate) fn emit<E: proto::Topic + serde::Serialize>(
    events: Option<&Arc<dyn ProcessEvents>>,
    event: &E,
) {
    if let Some(events) = events {
        events.publish(E::TOPIC, serde_json::to_value(event).unwrap_or_default());
    }
}

fn generate_id() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed) + 1;
    format!("process-{}-{}", std::process::id(), n)
}

// The id names a directory under the supervisor's state dir.
fn is_safe_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && !id.starts_with('.')
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

/// The shared, live state of one tracked process. Cloned as an `Arc` into the
/// supervision task and read back through the table by the query handlers.
struct Entry {
    info: Mutex<ProcessInfo>,
    log_path: PathBuf,
    err_path: Option<PathBuf>,
    stopping: AtomicBool,
    stop_notify: Notify,
}

impl Entry {
    fn snapshot(&self) -> ProcessInfo {
        self.info.lock().unwrap().clone()
    }

    fn is_running(&self) -> bool {
        self.info.lock().unwrap().state == ProcessState::Running
    }
}

enum Watched {
    Owned {
        child: Box<Child>,
        tree: Option<identity::Tree>,
    },
    /// Recovered from a record: not our child, so the exit code is
    /// unobservable and exit is detected by polling the identity.
    Adopted { pid: u32, token: u64 },
}

pub use task::{silent, Outcome, Task};

/// Notified once when a supervised process terminally exits, with its final info.
pub type ExitObserver = Arc<dyn Fn(&ProcessInfo) + Send + Sync>;

pub struct ProcessSupervisor {
    events: OnceLock<Arc<dyn ProcessEvents>>,
    on_exit: OnceLock<ExitObserver>,
    dir: PathBuf,
    table: Mutex<HashMap<String, Arc<Entry>>>,
}

impl ProcessSupervisor {
    pub fn new(dir: PathBuf) -> Self {
        ProcessSupervisor {
            events: OnceLock::new(),
            on_exit: OnceLock::new(),
            dir,
            table: Mutex::new(HashMap::new()),
        }
    }

    /// Called once at boot. Before it, a process still runs and is still
    /// recorded; it simply publishes nowhere.
    pub fn attach(&self, events: Arc<dyn ProcessEvents>, on_exit: Option<ExitObserver>) {
        let _ = self.events.set(events);
        if let Some(on_exit) = on_exit {
            let _ = self.on_exit.set(on_exit);
        }
    }

    fn dir(&self) -> &Path {
        &self.dir
    }

    fn events(&self) -> Option<&Arc<dyn ProcessEvents>> {
        self.events.get()
    }

    pub async fn start(&self, mut spec: ProcessSpec) -> Result<ProcessInfo, StartError> {
        if spec.program.trim().is_empty() {
            return Err(StartError::EmptyProgram);
        }
        if spec.id.is_empty() {
            spec.id = generate_id();
        }
        if !is_safe_id(&spec.id) {
            return Err(StartError::InvalidId(spec.id.clone()));
        }

        let proc_dir = self.dir().join(&spec.id);
        let failed = |source| StartError::Spawn {
            program: spec.program.clone(),
            source,
        };
        let io = prepare_stdio(&spec, &proc_dir).map_err(failed)?;
        let child = build_command(&spec, io.out, io.err)
            .spawn()
            .map_err(failed)?;
        let pid = child.id().unwrap_or(0);
        let started_unix = common::now_unix();

        let record = ProcessRecord::for_spawn(&spec, pid, started_unix);
        let entry = Arc::new(Entry {
            info: Mutex::new(record.info()),
            log_path: io.log_path,
            err_path: io.err_path,
            stopping: AtomicBool::new(false),
            stop_notify: Notify::new(),
        });
        self.table
            .lock()
            .unwrap()
            .insert(spec.id.clone(), entry.clone());
        records::save(&proc_dir, &record);
        let snapshot = entry.snapshot();

        // spec.args may carry launch credentials (e.g. an access token) — never log them.
        tracing::info!(
            id = %spec.id,
            pid,
            program = %spec.program,
            cwd = spec.cwd.as_ref().map(|p| p.display().to_string()),
            "process started"
        );
        emit(
            self.events(),
            &ProcessStartedEvent {
                id: spec.id.clone(),
                pid,
            },
        );

        tokio::spawn(supervise(
            entry,
            Watched::Owned {
                tree: identity::Tree::hold(&child),
                child: Box::new(child),
            },
            record,
            self.events().cloned(),
            proc_dir,
            io.tail_from,
            self.on_exit.get().cloned(),
        ));
        Ok(snapshot)
    }

    /// Called once at daemon start, before the endpoint accepts requests.
    pub fn recover(&self) {
        let mut known = HashSet::new();
        for record in records::scan(self.dir()) {
            known.insert(record.id.clone());
            if record.ended.is_some() {
                continue;
            }
            let proc_dir = self.dir().join(&record.id);
            if !identity::is_same(record.pid, record.pid_started) {
                tracing::info!(id = %record.id, pid = record.pid, "process exited while unsupervised");
                records::save(
                    &proc_dir,
                    &record
                        .clone()
                        .end(ProcessState::Exited, None, common::now_unix()),
                );
                emit(
                    self.events(),
                    &ProcessExitEvent {
                        id: record.id,
                        state: ProcessState::Exited,
                        exit_code: None,
                        success: false,
                    },
                );
                continue;
            }

            let (log_path, err_path) = log_paths(&record.spec, &proc_dir);
            let tail_from = std::fs::metadata(&log_path).map(|m| m.len()).unwrap_or(0);
            let entry = Arc::new(Entry {
                info: Mutex::new(record.info()),
                log_path,
                err_path,
                stopping: AtomicBool::new(false),
                stop_notify: Notify::new(),
            });
            self.table
                .lock()
                .unwrap()
                .insert(record.id.clone(), entry.clone());
            tracing::info!(id = %record.id, pid = record.pid, "re-adopted process");
            emit(
                self.events(),
                &ProcessStartedEvent {
                    id: record.id.clone(),
                    pid: record.pid,
                },
            );
            tokio::spawn(supervise(
                entry,
                Watched::Adopted {
                    pid: record.pid,
                    token: record.pid_started,
                },
                record,
                self.events().cloned(),
                proc_dir,
                tail_from,
                self.on_exit.get().cloned(),
            ));
        }
        self.sweep(&known);
        self.prune_finished();
    }

    /// Delete only true strays — a directory with no readable record. A finished
    /// process keeps its record and its logs; retention is what eventually
    /// reclaims those, so the post-mortem guarantee and the cleanup do not
    /// conflict.
    fn sweep(&self, known: &HashSet<String>) {
        let Ok(entries) = std::fs::read_dir(self.dir()) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if known.contains(&name) {
                continue;
            }
            tracing::debug!(id = %name, "sweeping a process directory belonging to no process");
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }

    /// Drop the oldest finished processes' directories, logs and all, once more
    /// than [`FINISHED_KEEP`] have accumulated. Stated retention is what makes
    /// keeping them safe: a launcher that never forgets a session would grow a
    /// log directory without bound.
    fn prune_finished(&self) {
        for stale in records::scan_finished(self.dir())
            .iter()
            .skip(FINISHED_KEEP)
        {
            tracing::debug!(id = %stale.id, "pruning a finished process directory");
            let _ = std::fs::remove_dir_all(self.dir().join(&stale.id));
        }
    }

    /// Returns false if no such id is tracked.
    pub fn stop(&self, id: &str) -> bool {
        let entry = self.table.lock().unwrap().get(id).cloned();
        match entry {
            Some(entry) => {
                tracing::info!(id, "stop requested");
                entry.stopping.store(true, Ordering::SeqCst);
                // notify_one stores a permit, so a stop that races ahead of the
                // supervision task's wait is not lost.
                entry.stop_notify.notify_one();
                true
            }
            None => false,
        }
    }

    /// Stop the named processes and wait for each to exit. False when one is
    /// still running after [`STOP_WAIT`] — a caller about to reuse the tree
    /// abandons the work rather than start over a process that may hold it.
    pub async fn stop_and_wait(&self, ids: &[String]) -> bool {
        for id in ids {
            self.stop(id);
        }
        let deadline = tokio::time::Instant::now() + STOP_WAIT;
        while ids.iter().any(|id| {
            self.status(id)
                .is_some_and(|p| p.state == ProcessState::Running)
        }) {
            if tokio::time::Instant::now() >= deadline {
                return false;
            }
            tokio::time::sleep(STOP_POLL).await;
        }
        true
    }

    pub async fn stop_all_and_wait(&self) {
        let running: Vec<Arc<Entry>> = self
            .table
            .lock()
            .unwrap()
            .values()
            .filter(|e| e.is_running())
            .cloned()
            .collect();
        if running.is_empty() {
            return;
        }
        tracing::info!(count = running.len(), "stopping supervised processes");
        for entry in &running {
            entry.stopping.store(true, Ordering::SeqCst);
            entry.stop_notify.notify_one();
        }
        let deadline = tokio::time::Instant::now() + STOP_GRACE + Duration::from_secs(5);
        while tokio::time::Instant::now() < deadline {
            if running.iter().all(|e| !e.is_running()) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        tracing::warn!("timed out waiting for supervised processes to stop");
    }

    /// Refused while the process is running.
    pub fn discard(&self, id: &str) -> bool {
        let mut table = self.table.lock().unwrap();
        if table.get(id).is_some_and(|e| e.is_running()) {
            return false;
        }
        table.remove(id);
        drop(table);
        tracing::debug!(id, "discarding process state");
        let _ = std::fs::remove_dir_all(self.dir().join(id));
        true
    }

    pub fn any_running(&self) -> bool {
        self.table.lock().unwrap().values().any(|e| e.is_running())
    }

    /// Every process the daemon knows: the live ones, plus the finished ones it
    /// has not yet pruned. The terminal entries come from their records, not
    /// from memory, so what the list says does not depend on whether the daemon
    /// happens to have restarted since.
    pub fn list(&self) -> Vec<ProcessInfo> {
        let live = self.table.lock().unwrap();
        let mut all: Vec<ProcessInfo> = live.values().map(|e| e.snapshot()).collect();
        all.extend(
            records::scan_finished(self.dir())
                .iter()
                .filter(|r| !live.contains_key(&r.id))
                .map(ProcessRecord::info),
        );
        all
    }

    pub fn status(&self, id: &str) -> Option<ProcessInfo> {
        if let Some(entry) = self.table.lock().unwrap().get(id) {
            return Some(entry.snapshot());
        }
        records::load(&self.dir().join(id))
            .filter(|record| record.ended.is_some())
            .map(|record| record.info())
    }

    /// A process's captured output — including a finished one's, which is the
    /// whole point of keeping its directory.
    pub fn logs(&self, id: &str, tail: Option<usize>) -> Option<Vec<ProcessLogLine>> {
        let live = self.table.lock().unwrap().get(id).cloned();
        let (log_path, err_path) = match live {
            Some(entry) => (entry.log_path.clone(), entry.err_path.clone()),
            None => {
                let proc_dir = self.dir().join(id);
                let record = records::load(&proc_dir)?;
                log_paths(&record.spec, &proc_dir)
            }
        };
        let limit = tail.unwrap_or(DEFAULT_LOG_LINES);
        let mut stream = LogStream::Stdout;
        let mut lines = tail::read_last_lines(&log_path, limit);
        if lines.is_empty() {
            if let Some(err_path) = &err_path {
                lines = tail::read_last_lines(err_path, limit);
                stream = LogStream::Stderr;
            }
        }
        Some(
            lines
                .into_iter()
                .map(|(offset, line)| ProcessLogLine {
                    stream,
                    line,
                    offset,
                })
                .collect(),
        )
    }
}

/// A typed launch failure, mapped to a protocol error code at the service edge.
#[derive(Debug, thiserror::Error)]
pub enum StartError {
    #[error("a process needs a program to run")]
    EmptyProgram,
    #[error("'{0}' is not a usable process id")]
    InvalidId(String),
    #[error("cannot spawn {program}: {source}")]
    Spawn {
        program: String,
        #[source]
        source: std::io::Error,
    },
}

struct PreparedIo {
    out: Stdio,
    err: Stdio,
    log_path: PathBuf,
    err_path: Option<PathBuf>,
    tail_from: u64,
}

fn log_paths(spec: &ProcessSpec, proc_dir: &Path) -> (PathBuf, Option<PathBuf>) {
    match &spec.log {
        LogSource::Capture => (proc_dir.join("output.log"), None),
        LogSource::File(path) => (resolve_external(path, spec), Some(proc_dir.join("jvm.log"))),
    }
}

fn resolve_external(path: &Path, spec: &ProcessSpec) -> PathBuf {
    match &spec.cwd {
        Some(cwd) if path.is_relative() => cwd.join(path),
        _ => path.to_path_buf(),
    }
}

// Owner-only: an old client prints its own session id to stdout.
fn open_log(path: &Path) -> std::io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

fn prepare_stdio(spec: &ProcessSpec, proc_dir: &Path) -> std::io::Result<PreparedIo> {
    std::fs::create_dir_all(proc_dir)?;
    let (log_path, err_path) = log_paths(spec, proc_dir);
    match &spec.log {
        LogSource::Capture => {
            let tail_from = std::fs::metadata(&log_path).map(|m| m.len()).unwrap_or(0);
            let file = open_log(&log_path)?;
            let err = file.try_clone()?;
            Ok(PreparedIo {
                out: file.into(),
                err: err.into(),
                log_path,
                err_path,
                tail_from,
            })
        }
        LogSource::File(_) => {
            let err_file = open_log(err_path.as_deref().expect("file source has an err path"))?;
            // The process rotates/rewrites its own log; only lines written
            // after this spawn are its output.
            let tail_from = std::fs::metadata(&log_path).map(|m| m.len()).unwrap_or(0);
            Ok(PreparedIo {
                out: Stdio::null(),
                err: err_file.into(),
                log_path,
                err_path,
                tail_from,
            })
        }
    }
}

fn build_command(spec: &ProcessSpec, out: Stdio, err: Stdio) -> Command {
    let mut cmd = Command::new(&spec.program);
    cmd.args(&spec.args)
        .stdin(Stdio::null())
        .stdout(out)
        .stderr(err);
    // The child must outlive the daemon: no kill_on_drop, and its own process
    // group so terminal signals aimed at hestiad never reach it.
    #[cfg(unix)]
    cmd.process_group(0);
    #[cfg(windows)]
    cmd.creation_flags(
        windows_sys::Win32::System::Threading::CREATE_NEW_PROCESS_GROUP
            | windows_sys::Win32::System::Threading::CREATE_NO_WINDOW,
    );
    if let Some(cwd) = &spec.cwd {
        cmd.current_dir(cwd);
    }
    for (key, value) in &spec.env {
        cmd.env(key, value);
    }
    cmd
}

async fn supervise(
    entry: Arc<Entry>,
    mut watched: Watched,
    record: ProcessRecord,
    events: Option<Arc<dyn ProcessEvents>>,
    proc_dir: PathBuf,
    tail_from: u64,
    on_exit: Option<ExitObserver>,
) {
    let tailer = tail::spawn(
        entry.log_path.clone(),
        tail_from,
        record.id.clone(),
        events.clone(),
    );
    let code = wait_or_stop(&entry, &mut watched, &record.id).await;
    tailer.finish().await;

    let killed = entry.stopping.load(Ordering::SeqCst);
    let success = !killed && code == Some(0);
    let state = if killed {
        ProcessState::Killed
    } else {
        ProcessState::Exited
    };
    {
        let mut info = entry.info.lock().unwrap();
        info.state = state;
        info.exit_code = code;
    }
    if success || killed {
        tracing::info!(id = %record.id, ?state, exit_code = code, "process exited");
    } else {
        tracing::warn!(id = %record.id, ?state, exit_code = code, "process exited with failure");
    }
    emit(
        events.as_ref(),
        &ProcessExitEvent {
            id: record.id.clone(),
            state,
            exit_code: code,
            success,
        },
    );

    records::save(&proc_dir, &record.end(state, code, common::now_unix()));
    if let Some(observe) = &on_exit {
        observe(&entry.snapshot());
    }
}

async fn wait_or_stop(entry: &Entry, watched: &mut Watched, id: &str) -> Option<i32> {
    match watched {
        Watched::Owned { child, tree } => {
            let pid = child.id().unwrap_or(0);
            tokio::select! {
                status = child.wait() => status.ok().and_then(|s| s.code()),
                _ = entry.stop_notify.notified() => {
                    tracing::debug!(id, pid, "requesting graceful stop");
                    match tree.as_ref() {
                        Some(tree) => tree.stop(),
                        None => identity::request_stop(pid),
                    }
                    let graceful = tokio::select! {
                        status = child.wait() => Some(status),
                        _ = tokio::time::sleep(STOP_GRACE) => None,
                    };
                    match graceful {
                        Some(status) => status.ok().and_then(|s| s.code()),
                        None => {
                            tracing::warn!(id, pid, "grace period expired; killing process");
                            match tree.as_ref() {
                                Some(tree) => tree.kill(),
                                None => identity::kill(pid),
                            }
                            child.wait().await.ok().and_then(|s| s.code())
                        }
                    }
                }
            }
        }
        Watched::Adopted { pid, token } => {
            let (pid, token) = (*pid, *token);
            loop {
                tokio::select! {
                    _ = tokio::time::sleep(ADOPTED_POLL) => {
                        if !identity::is_same(pid, token) {
                            return None;
                        }
                    }
                    _ = entry.stop_notify.notified() => {
                        tracing::debug!(id, pid, "requesting graceful stop of adopted process");
                        identity::request_stop(pid);
                        let deadline = tokio::time::Instant::now() + STOP_GRACE;
                        let mut killed = false;
                        while identity::is_same(pid, token) {
                            if !killed && tokio::time::Instant::now() >= deadline {
                                tracing::warn!(id, pid, "grace period expired; killing adopted process");
                                killed = true;
                                identity::kill(pid);
                            }
                            tokio::time::sleep(Duration::from_millis(200)).await;
                        }
                        return None;
                    }
                }
            }
        }
    }
}
