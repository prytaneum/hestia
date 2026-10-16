//! The daemon's long-lived collaborators in one place — the anti-churn seam a
//! new subsystem hangs off, mirroring the engine's aggregate root.

mod event_hub;
mod managers;
mod metrics;
mod network;
mod presence;
pub mod router;
mod scheduler;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use engine::{Engine, InstanceRecord, ServerRecord};
use proto::error::ErrorInfo;
use proto::instance::InstanceInfo;
use proto::process::{LogSource, ProcessInfo, ProcessSpec, ProcessState};
use proto::server::ServerInfo;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::Notify;

pub use engine::error_info as engine_error;
pub use engine::{ExitObserver, ProcessSupervisor, StartError};
pub use event_hub::EventHub;
pub use managers::{
    BackupJob, BackupManager, Cancellations, ContentJob, ContentManager, InstanceLaunchManager,
    JavaInstallManager, JobEntry, LaunchOrder, ModpackJob, ModpackManager, ServerCreateManager,
    ServerUpdateManager, TransferJob, TransferManager, UpdateManager,
};
pub use metrics::spawn_metrics_sampler;
pub use network::spawn_network_watcher;
pub use presence::spawn_presence_updater;
pub use router::{error_response, require_account, Channels, Router};
pub use scheduler::spawn_backup_scheduler;

// The supervisor key vocabulary lives in `proto::naming`: a front-end names the
// process it follows from the entry's id alone, so both sides must derive the
// same keys.
pub use proto::naming::{
    instance_id_of_session, instance_process_id, instance_session_id, instance_session_prefix,
    server_process_id,
};

// A stop that takes longer than this is not waited out: the restart is
// abandoned rather than started over a server that may still hold its world.
const RESTART_STOP_WAIT: Duration = Duration::from_secs(120);
const RESTART_STOP_POLL: Duration = Duration::from_millis(250);

pub(crate) fn now_unix() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn server_info(
    record: ServerRecord,
    process: Option<proto::process::ProcessInfo>,
    accepts: Vec<proto::content::ContentKind>,
) -> ServerInfo {
    let ready = record.ready();
    ServerInfo {
        id: record.id,
        name: record.name,
        flavor: record.profile.flavor,
        game_version: record.profile.game_version,
        loader_version: record.profile.loader_version,
        java_major: record.profile.java_major,
        created_unix: record.created_unix,
        ready,
        game_port: record.game_port,
        console: record.rcon.is_some(),
        accepts,
        process,
    }
}

pub fn instance_info(
    record: InstanceRecord,
    sessions: Vec<proto::process::ProcessInfo>,
    accepts: Vec<proto::content::ContentKind>,
) -> InstanceInfo {
    InstanceInfo {
        id: record.id,
        name: record.name,
        flavor: record.profile.flavor,
        game_version: record.profile.game_version,
        loader_version: record.profile.loader_version,
        java_major: record.profile.java_major,
        created_unix: record.created_unix,
        last_played_unix: record.last_played_unix,
        playtime_seconds: record.playtime_seconds,
        accepts,
        sessions,
    }
}

pub struct Runtime {
    engine: Arc<Engine>,
    hub: Arc<EventHub>,
    java_installs: JavaInstallManager,
    server_creates: ServerCreateManager,
    server_updates: ServerUpdateManager,
    instance_launches: InstanceLaunchManager,
    backups: BackupManager,
    content_jobs: ContentManager,
    modpack_jobs: ModpackManager,
    transfers: TransferManager,
    updates: UpdateManager,
    processes: Arc<ProcessSupervisor>,
    log_path: PathBuf,
    cancellations: Cancellations,
    started: Instant,
    stop: Notify,
    stop_processes: AtomicBool,
}

