//! Fabric — the loader profile layered over a base game version. The server is a
//! self-contained launcher jar; the client merges Fabric's libraries and main
//! class over the vanilla profile.

use std::borrow::Cow;

use anyhow::Result;
use async_trait::async_trait;
use proto::content::ContentKind;
use proto::minecraft::{Artifact, GameVersion, VersionKind};

use super::meta::{fabric, mojang};
use super::overlay;
use super::profile::{InstanceProfile, ServerJar, ServerLaunch, ServerProfile};
use super::provider::{FlavorInfo, FlavorProvider, ResolveRequest, BOTH_SIDES};

const ID: &str = "fabric";

const INFO: FlavorInfo = FlavorInfo {
    id: Cow::Borrowed(ID),
    name: Cow::Borrowed("Fabric"),
    summary: Cow::Borrowed(
        "A light, fast-moving mod loader. Usually the first to support a new Minecraft version.",
    ),
    loads: Some(ContentKind::Mod),
    requires: &[],
    sides: BOTH_SIDES,
    mrpack_key: Some(Cow::Borrowed("fabric-loader")),
    prism_uid: Some(Cow::Borrowed("net.fabricmc.fabric-loader")),
};

async fn game_versions() -> Result<Vec<GameVersion>> {
    Ok(fabric::game_versions()
        .await?
        .into_iter()
        .map(|(id, stable)| GameVersion {
            id,
            kind: if stable {
                VersionKind::Release
            } else {
                VersionKind::Snapshot
            },
            stable,
        })
        .collect())
}

async fn resolve_loader(request: &ResolveRequest<'_>) -> Result<String> {
    match &request.loader_version {
        Some(loader) => Ok(loader.clone()),
        None => fabric::latest_loader(&request.version).await,
    }
}

pub struct Fabric;

#[async_trait]
impl FlavorProvider for Fabric {
    fn info(&self) -> &FlavorInfo {
        &INFO
    }

    async fn versions(&self) -> Result<Vec<GameVersion>> {
        game_versions().await
    }

    async fn loader_versions(&self, game: &str) -> Result<Vec<String>> {
        fabric::loader_versions(game).await
    }

    async fn resolve_server(&self, request: &ResolveRequest<'_>) -> Result<ServerProfile> {
        let loader = resolve_loader(request).await?;
        let installer = fabric::latest_installer().await?;
        let base = mojang::version_json(&request.version).await?;
        let url = fabric::server_launcher_url(&request.version, &loader, &installer);
        Ok(ServerProfile {
            flavor: ID.to_string(),
            game_version: request.version.clone(),
            loader_version: Some(loader.clone()),
            jar: ServerJar::Download(Artifact {
                filename: format!(
                    "fabric-server-mc.{}-loader.{}-launcher.{}.jar",
                    request.version, loader, installer
                ),
                url,
                size: 0,
                checksum: None,
                unhashed: false,
            }),
            launch: ServerLaunch::Jar,
            java_major: mojang::java_major(&base),
            jvm_args: Vec::new(),
        })
    }

    async fn resolve_client(&self, request: &ResolveRequest<'_>) -> Result<InstanceProfile> {
        let loader = resolve_loader(request).await?;
        let base = mojang::version_json(&request.version).await?;
        let profile = fabric::profile_json(&request.version, &loader).await?;

        let libraries =
            overlay::merge_libraries(mojang::libraries(&base), fabric::libraries(&profile));

        let mut jvm_args = mojang::jvm_args(&base);
        jvm_args.extend(fabric::jvm_args(&profile));

        Ok(InstanceProfile {
            flavor: ID.to_string(),
            game_version: request.version.clone(),
            loader_version: Some(loader),
            client: mojang::client_artifact(&base)?,
            libraries,
            natives: mojang::natives(&base),
            asset_index: mojang::asset_index(&base)?,
            java_major: mojang::java_major(&base),
            main_class: fabric::client_main_class(&profile),
            jvm_args,
            game_args: mojang::game_args(&base),
        })
    }
}
