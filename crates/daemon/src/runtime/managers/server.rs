use engine::{Engine, ServerCreateSpec, ServerRecord, ServerUpdateSpec};
use proto::error::EntryKind;
use proto::process::ProcessState;
use proto::server::{
    ServerCreate, ServerCreateParams, ServerDone, ServerUpdate, ServerUpdateParams,
};
use proto::warning::WarningInfo;

use super::job::{Jobs, Spec};
use crate::runtime::{
    create_key, server_info, server_process_id, start_server, stop_server, Claim, JobKind,
};

/// What both server jobs settle with: the wire's view of the server, plus
/// whatever the provision degraded on. Assembled where the engine is in reach.
fn provisioned(engine: &Engine, outcome: (engine::ServerRecord, Vec<WarningInfo>)) -> ServerDone {
    let (record, warnings) = outcome;
    let accepts = engine.server_accepts(&record.profile.flavor);
    ServerDone {
        server: server_info(record, None, accepts),
        warnings,
        failures: Vec::new(),
    }
}

impl Jobs {
    /// Start a provisioning job off-thread, one per server name at a time.
    /// Returns the job id, or `None` if that name is already being created.
    pub fn start_server_create(&self, params: ServerCreateParams) -> Option<String> {
        let name = match params.name.trim() {
            "" => format!("{}-{}", params.flavor, params.version),
            name => name.to_string(),
        };
        let claim = self
            .claims
            .claim(create_key(&name), JobKind::Install)
            .ok()?;
        let spec = Spec {
            id: params.id.clone(),
            prefix: "server-create",
            claim: Some(claim),
            subject: None,
        };

        Some(self.run::<ServerCreate, _>(spec, move |engine, reporter| {
            Box::pin(async move {
                let create = ServerCreateSpec {
                    name: params.name,
                    flavor: params.flavor,
                    version: params.version,
                    loader_version: params.loader_version,
                    port: params.port,
                    config: params.config,
                    eula: params.eula,
                };
                let outcome = engine.provision_server(create, &reporter.job()).await?;
                Ok(provisioned(engine, outcome))
            })
        }))
    }

    /// Start an update job off-thread; `claim` is the server's, taken by the
    /// guard that admitted it. With `restart`, a running server is stopped
    /// first and started again once the update settles, whichever way it
    /// settles.
    pub fn start_server_update(
        &self,
        record: ServerRecord,
        params: ServerUpdateParams,
        claim: Claim,
    ) -> String {
        let spec = Spec {
            id: params.id.clone(),
            prefix: "server-update",
            claim: Some(claim),
            subject: Spec::subject(EntryKind::Server, &record.id),
        };
        let processes = self.processes.clone();

        self.run::<ServerUpdate, _>(spec, move |engine, reporter| {
            Box::pin(async move {
                let process_id = server_process_id(&record.id);
                let stopped = params.restart
                    && processes
                        .status(&process_id)
                        .is_some_and(|p| p.state == ProcessState::Running);
                if stopped {
                    stop_server(&processes, &record).await?;
                }
                let update = ServerUpdateSpec {
                    server: record.id.clone(),
                    version: params.version,
                    loader_version: params.loader_version,
                    allow_downgrade: params.allow_downgrade,
                };
                let outcome = engine
                    .update_server_with_pack(update, params.pack, &reporter.job())
                    .await;
                let restarted = match stopped {
                    true => Some(start_server(engine, &processes, &record.id).await),
                    false => None,
                };
                if let Some(Err(error)) = &restarted {
                    tracing::warn!(server = %record.id, error = %error, "server not restarted after its update");
                }
                let changed = outcome?;
                let mut done = provisioned(engine, (changed.record, changed.warnings));
                done.failures = changed.failures;
                done.server.process = processes.status(&process_id);
                if let Some(Err(error)) = restarted {
                    done.warnings.push(WarningInfo::ServerNotRestarted {
                        server: record.name,
                        detail: error.to_string(),
                    });
                }
                Ok(done)
            })
        })
    }
}