impl Runtime {
    pub fn new(log_path: PathBuf, override_home: Option<&std::path::Path>) -> Self {
        let engine = Arc::new(Engine::new(override_home));
        let hub = Arc::new(EventHub::default());
        // One registry for every cancellable job, keyed by job id — what
        // `job.cancel` looks a running job up in, whichever manager owns it.
        let cancellations = Cancellations::new();
        let java_installs =
            JavaInstallManager::new(engine.clone(), hub.clone(), cancellations.clone());
        let session_engine = engine.clone();
        let on_exit: ExitObserver = Arc::new(move |info: &proto::process::ProcessInfo| {
            let Some(instance_id) = instance_id_of_session(&info.id) else {
                return;
            };
            let elapsed = now_unix() - info.started_unix;
            if let Err(e) = session_engine
                .instances()
                .add_playtime(&instance_id, elapsed)
            {
                tracing::warn!(instance = %instance_id, error = %e, "failed to record playtime");
            }
            session_engine.record_plays(&instance_id);
            session_engine.finish_instance_sync(&info.id);
        });
        let processes = engine.processes().clone();
        processes.attach(hub.clone(), Some(on_exit));
        let server_creates =
            ServerCreateManager::new(engine.clone(), hub.clone(), cancellations.clone());
        let server_updates =
            ServerUpdateManager::new(engine.clone(), hub.clone(), cancellations.clone());
        let instance_launches = InstanceLaunchManager::new(
            engine.clone(),
            hub.clone(),
            processes.clone(),
            cancellations.clone(),
        );
        let backups = BackupManager::new(engine.clone(), hub.clone(), cancellations.clone());
        let content_jobs = ContentManager::new(engine.clone(), hub.clone(), cancellations.clone());
        let modpack_jobs = ModpackManager::new(engine.clone(), hub.clone(), cancellations.clone());
        let transfers = TransferManager::new(engine.clone(), hub.clone(), cancellations.clone());
        let updates = UpdateManager::new(engine.clone(), hub.clone(), cancellations.clone());
        Runtime {
            engine,
            hub,
            java_installs,
            server_creates,
            server_updates,
            instance_launches,
            backups,
            content_jobs,
            modpack_jobs,
            transfers,
            updates,
            cancellations,
            processes,
            log_path,
            started: Instant::now(),
            stop: Notify::new(),
            stop_processes: AtomicBool::new(false),
        }
    }

    /// A server's record merged with its live process state (when started).
    pub fn server_view(&self, record: ServerRecord) -> ServerInfo {
        let process = self.processes.status(&server_process_id(&record.id));
        let accepts = self.engine.server_accepts(&record.profile.flavor);
        server_info(record, process, accepts)
    }

    /// Spawn a server's process from its launch plan — the one way a server
    /// is started, whether a client asked or a scheduled restart did.
    pub async fn start_server(&self, id: &str) -> Result<ProcessInfo, ErrorInfo> {
        let (_, plan) = self.engine.server_launch_plan(id).map_err(engine_error)?;
        let spec = ProcessSpec {
            id: server_process_id(id),
            program: plan.program.to_string_lossy().into_owned(),
            args: plan.args,
            log: LogSource::File(plan.cwd.join("logs").join("latest.log")),
            cwd: Some(plan.cwd),
            env: Default::default(),
        };
        match self.processes.start(spec).await {
            Ok(info) => Ok(info),
            Err(StartError::EmptyProgram | StartError::InvalidId(_)) => Err(ErrorInfo::Internal {
                detail: "invalid launch plan".into(),
            }),
            Err(e @ StartError::Spawn { .. }) => Err(ErrorInfo::Internal {
                detail: format!("cannot spawn the server: {e}"),
            }),
        }
    }

    /// Stop a running server, wait for its process to exit, and start it
    /// again — shared by the `server.restart` channel and a scheduled restart.
    pub async fn restart_server(&self, record: &ServerRecord) -> Result<ProcessInfo, ErrorInfo> {
        let process_id = server_process_id(&record.id);
        self.processes.stop(&process_id);
        let deadline = tokio::time::Instant::now() + RESTART_STOP_WAIT;
        while self
            .processes
            .status(&process_id)
            .is_some_and(|p| p.state == ProcessState::Running)
        {
            if tokio::time::Instant::now() >= deadline {
                return Err(ErrorInfo::StopTimedOut {
                    name: record.name.clone(),
                });
            }
            tokio::time::sleep(RESTART_STOP_POLL).await;
        }
        self.start_server(&record.id).await
    }

