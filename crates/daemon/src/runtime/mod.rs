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
use std::time::Instant;

use engine::{Engine, InstanceRecord, ServerRecord};
use ipc::Peer;
use proto::instance::InstanceInfo;
use proto::server::ServerInfo;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::Notify;

pub use engine::error_info as engine_error;
pub use engine::{ExitObserver, ProcessSupervisor, StartError};
pub use event_hub::EventHub;
pub use managers::{
    BackupJob, BackupManager, Cancellations, ContentJob, ContentManager, DownloadManager,
    InstanceLaunchManager, JavaInstallManager, JobEntry, LaunchOrder, ModpackJob, ModpackManager,
    ServerCreateManager, ServerUpdateManager, TransferJob, TransferManager, UpdateManager,
};
pub use metrics::spawn_metrics_sampler;
pub use network::spawn_network_watcher;
pub use presence::spawn_presence_updater;
pub use router::{error_response, Channels, Router};
pub use scheduler::spawn_backup_scheduler;

// The supervisor key vocabulary lives in `proto::naming`: a front-end names the
// process it follows from the entry's id alone, so both sides must derive the
// same keys.
pub use proto::naming::{
    instance_id_of_session, instance_process_id, instance_session_id, instance_session_prefix,
    server_process_id,
};

fn now_unix() -> i64 {
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
    downloads: DownloadManager,
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
        let downloads = DownloadManager::new(engine.clone(), hub.clone(), cancellations.clone());
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
            downloads,
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

    pub fn downloads(&self) -> &DownloadManager {
        &self.downloads
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
/// it), and the verified peer.
#[derive(Clone)]
pub struct HandlerContext {
    pub runtime: Arc<Runtime>,
    pub conn_id: u64,
    pub out: UnboundedSender<String>,
    // The verified peer identity: the seam a future token/cert auth check reads.
    // Carried on every request even though no handler consumes it yet.
    #[allow(dead_code)]
    pub peer: Peer,
}
