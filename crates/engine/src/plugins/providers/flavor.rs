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
use proto::plugin::{Grant, Manifest, FLAVOR_PROVIDER};
use proto::profile::ServerProfile;
use proto::profile::{FlavorDescription, InstanceProfile, Library, ServerJar, ServerLaunch};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};

use super::call_failed;
use crate::minecraft::materialize::{safe_join, validate_filename};
use crate::minecraft::{
    FlavorInfo, FlavorProvider, Minecraft, ResolveRequest, BOTH_SIDES, CLIENT_ONLY,
    REQUIRED_JAVA_MAJORS, SERVER_ONLY,
};
use crate::plugins::chain::CallChain;
use crate::plugins::http::download_allowed;
use crate::plugins::runtime::{LoadedPlugin, PROVIDER_TIMEOUT};

/// One function of the `flavor` export.
#[derive(Debug, Clone, Copy)]
pub(in crate::plugins) enum FlavorOp {
    Info,
    Versions,
    LoaderVersions,
    ResolveServer,
    ResolveClient,
}

impl FlavorOp {
    fn name(self) -> &'static str {
        match self {
            FlavorOp::Info => "info",
            FlavorOp::Versions => "versions",
            FlavorOp::LoaderVersions => "loader-versions",
            FlavorOp::ResolveServer => "resolve-server",
            FlavorOp::ResolveClient => "resolve-client",
        }
    }
}

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
    local: String,
    plugin: Arc<LoadedPlugin>,
    info: FlavorInfo,
}

impl PluginFlavorProvider {
    /// Asks the guest to describe the flavor `local` of its manifest.
    pub async fn connect(plugin: Arc<LoadedPlugin>, local: &str) -> Result<Self> {
        let mut provider = PluginFlavorProvider {
            local: local.to_string(),
            info: FlavorInfo {
                id: Cow::Owned(format!("{}.{local}", plugin.id())),
                name: Cow::Borrowed(""),
                summary: Cow::Borrowed(""),
                loads: None,
                requires: &[],
                sides: BOTH_SIDES,
            },
            plugin,
        };
        let described: FlavorDescription = provider.ask(FlavorOp::Info, Value::Null).await?;
        provider.info.sides = match (described.server, described.client) {
            (true, true) => BOTH_SIDES,
            (true, false) => SERVER_ONLY,
            (false, true) => CLIENT_ONLY,
            (false, false) => {
                return Err(provider.failed("the flavor serves neither side".into()));
            }
        };
        provider.info.name = Cow::Owned(described.name);
        provider.info.summary = Cow::Owned(described.summary);
        provider.info.loads = described.loads;
        Ok(provider)
    }

    async fn ask<T: DeserializeOwned>(&self, op: FlavorOp, args: Value) -> Result<T> {
        let reply = self
            .plugin
            .flavor(
                &CallChain::default(),
                op,
                &self.local,
                &args.to_string(),
                PROVIDER_TIMEOUT,
            )
            .await
            .map_err(|error| self.failed(format!("{}: {error}", op.name())))?;
        serde_json::from_str(&reply).map_err(|error| {
            tracing::warn!(plugin = self.plugin.id(), flavor = %self.local, op = op.name(), %error, "malformed flavor reply");
            self.failed(format!("{} returned an unreadable reply", op.name()))
        })
    }

    fn failed(&self, detail: String) -> anyhow::Error {
        call_failed(self.plugin.id(), detail)
    }

    fn refused(&self, op: FlavorOp, refusal: Refusal) -> anyhow::Error {
        tracing::warn!(plugin = self.plugin.id(), flavor = %self.local, op = op.name(), %refusal, "flavor profile refused");
        self.failed(format!(
            "{} returned a refused profile: {refusal}",
            op.name()
        ))
    }

    fn resolve_args(request: &ResolveRequest<'_>) -> Value {
        json!({ "version": request.version, "loaderVersion": request.loader_version })
    }
}

#[async_trait]
impl FlavorProvider for PluginFlavorProvider {
    fn info(&self) -> &FlavorInfo {
        &self.info
    }

    async fn versions(&self) -> Result<Vec<GameVersion>> {
        self.ask(FlavorOp::Versions, Value::Null).await
    }

    async fn loader_versions(&self, game: &str) -> Result<Vec<String>> {
        self.ask(FlavorOp::LoaderVersions, json!({ "game": game }))
            .await
    }

    async fn resolve_server(&self, request: &ResolveRequest<'_>) -> Result<ServerProfile> {
        let mut profile: ServerProfile = self
            .ask(FlavorOp::ResolveServer, Self::resolve_args(request))
            .await?;
        vet_server(self.plugin.grant(), &profile)
            .map_err(|refusal| self.refused(FlavorOp::ResolveServer, refusal))?;
        profile.flavor = self.info.id.to_string();
        profile.game_version = request.version.clone();
        Ok(profile)
    }

    async fn resolve_client(&self, request: &ResolveRequest<'_>) -> Result<InstanceProfile> {
        let mut profile: InstanceProfile = self
            .ask(FlavorOp::ResolveClient, Self::resolve_args(request))
            .await?;
        vet_client(self.plugin.grant(), &profile)
            .map_err(|refusal| self.refused(FlavorOp::ResolveClient, refusal))?;
        profile.flavor = self.info.id.to_string();
        profile.game_version = request.version.clone();
        Ok(profile)
    }
}

/// Connects every flavor the manifest contributes. One that fails to describe
/// itself is logged and left out; the plugin stays enabled.
pub async fn connect_flavors(
    plugin: &Arc<LoadedPlugin>,
    manifest: &Manifest,
) -> Vec<PluginFlavorProvider> {
    let mut flavors = Vec::new();
    for contribution in &manifest.contributes.providers {
        if contribution.kind != FLAVOR_PROVIDER {
            continue;
        }
        match PluginFlavorProvider::connect(plugin.clone(), &contribution.id).await {
            Ok(flavor) => flavors.push(flavor),
            Err(error) => tracing::warn!(
                plugin = plugin.id(),
                flavor = %contribution.id,
                error = %format!("{error:#}"),
                "flavor not registered"
            ),
        }
    }
    flavors
}

/// Adds `flavors` to `minecraft` under `owner`. `official` is whether the
/// plugin's key is the official one; see [`Roster::register`](crate::roster).
pub fn register_flavors(
    minecraft: &Minecraft,
    owner: &str,
    official: bool,
    flavors: Vec<PluginFlavorProvider>,
) {
    for flavor in flavors {
        let id = flavor.info.id.to_string();
        match minecraft.register(owner, official, Arc::new(flavor)) {
            Ok(()) => tracing::info!(plugin = owner, flavor = %id, "flavor registered"),
            Err(error) => {
                tracing::warn!(plugin = owner, flavor = %id, %error, "flavor refused")
            }
        }
    }
}
