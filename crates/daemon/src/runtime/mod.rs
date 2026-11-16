//! The daemon's long-lived collaborators in one place — the anti-churn seam a
//! new subsystem hangs off, mirroring the engine's aggregate root.

mod calls;
mod cancellations;
mod changed;
mod claims;
mod event_hub;
mod managers;
mod metrics;
mod network;
mod ready;
pub mod router;
mod scheduler;
mod surface;
mod updates;

use std::fmt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use engine::{Engine, InstanceRecord, ProcessIo, ServerRecord};
use kernel::{Limits, Plugins, Sandbox, Table};
use proto::error::ErrorInfo;
use proto::instance::InstanceInfo;
use proto::process::{ProcessInfo, ProcessSpec};
use proto::server::ServerInfo;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::Notify;

pub use calls::RouterCalls;
pub use cancellations::Cancellations;
pub use changed::Changed;
pub use claims::{create_key, Claim, Claims, JobKind};
pub use engine::error_info as engine_error;
pub use engine::{ExitObserver, ProcessSupervisor, StartError};
pub use event_hub::EventHub;
pub use managers::{BackupJob, ContentJob, ExportOrder, JobEntry, Jobs, LaunchOrder, ModpackJob};
pub use metrics::spawn_metrics_sampler;
pub use network::spawn_network_watcher;
pub use ready::spawn_ready_watcher;
pub use router::{error_response, require_account, Channels, Router};
pub use scheduler::spawn_task_scheduler;
pub use surface::Surface;
pub use updates::spawn_update_watcher;

// The supervisor key vocabulary lives in `proto::naming`: a front-end names the
// process it follows from the entry's id alone, so both sides must derive the
// same keys.
pub use proto::naming::{
    instance_id_of_session, instance_process_id, instance_session_id, instance_session_prefix,
    server_process_id,
};

/// Spawn a server's process from its launch plan — the one way a server is
/// started, whether a client asked, a scheduled restart did, or an update put
/// back a server it had stopped.
pub async fn start_server(
    engine: &Engine,
    processes: &ProcessSupervisor,
    id: &str,
) -> Result<ProcessInfo, ErrorInfo> {
    let (record, plan) = engine.server_launch_plan(id).await.map_err(engine_error)?;
    let ProcessIo { log, stdin, stop } = engine::process_io(record.edition, &plan.cwd);
    let spec = ProcessSpec {
        id: server_process_id(id),
        program: plan.program.to_string_lossy().into_owned(),
        args: plan.args,
        log,
        cwd: Some(plan.cwd),
        env: plan.env.into_iter().collect(),
        stdin,
        stop,
        ..ProcessSpec::default()
    };
    match processes.start(spec).await {
        Ok(info) => Ok(info),
        Err(e) => Err(start_error("server", e)),
    }
}

pub fn start_error(what: &str, error: StartError) -> ErrorInfo {
    match error {
        StartError::EmptyProgram | StartError::InvalidId(_) => ErrorInfo::Internal {
            detail: "invalid launch plan".into(),
        },
        StartError::Spawn { .. } => ErrorInfo::Internal {
            detail: format!("cannot spawn the {what}: {error}"),
        },
    }
}

/// Stop a server and wait for its process to exit. A stop that outlasts the
/// supervisor's wait is an error: the caller abandons its work rather than
/// reuse a tree the server may still hold.
pub async fn stop_server(
    processes: &ProcessSupervisor,
    record: &ServerRecord,
) -> Result<(), ErrorInfo> {
    match processes
        .stop_and_wait(&[server_process_id(&record.id)])
        .await
    {
        true => Ok(()),
        false => Err(ErrorInfo::StopTimedOut {
            name: record.name.clone(),
        }),
    }
}

pub fn server_info(
    engine: &Engine,
    record: ServerRecord,
    process: Option<proto::process::ProcessInfo>,
) -> ServerInfo {
    let ready = record.ready();
    let accepts = engine.server_accepts(record.edition, &record.profile.flavor);
    let ports = engine.servers().ports(&record);
    let icon = engine.servers().icon(&record);
    let console = engine::has_console(&record);
    ServerInfo {
        id: record.id,
        name: record.name,
        flavor: record.profile.flavor,
        game_version: record.profile.game_version,
        loader_version: record.profile.loader_version,
        edition: record.edition,
        java_major: record.profile.runtime.java_major(),
        created_unix: record.created_unix,
        ready,
        game_port: record.game_port,
        ports,
        console,
        accepts,
        icon,
        process,
    }
}

pub fn instance_info(
    engine: &Engine,
    record: InstanceRecord,
    sessions: Vec<proto::process::ProcessInfo>,
) -> InstanceInfo {
    let accepts = engine.instance_accepts(&record.profile.flavor);
    let icon = engine.instances().icon(&record);
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
        icon,
        sessions,
    }
}

pub struct Runtime {
    engine: Arc<Engine>,
    plugins: Plugins,
    sandbox: Option<Arc<Sandbox>>,
    hub: Arc<EventHub>,
    surface: Arc<Surface>,
    jobs: Jobs,
    claims: Claims,
    processes: Arc<ProcessSupervisor>,
    log_path: PathBuf,
    cancellations: Cancellations,
    started: Instant,
    stop: Notify,
    stop_processes: AtomicBool,
}

