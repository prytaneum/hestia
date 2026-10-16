//! Vanilla (Mojang) — server jar and client, resolved straight from a version's
//! piston-meta detail JSON.

use anyhow::Result;
use async_trait::async_trait;
use proto::minecraft::GameVersion;

use super::meta::mojang;
use super::profile::{InstanceProfile, ServerJar, ServerLaunch, ServerProfile};
use super::provider::{FlavorInfo, FlavorProvider, ResolveRequest, BOTH_SIDES};

const ID: &str = "vanilla";

const INFO: FlavorInfo = FlavorInfo {
    id: ID,
    name: "Vanilla",
    summary: "Mojang's game, unchanged. Takes datapacks only — no mods or plugins.",
    loads: None,
    requires: &[],
    sides: BOTH_SIDES,
};

pub struct Vanilla;

#[async_trait]
impl FlavorProvider for Vanilla {
    fn info(&self) -> &FlavorInfo {
        &INFO
    }

    async fn versions(&self) -> Result<Vec<GameVersion>> {
        mojang::versions().await
    }

    async fn resolve_server(&self, request: &ResolveRequest<'_>) -> Result<ServerProfile> {
        let version = mojang::version_json(&request.version).await?;
        Ok(ServerProfile {
            flavor: ID.to_string(),
            game_version: request.version.clone(),
            loader_version: None,
            jar: ServerJar::Download(mojang::server_artifact(&version)?),
            launch: ServerLaunch::Jar,
            java_major: mojang::java_major(&version),
            jvm_args: Vec::new(),
        })
    }

    async fn resolve_client(&self, request: &ResolveRequest<'_>) -> Result<InstanceProfile> {
        let version = mojang::version_json(&request.version).await?;
        Ok(InstanceProfile {
            flavor: ID.to_string(),
            game_version: request.version.clone(),
            loader_version: None,
            client: mojang::client_artifact(&version)?,
            libraries: mojang::libraries(&version),
            natives: mojang::natives(&version),
            asset_index: mojang::asset_index(&version)?,
            java_major: mojang::java_major(&version),
            main_class: mojang::main_class(&version),
            jvm_args: mojang::jvm_args(&version),
            game_args: mojang::game_args(&version),
        })
    }
}
