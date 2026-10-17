use std::collections::BTreeMap;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use engine::{Engine, LaunchRequest};
use proto::error::ErrorInfo;
use proto::instance::{InstanceLaunch, InstanceLaunchDone, QuickPlay};
use proto::process::{LogSource, ProcessSpec};

use super::job::{Cancellations, Runner, Spec};
use crate::runtime::{
    instance_session_id, instance_session_prefix, EventHub, ProcessSupervisor, StartError,
};

pub struct InstanceLaunchManager {
    runner: Runner<String>,
    processes: Arc<ProcessSupervisor>,
    /// Session ids reserved between seq allocation and the supervisor accepting
    /// them, so two concurrent launches of one instance can't collide on a seq.
    reserved: Arc<Mutex<HashSet<String>>>,
}

impl InstanceLaunchManager {
    pub fn new(
        engine: Arc<Engine>,
        hub: Arc<EventHub>,
        processes: Arc<ProcessSupervisor>,
        cancellations: Cancellations,
    ) -> Self {
        InstanceLaunchManager {
            runner: Runner::new(engine, hub, cancellations),
            processes,
            reserved: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// Prepare and spawn a fresh session of an instance off-thread. Instances may
    /// run several sessions at once, so this takes no in-flight claim — it does
    /// not refuse a running instance. Returns the launch job id.
    pub fn start(&self, order: LaunchOrder) -> Option<String> {
        let LaunchOrder {
            instance_id,
            account,
            reconcile,
            quick_play,
            offline,
            id,
        } = order;
        let (session_id, seq) = self.reserve_session(&instance_id);

        let spec = Spec {
            id,
            prefix: "instance-launch",
            key: None,
        };

        let processes = self.processes.clone();
        let reserved = self.reserved.clone();
        self.runner
            .start::<InstanceLaunch, _>(spec, move |engine, reporter| {
                Box::pin(async move {
                    let outcome = launch(
                        engine,
                        &processes,
                        &session_id,
                        LaunchRequest {
                            instance: &instance_id,
                            account: &account,
                            session_seq: seq,
                            reconcile,
                            quick_play,
                            offline,
                        },
                        &reporter.job(),
                    )
                    .await;
                    // The supervisor now owns the id (or the launch failed) — release it.
                    reserved.lock().unwrap().remove(&session_id);
                    outcome
                })
            })
    }

    /// Claim the next free session id for an instance under the reservation lock,
    /// counting both live sessions and ids already reserved but not yet spawned.
    fn reserve_session(&self, instance_id: &str) -> (String, u32) {
        let prefix = instance_session_prefix(instance_id);
        let mut reserved = self.reserved.lock().unwrap();
        let live_max = self
            .processes
            .list()
            .into_iter()
            .filter_map(|p| {
                p.id.strip_prefix(&prefix)
                    .and_then(|s| s.parse::<u32>().ok())
            })
            .max();
        let mut seq = live_max.map_or(1, |n| n + 1);
        while reserved.contains(&instance_session_id(instance_id, seq)) {
            seq += 1;
        }
        let session_id = instance_session_id(instance_id, seq);
        reserved.insert(session_id.clone());
        (session_id, seq)
    }
}

/// Materialise the instance, then hand the plan to the supervisor under the
/// session's own id and per-session log file.
async fn launch(
    engine: &Engine,
    processes: &ProcessSupervisor,
    session_id: &str,
    request: LaunchRequest<'_>,
    on_progress: &engine::Job<'_>,
) -> anyhow::Result<InstanceLaunchDone> {
    let instance_id = request.instance.to_string();
    let prepared = engine.prepare_instance(request, on_progress).await?;

    let spec = ProcessSpec {
        id: session_id.to_string(),
        program: prepared.plan.program.to_string_lossy().into_owned(),
        args: prepared.plan.args,
        log: LogSource::File(prepared.log_file),
        cwd: Some(prepared.plan.cwd),
        env: BTreeMap::new(),
    };
    match processes.start(spec).await {
        Ok(info) => {
            if let Err(e) = engine.instances().mark_launched(&instance_id) {
                tracing::warn!(instance = %instance_id, error = %e, "failed to stamp last-played");
            }
            Ok(InstanceLaunchDone {
                process_id: info.id,
                pid: info.pid,
                warnings: prepared.warnings,
            })
        }
        Err(StartError::EmptyProgram | StartError::InvalidId(_)) => Err(ErrorInfo::Internal {
            detail: "invalid launch plan".to_string(),
        }
        .into()),
        Err(e @ StartError::Spawn { .. }) => Err(ErrorInfo::Internal {
            detail: format!("cannot spawn the game: {e}"),
        }
        .into()),
    }
}

/// What a caller is asking the manager to launch. The daemon's own order,
/// distinct from the engine's [`LaunchRequest`]: it carries the job id and owns
/// its strings, since the launch outlives the request that asked for it.
pub struct LaunchOrder {
    pub instance_id: String,
    /// Account name or uuid; empty picks the sole signed-in one.
    pub account: String,
    /// Off skips the sync/mirror pass — other sessions have the mirror in use.
    pub reconcile: bool,
    /// Join a world or server on start instead of the title screen.
    pub quick_play: Option<QuickPlay>,
    /// Run unauthenticated instead of contacting Microsoft.
    pub offline: bool,
    /// Client-supplied job id; empty asks for an allocated one.
    pub id: String,
}
