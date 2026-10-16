use std::sync::Arc;

use engine::Engine;
use proto::error::EntryKind;
use proto::minecraft::ProvisionProgress;
use proto::modpack::{
    ModpackCancelledEvent, ModpackDoneEvent, ModpackErrorEvent, ModpackMove, ModpackProgressEvent,
    ModpackRef, ModpackTarget,
};

use super::content::Entry;
use super::job::{progress_event, settle, Cancellations, Runner, Spec};
use crate::runtime::{instance_process_id, server_process_id, EventHub};

/// One pack install or update — what `ModpackManager::start` runs off-thread.
pub enum ModpackJob {
    InstallInstance {
        pack: ModpackRef,
        target: ModpackTarget,
    },
    InstallServer {
        pack: ModpackRef,
        target: ModpackTarget,
        eula: bool,
        port: Option<u16>,
    },
    Update {
        entry: Entry,
        to: ModpackMove,
    },
}

impl ModpackJob {
    /// The in-flight key: the entry's process id, shared with the content and
    /// backup jobs so nothing else touches the tree while a pack lands. An
    /// install that *creates* its entry has none — the entry does not exist to
    /// conflict with — so it takes no claim.
    fn key(&self) -> Option<String> {
        match self {
            ModpackJob::InstallInstance {
                target: ModpackTarget::Existing { entry },
                ..
            } => Some(instance_process_id(entry)),
            ModpackJob::InstallServer {
                target: ModpackTarget::Existing { entry },
                ..
            } => Some(server_process_id(entry)),
            ModpackJob::Update { entry, .. } => Some(entry.key()),
            _ => None,
        }
    }

    fn id_prefix(&self) -> &'static str {
        match self {
            ModpackJob::InstallInstance { .. } => "instance-modpack-install",
            ModpackJob::InstallServer { .. } => "server-modpack-install",
            ModpackJob::Update { entry, .. } => match entry.kind {
                EntryKind::Server => "server-modpack-update",
                EntryKind::Instance => "instance-modpack-update",
            },
        }
    }

    async fn run(
        self,
        engine: &Engine,
        on_progress: &engine::Job<'_>,
    ) -> anyhow::Result<engine::ModpackOutcome> {
        match self {
            ModpackJob::InstallInstance { pack, target } => {
                engine
                    .install_instance_modpack(&pack, &target, on_progress)
                    .await
            }
            ModpackJob::InstallServer {
                pack,
                target,
                eula,
                port,
            } => {
                engine
                    .install_server_modpack(&pack, &target, eula, port, on_progress)
                    .await
            }
            ModpackJob::Update { entry, to } => {
                engine
                    .update_entry_modpack(entry.entry_ref(), &to, on_progress)
                    .await
            }
        }
    }
}

pub struct ModpackManager {
    runner: Runner<String>,
}

impl ModpackManager {
    pub fn new(engine: Arc<Engine>, hub: Arc<EventHub>, cancellations: Cancellations) -> Self {
        ModpackManager {
            runner: Runner::new(engine, hub, cancellations),
        }
    }

    /// Whether a pack job is still running for this entry key
    /// (`server-<id>` / `instance-<id>`).
    pub fn in_flight(&self, key: &str) -> bool {
        self.runner.in_flight(key)
    }

    /// Start a pack job off-thread, one per entry at a time. Returns the job id,
    /// or `None` if that entry is already busy.
    pub fn start(&self, job: ModpackJob, id: String) -> Option<String> {
        let spec = Spec {
            id,
            prefix: job.id_prefix(),
            key: job.key(),
            progress: progress_event(|id, progress: &ProvisionProgress| ModpackProgressEvent {
                id,
                progress: progress.clone(),
            }),
            done: settle(|id, outcome: engine::ModpackOutcome| ModpackDoneEvent {
                id,
                entry: outcome.entry,
                entry_name: outcome.entry_name,
                pack: outcome.pack,
                failures: outcome.failures,
                warnings: outcome.warnings,
            }),
            cancelled: Some(settle(|id, ()| ModpackCancelledEvent { id })),
            error: settle(|id, e| ModpackErrorEvent {
                id,
                error: crate::runtime::engine_error(e),
            }),
        };

        self.runner.start(spec, move |engine, reporter| {
            Box::pin(async move { job.run(engine, &reporter.job()).await })
        })
    }
}