    pub fn instance_view(&self, record: InstanceRecord) -> InstanceInfo {
        let sessions = self.instance_sessions(&record.id);
        let accepts = self.engine.instance_accepts(&record.profile.flavor);
        instance_info(record, sessions, accepts)
    }

    /// Every live session of an instance, newest first.
    pub fn instance_sessions(&self, id: &str) -> Vec<proto::process::ProcessInfo> {
        let prefix = instance_session_prefix(id);
        let mut sessions: Vec<_> = self
            .processes
            .list()
            .into_iter()
            .filter(|p| p.id.starts_with(&prefix))
            .collect();
        sessions.sort_by_key(|s| std::cmp::Reverse(s.started_unix));
        sessions
    }

    /// True while any session of the instance is still running.
    pub fn instance_running(&self, id: &str) -> bool {
        self.instance_sessions(id)
            .iter()
            .any(|p| p.state == proto::process::ProcessState::Running)
    }

    /// Stop every session of an instance; returns how many were signalled.
    pub fn stop_instance_sessions(&self, id: &str) -> usize {
        self.instance_sessions(id)
            .into_iter()
            .filter(|p| self.processes.stop(&p.id))
            .count()
    }

    /// Discard the supervisor state of every session of an instance.
    pub fn discard_instance_sessions(&self, id: &str) {
        for session in self.instance_sessions(id) {
            self.processes.discard(&session.id);
        }
    }

    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    pub fn hub(&self) -> &EventHub {
        &self.hub
    }

    /// Every cancellable job in flight, whichever manager owns it.
    pub fn cancellations(&self) -> &Cancellations {
        &self.cancellations
    }

    pub fn java_installs(&self) -> &JavaInstallManager {
        &self.java_installs
    }

    pub fn server_creates(&self) -> &ServerCreateManager {
        &self.server_creates
    }

    pub fn server_updates(&self) -> &ServerUpdateManager {
        &self.server_updates
    }

    pub fn instance_launches(&self) -> &InstanceLaunchManager {
        &self.instance_launches
    }

    pub fn backups(&self) -> &BackupManager {
        &self.backups
    }

    pub fn content_jobs(&self) -> &ContentManager {
        &self.content_jobs
    }

    pub fn modpack_jobs(&self) -> &ModpackManager {
        &self.modpack_jobs
    }

    pub fn transfers(&self) -> &TransferManager {
        &self.transfers
    }

    pub fn updates(&self) -> &UpdateManager {
        &self.updates
    }

    pub fn processes(&self) -> &ProcessSupervisor {
        &self.processes
    }

    pub fn log_path(&self) -> &PathBuf {
        &self.log_path
    }

    pub fn uptime_seconds(&self) -> i64 {
        self.started.elapsed().as_secs() as i64
    }

    /// Ask the serve loop to shut down (the `daemon.stop` handler calls this).
    pub fn request_stop(&self, stop_processes: bool) {
        self.stop_processes.store(stop_processes, Ordering::SeqCst);
        self.stop.notify_waiters();
    }

    /// Resolves when a stop has been requested.
    pub async fn stopped(&self) {
        self.stop.notified().await;
    }

    /// An OS-signal shutdown never stops workloads; only an explicit
    /// `daemon.stop` with `stop_processes` does.
    pub async fn shutdown_workloads(&self) {
        if self.stop_processes.load(Ordering::SeqCst) {
            self.processes.stop_all_and_wait().await;
        }
    }
}

/// What every handler receives: the shared runtime, the calling connection's
/// outbound channel (so streaming handlers like `events.subscribe` can push to
/// it).
#[derive(Clone)]
pub struct HandlerContext {
    pub runtime: Arc<Runtime>,
    pub conn_id: u64,
    pub out: UnboundedSender<String>,
}
