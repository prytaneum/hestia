//! Minecraft flavors contributed by plugins. A plugin's `flavor` export is
//! wrapped in a [`PluginFlavorProvider`], which the Minecraft aggregate treats
//! like any built-in flavor. A flavor decides the code the game runs, so every
//! profile the guest returns is vetted here before the engine acts on it: a
//! profile with one refused artifact is unusable and is refused whole.

use std::borrow::Cow;
use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use proto::download::HashAlgorithm;
use proto::minecraft::{Artifact, GameVersion};
use proto::plugin::{Grant, Manifest};
use proto::profile::ServerProfile;
use proto::profile::{InstanceProfile, Library, ServerJar, ServerLaunch};
use proto::provider::flavor::{self as op, Flavor, GameQuery, ResolveQuery};
use proto::provider::ProviderOp;
use proto::Empty;

use super::{connect_each, registered, Guest};
use crate::engine::Engine;
use crate::minecraft::materialize::{safe_join, validate_filename};
use crate::minecraft::{
    FlavorInfo, FlavorProvider, ResolveRequest, BOTH_SIDES, CLIENT_ONLY, REQUIRED_JAVA_MAJORS,
    SERVER_ONLY,
};
use crate::plugins::http::download_allowed;
use crate::plugins::runtime::LoadedPlugin;

/// Why a guest's profile was refused.
#[derive(Debug, thiserror::Error)]
enum Refusal {
    #[error("`{0}` is not downloadable from the plugin's network hosts")]
    Host(String),
    #[error("artifact `{0}` has no well-formed sha1")]
    Checksum(String),
    #[error("{0}")]
    Path(String),
    #[error("the plugin cannot build a server; it must name a downloadable jar")]
    Built,
    #[error("the plugin cannot launch from an args file")]
    ArgsFile,
    #[error("Java {0} is not a major this launcher knows")]
    Java(i32),
    #[error("the profile names no main class")]
    MainClass,
}

fn artifact(grant: &Grant, artifact: &Artifact) -> Result<(), Refusal> {
    validate_filename(&artifact.filename).map_err(|e| Refusal::Path(e.to_string()))?;
    if !download_allowed(grant, &artifact.url) {
        return Err(Refusal::Host(artifact.filename.clone()));
    }
    let sha1 = artifact
        .checksum
        .as_ref()
        .is_some_and(|sum| sum.algorithm == HashAlgorithm::Sha1 && sum.is_valid());
    sha1.then_some(())
        .ok_or_else(|| Refusal::Checksum(artifact.filename.clone()))
}

fn library(grant: &Grant, library: &Library) -> Result<(), Refusal> {
    safe_join(Path::new(""), &library.path).map_err(|e| Refusal::Path(e.to_string()))?;
    artifact(grant, &library.artifact)
}

fn java(major: i32) -> Result<(), Refusal> {
    REQUIRED_JAVA_MAJORS
        .contains(&major)
        .then_some(())
        .ok_or(Refusal::Java(major))
}

fn vet_server(grant: &Grant, profile: &ServerProfile) -> Result<(), Refusal> {
    match &profile.jar {
        ServerJar::Download(jar) => artifact(grant, jar)?,
        ServerJar::Built { .. } => return Err(Refusal::Built),
    }
    if !matches!(profile.launch, ServerLaunch::Jar) {
        return Err(Refusal::ArgsFile);
    }
    java(profile.java_major)
}

fn vet_client(grant: &Grant, profile: &InstanceProfile) -> Result<(), Refusal> {
    artifact(grant, &profile.client)?;
    for lib in &profile.libraries {
        library(grant, lib)?;
    }
    for native in &profile.natives {
        library(grant, &native.library)?;
    }
    validate_filename(&profile.asset_index.id).map_err(|e| Refusal::Path(e.to_string()))?;
    artifact(grant, &profile.asset_index.artifact)?;
    if profile.main_class.is_empty() {
        return Err(Refusal::MainClass);
    }
    java(profile.java_major)
}

/// A flavor a plugin contributes, registered as `<plugin>.<id>`.
pub struct PluginFlavorProvider {
    guest: Guest,
    info: FlavorInfo,
}

impl PluginFlavorProvider {
    /// Asks the guest to describe its flavor.
    async fn connect(guest: Guest) -> Result<Self> {
        let described = guest.ask::<op::Info>(&Empty {}).await?;
        let sides = match (described.server, described.client) {
            (true, true) => BOTH_SIDES,
            (true, false) => SERVER_ONLY,
            (false, true) => CLIENT_ONLY,
            (false, false) => {
                return Err(guest.failed("the flavor serves neither side".into()));
            }
        };
        let info = FlavorInfo {
            id: Cow::Owned(guest.id().to_string()),
            name: Cow::Owned(described.name),
            summary: Cow::Owned(described.summary),
            loads: described.loads,
            requires: &[],
            sides,
        };
        Ok(PluginFlavorProvider { guest, info })
    }

    fn refused(&self, op: &str, refusal: Refusal) -> anyhow::Error {
        tracing::warn!(flavor = %self.guest.id(), op, %refusal, "flavor profile refused");
        self.guest
            .failed(format!("{op} returned a refused profile: {refusal}"))
    }

    fn resolve_query(request: &ResolveRequest<'_>) -> ResolveQuery {
        ResolveQuery {
            version: request.version.clone(),
            loader_version: request.loader_version.clone(),
        }
    }
}

#[async_trait]
impl FlavorProvider for PluginFlavorProvider {
    fn info(&self) -> &FlavorInfo {
        &self.info
    }

    async fn versions(&self) -> Result<Vec<GameVersion>> {
        self.guest.ask::<op::Versions>(&Empty {}).await
    }

    /// A guest without `loader-versions` has no loader builds.
    async fn loader_versions(&self, game: &str) -> Result<Vec<String>> {
        let query = GameQuery { game: game.into() };
        Ok(self
            .guest
            .try_ask::<op::LoaderVersions>(&query)
            .await?
            .unwrap_or_default())
    }

    async fn resolve_server(&self, request: &ResolveRequest<'_>) -> Result<ServerProfile> {
        let mut profile = self
            .guest
            .ask::<op::ResolveServer>(&Self::resolve_query(request))
            .await?;
        vet_server(self.guest.plugin().grant(), &profile)
            .map_err(|refusal| self.refused(op::ResolveServer::OP, refusal))?;
        profile.flavor = self.info.id.to_string();
        profile.game_version = request.version.clone();
        Ok(profile)
    }

    async fn resolve_client(&self, request: &ResolveRequest<'_>) -> Result<InstanceProfile> {
        let mut profile = self
            .guest
            .ask::<op::ResolveClient>(&Self::resolve_query(request))
            .await?;
        vet_client(self.guest.plugin().grant(), &profile)
            .map_err(|refusal| self.refused(op::ResolveClient::OP, refusal))?;
        profile.flavor = self.info.id.to_string();
        profile.game_version = request.version.clone();
        Ok(profile)
    }
}

pub(super) async fn connect(
    plugin: &Arc<LoadedPlugin>,
    manifest: &Manifest,
) -> Vec<PluginFlavorProvider> {
    connect_each::<Flavor, _, _, _>(plugin, manifest, PluginFlavorProvider::connect).await
}

pub(super) fn register(
    engine: &Engine,
    owner: &str,
    official: bool,
    flavors: Vec<PluginFlavorProvider>,
) {
    for flavor in flavors {
        let id = flavor.info.id.to_string();
        let outcome = engine
            .minecraft()
            .register(owner, official, Arc::new(flavor));
        registered(owner, "flavor", &id, outcome);
    }
}