impl Runtime {
    pub fn new(log_path: PathBuf) -> Self {
        Runtime::over(Engine::new(None), log_path)
    }

    /// Build over a given engine, the seam a test crosses to use a temporary
    /// data home.
    pub fn over(engine: Engine, log_path: PathBuf) -> Self {
        let home = engine.data_home();
        let hub = Arc::new(EventHub::default());
        let surface = Arc::new(Surface::new(hub.clone(), home.to_path_buf()));
        let sandbox = match Sandbox::new(
            home.join("meta").join("plugins"),
            &home.join("plugins.key"),
            Limits::default(),
        ) {
            Ok(sandbox) => Some(Arc::new(sandbox.with_services(surface.clone()))),
            Err(error) => {
                tracing::error!(%error, "the plugin sandbox did not start; WASM plugins will not run");
                None
            }
        };
        let own_programs = [
            common::app::CLI_BIN,
            common::app::DAEMON_BIN,
            common::app::DESKTOP_BIN,
            common::app::TRAY_BIN,
        ]
        .into_iter()
        .filter_map(common::paths::sibling_binary)
        .chain(std::env::current_exe().ok())
        .collect();
        let own_endpoints = vec![ipc::endpoint::default_endpoint()
            .to_string_lossy()
            .into_owned()];
        let plugins = Plugins::new(home, engine::builtins(), sandbox.clone(), Table::kernel())
            .expect(
                "engine::builtins() must list unique ids whose requirements exist and do not cycle",
            )
            .reserving(own_programs, own_endpoints);
        let engine = Arc::new(engine);
        // One registry for every cancellable job, keyed by job id — what
        // `job.cancel` looks a running job up in, whichever manager owns it.
        let cancellations = Cancellations::default();
        let session_engine = engine.clone();
        let on_exit: ExitObserver = Arc::new(move |info: &proto::process::ProcessInfo| {
            let Some(instance_id) = instance_id_of_session(&info.id) else {
                return;
            };
            let elapsed = common::now_unix() - info.started_unix;
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
        match std::env::current_exe() {
            Ok(exe) => processes.set_relay_program(exe),
            Err(e) => tracing::warn!("cannot locate the daemon binary for the stdin relay: {e}"),
        }
        let claims = Claims::default();
        let jobs = Jobs::new(
            engine.clone(),
            hub.clone(),
            claims.clone(),
            processes.clone(),
            cancellations.clone(),
        );
        Runtime {
            engine,
            plugins,
            sandbox,
            hub,
            surface,
            jobs,
            claims,
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
        server_info(&self.engine, record, process)
    }

    /// Stop a running server, wait for its process to exit, and start it
    /// again — shared by the `server.restart` channel and a scheduled restart.
    pub async fn restart_server(&self, record: &ServerRecord) -> Result<ProcessInfo, ErrorInfo> {
        stop_server(&self.processes, record).await?;
        start_server(&self.engine, &self.processes, &record.id).await
    }

    pub fn instance_view(&self, record: InstanceRecord) -> InstanceInfo {
        let sessions = self.instance_sessions(&record.id);
        instance_info(&self.engine, record, sessions)
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

    pub fn process_running(&self, id: &str) -> bool {
        self.processes()
            .status(id)
            .is_some_and(|info| info.state == proto::process::ProcessState::Running)
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

    pub fn plugins(&self) -> &Plugins {
        &self.plugins
    }

    /// Missing when the sandbox could not start; WASM plugins then never run.
    pub fn sandbox(&self) -> Option<&Sandbox> {
        self.sandbox.as_deref()
    }

    pub fn hub(&self) -> &EventHub {
        &self.hub
    }

    /// What plugins draw through and keep their files in.
    pub fn surface(&self) -> &Arc<Surface> {
        &self.surface
    }

    /// Every cancellable job in flight, whichever manager owns it.
    pub fn cancellations(&self) -> &Cancellations {
        &self.cancellations
    }

    pub fn jobs(&self) -> &Jobs {
        &self.jobs
    }

    /// Who holds each entry's tree right now.
    pub fn claims(&self) -> &Claims {
        &self.claims
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
    fn request_stop(&self, stop_processes: bool) {
        self.stop_processes.store(stop_processes, Ordering::SeqCst);
        self.stop.notify_one();
    }

    /// Stop on a short delay so the response being sent reaches the client
    /// before the serve loop shuts down.
    pub fn stop_soon(self: &Arc<Self>, stop_processes: bool) {
        let runtime = self.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            runtime.request_stop(stop_processes);
        });
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

/// Who a request came from, for the request span. A plugin's call carries the
/// plugins that led to it.
#[derive(Clone, Debug)]
pub enum Caller {
    Client,
    Plugin(kernel::Chain),
}

impl fmt::Display for Caller {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Caller::Client => f.write_str("client"),
            Caller::Plugin(chain) => write!(f, "plugin:{}", chain.plugins().join(">")),
        }
    }
}

/// What every handler receives: the shared runtime, the calling connection's
/// outbound channel (so streaming handlers like `events.subscribe` can push to
/// it), and who is calling. `changed` collects what the request changed, for its `changed` event.
#[derive(Clone)]
pub struct HandlerContext {
    pub runtime: Arc<Runtime>,
    pub conn_id: u64,
    pub out: UnboundedSender<String>,
    pub caller: Caller,
    pub changed: Changed,
}
