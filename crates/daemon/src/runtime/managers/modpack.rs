use engine::Engine;
use proto::error::EntryKind;
use proto::job::{Job, JobSubject};
use proto::minecraft::ProvisionProgress;
use proto::modpack::{ModpackDone, ModpackMove, ModpackRef, ModpackTarget};

use super::content::Entry;
use super::job::{Jobs, Spec};
use crate::runtime::Claim;

/// One pack install or update — what `Jobs::start_modpack` runs off-thread.
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

    fn subject(&self) -> Option<JobSubject> {
        match self {
            ModpackJob::InstallInstance {
                target: ModpackTarget::Existing { entry },
                ..
            } => Spec::subject(EntryKind::Instance, entry),
            ModpackJob::InstallServer {
                target: ModpackTarget::Existing { entry },
                ..
            } => Spec::subject(EntryKind::Server, entry),
            ModpackJob::Update { entry, .. } => entry.subject(),
            _ => None,
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

impl Jobs {
    /// Start a pack job off-thread. An install that creates its entry has no
    /// claim to hold: the entry does not exist yet to conflict with.
    pub fn start_modpack<C>(&self, job: ModpackJob, id: String, claim: Option<Claim>) -> String
    where
        C: Job<Progress = ProvisionProgress, Done = ModpackDone>,
    {
        let spec = Spec {
            id,
            prefix: job.id_prefix(),
            claim,
            subject: job.subject(),
        };

        self.run::<C, _>(spec, move |engine, reporter| {
            Box::pin(async move {
                let outcome = job.run(engine, &reporter.job()).await?;
                Ok(ModpackDone {
                    entry: outcome.entry,
                    entry_name: outcome.entry_name,
                    pack: outcome.pack,
                    failures: outcome.failures,
                    warnings: outcome.warnings,
                })
            })
        })
    }
}
