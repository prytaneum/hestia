//! Vanilla (Mojang) — server jar and client, resolved straight from a version's
//! piston-meta detail JSON.

use std::borrow::Cow;

use anyhow::Result;
use async_trait::async_trait;
use proto::minecraft::GameVersion;

use super::meta::mojang;
use super::profile::{InstanceProfile, ServerJar, ServerLaunch, ServerProfile};
use super::provider::{FlavorInfo, FlavorProvider, ResolveRequest, BOTH_SIDES};

pub(super) const ID: &str = "vanilla";

const INFO: FlavorInfo = FlavorInfo {
    id: Cow::Borrowed(ID),
    name: Cow::Borrowed("Vanilla"),
    summary: Cow::Borrowed("Mojang's game, unchanged. Takes datapacks only — no mods or plugins."),
    loads: None,
    requires: Cow::Borrowed(&[]),
    sides: BOTH_SIDES,
    mrpack_key: None,
    prism_uid: None,
};

const HOSTS: [&str; 5] = [
    "piston-meta.mojang.com",
    "piston-data.mojang.com",
    "launcher.mojang.com",
    "libraries.minecraft.net",
    "resources.download.minecraft.net",
];

/// Whether `url` is on a host Mojang serves the game's own files from, which
/// the launcher fetches under its core policy even inside a plugin's profile.
pub(crate) fn serves(url: &str) -> bool {
    reqwest::Url::parse(url)
        .ok()
        .and_then(|url| url.host_str().map(|host| HOSTS.contains(&host)))
        .unwrap_or(false)
}

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
