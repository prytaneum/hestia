use engine::{Engine, ServerCreateSpec, ServerUpdateSpec};
use proto::server::{
    ServerCreate, ServerCreateParams, ServerDone, ServerUpdate, ServerUpdateParams,
};
use proto::warning::WarningInfo;

use super::job::{Jobs, Spec};
use crate::runtime::{create_key, server_info, Claim, JobKind};

/// What both server jobs settle with: the wire's view of the server, plus
/// whatever the provision degraded on. Assembled where the engine is in reach.
fn provisioned(engine: &Engine, outcome: (engine::ServerRecord, Vec<WarningInfo>)) -> ServerDone {
    let (record, warnings) = outcome;
    let accepts = engine.server_accepts(&record.profile.flavor);
    ServerDone {
        server: server_info(record, None, accepts),
        warnings,
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
    /// guard that admitted it.
    pub fn start_server_update(
        &self,
        server_id: String,
        params: ServerUpdateParams,
        claim: Claim,
    ) -> String {
        let spec = Spec {
            id: params.id.clone(),
            prefix: "server-update",
            claim: Some(claim),
        };

        self.run::<ServerUpdate, _>(spec, move |engine, reporter| {
            Box::pin(async move {
                let update = ServerUpdateSpec {
                    server: server_id,
                    version: params.version,
                    loader_version: params.loader_version,
                    allow_downgrade: params.allow_downgrade,
                };
                let outcome = engine.update_server(update, &reporter.job()).await?;
                Ok(provisioned(engine, outcome))
            })
        })
    }
}
