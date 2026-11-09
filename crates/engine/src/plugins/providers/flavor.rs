//! Minecraft flavors contributed by plugins. A plugin's `flavor` export is
//! wrapped in a [`PluginFlavorProvider`], which the Minecraft aggregate treats
//! like any built-in flavor. A flavor decides the code the game runs, so every
//! profile the guest returns is vetted here before the engine acts on it: a
//! profile with one refused artifact is unusable and is refused whole.
//!
//! What a profile names as built is produced by the guest's optional `build`,
//! which works in an area of its own; [`super::build`] places the outputs.

use std::borrow::Cow;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::{Context, Result};
use async_trait::async_trait;
use proto::download::HashAlgorithm;
use proto::minecraft::{Artifact, GameVersion, ProvisionPhase, ProvisionProgress};
use proto::plugin::{Grant, Manifest, PluginRunProgress, BUILD_MOUNT, VANILLA_MOUNT};
use proto::profile::ServerProfile;
use proto::profile::{InstanceProfile, Library, ServerJar, ServerLaunch};
use proto::provider::flavor;
use proto::provider::flavor::{
    self as op, BuildOutput, BuildQuery, Flavor, GameQuery, ResolveQuery,
};
use proto::provider::ProviderOp;
use proto::Empty;
use tokio::sync::watch;

use super::build::{self, Expected};
use super::{connect_each, registered, Guest};
use crate::cancel::Cancelled;
use crate::engine::Engine;
use crate::minecraft::materialize::{safe_join, validate_filename, OnProgress};
use crate::minecraft::{
    FlavorInfo, FlavorProvider, InstallRequest, ResolveRequest, Side, BOTH_SIDES, CLIENT_ONLY,
    REQUIRED_JAVA_MAJORS, SERVER_ONLY,
};
use crate::plugins::context::{CallContext, Mount};
use crate::plugins::http::download_allowed;
use crate::plugins::imports::CommandRun;
use crate::plugins::runtime::{LoadedPlugin, CANCEL_GRACE, COMMAND_TIMEOUT};

const PROGRESS_STEPS: u64 = 1000;

/// Why a guest's profile was refused.
#[derive(Debug, thiserror::Error)]
enum Refusal {
    #[error("`{0}` is not downloadable from the plugin's network hosts")]
    Host(String),
    #[error("artifact `{0}` has no well-formed sha1")]
    Checksum(String),
    #[error("{0}")]
    Path(String),
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
    if library.built {
        return Ok(());
    }
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
        ServerJar::Built { filename } => {
            validate_filename(filename).map_err(|e| Refusal::Path(e.to_string()))?
        }
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
    /// Cleared once the guest answers `build` with unsupported, so a flavor with
    /// nothing to build costs no guest call per launch.
    builds: AtomicBool,
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
            mrpack_key: described.mrpack_key.map(Cow::Owned),
            prism_uid: described.prism_uid.map(Cow::Owned),
        };
        Ok(PluginFlavorProvider {
            guest,
            info,
            builds: AtomicBool::new(true),
        })
    }

    /// A copy of the vanilla jar in a host-owned directory of its own, so the
    /// read-only mount shows that jar and nothing beside it: a server's jar
    /// sits in its data directory, next to `server.properties` and its RCON
    /// password.
    fn stage_vanilla(&self, meta: &Path, jar: &Path) -> Result<std::path::PathBuf> {
        let name = jar
            .file_name()
            .context("the vanilla jar has no file name")?;
        let staged = meta
            .join("flavors")
            .join(".vanilla")
            .join(self.guest.id())
            .join(name);
        build::copy_into(jar, &staged)
            .with_context(|| format!("cannot stage {}", jar.display()))?;
        Ok(staged)
    }

    /// Runs the guest's `build` in its area and returns what it produced; none
    /// when the guest has nothing to build.
    async fn build(
        &self,
        side: Side,
        request: &InstallRequest<'_>,
        on: OnProgress<'_>,
    ) -> Result<Vec<BuildOutput>> {
        let area = request.meta.join("flavors").join(self.guest.id());
        std::fs::create_dir_all(&area)
            .with_context(|| format!("cannot create {}", area.display()))?;
        let mut mounts = vec![Mount {
            host: area,
            guest: BUILD_MOUNT,
            writable: true,
        }];
        let vanilla_jar = match request.minecraft_jar.filter(|jar| jar.is_file()) {
            Some(jar) => {
                let staged = self.stage_vanilla(request.meta, jar)?;
                let name = staged
                    .file_name()
                    .and_then(|name| name.to_str())
                    .context("the vanilla jar has no file name")?
                    .to_string();
                mounts.push(Mount {
                    host: staged
                        .parent()
                        .context("the staged jar has no parent")?
                        .to_path_buf(),
                    guest: VANILLA_MOUNT,
                    writable: false,
                });
                Some(format!("{VANILLA_MOUNT}/{name}"))
            }
            None => None,
        };
        let query = BuildQuery {
            side: match side {
                Side::Server => flavor::BuildSide::Server,
                Side::Client => flavor::BuildSide::Client,
            },
            version: request.game_version.to_string(),
            loader_version: request.loader_version.map(str::to_string),
            vanilla_jar,
        };
        tracing::info!(
            flavor = %self.guest.id(),
            side = side.noun(),
            version = request.game_version,
            "building"
        );
        let (progress, mut reports) = watch::channel(None);
        let context = CallContext {
            run: Some(CommandRun {
                cancel: on.cancel().clone(),
                progress,
                grace: CANCEL_GRACE,
            }),
            mounts,
        };
        let call = self
            .guest
            .try_ask_within::<op::Build>(&query, COMMAND_TIMEOUT, context);
        tokio::pin!(call);
        let reply = loop {
            tokio::select! {
                reply = &mut call => break reply,
                Ok(()) = reports.changed() => {
                    let latest = reports.borrow_and_update().clone();
                    if let Some(latest) = latest {
                        on.report(&provision_progress(side, &latest));
                    }
                }
            }
        };
        match reply {
            Ok(Some(done)) => Ok(done.outputs),
            Ok(None) => {
                self.builds.store(false, Ordering::Relaxed);
                Ok(Vec::new())
            }
            Err(_) if on.is_cancelled() => Err(Cancelled.into()),
            Err(error) => Err(error),
        }
    }

    /// Copies the outputs to where the profile named them and fails unless every
    /// built file exists afterwards.
    async fn place(
        &self,
        side: Side,
        request: &InstallRequest<'_>,
        outputs: Vec<BuildOutput>,
    ) -> Result<()> {
        if request.built.is_empty() && outputs.is_empty() {
            return Ok(());
        }
        let libraries = request.meta.join("libraries");
        let expected = request
            .built
            .iter()
            .map(|target| {
                let destination = match side {
                    Side::Server => {
                        validate_filename(target)?;
                        safe_join(request.root, target)?
                    }
                    Side::Client => safe_join(&libraries, target)?,
                };
                Ok(Expected {
                    target: target.clone(),
                    destination,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let area = request.meta.join("flavors").join(self.guest.id());
        tokio::task::spawn_blocking(move || build::place(&area, &outputs, &expected))
            .await?
            .map_err(|fault| {
                tracing::warn!(flavor = %self.guest.id(), %fault, "build outputs refused");
                self.guest.failed(format!("build: {fault}"))
            })
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

    async fn install(
        &self,
        side: Side,
        request: &InstallRequest<'_>,
        on: OnProgress<'_>,
    ) -> Result<()> {
        let outputs = match self.builds.load(Ordering::Relaxed) {
            true => self.build(side, request, on).await?,
            false => Vec::new(),
        };
        self.place(side, request, outputs).await
    }
}

fn provision_progress(side: Side, reported: &PluginRunProgress) -> ProvisionProgress {
    ProvisionProgress {
        phase: match side {
            Side::Client => ProvisionPhase::Libraries,
            Side::Server => ProvisionPhase::Server,
        },
        current: reported
            .fraction
            .map_or(0, |fraction| (fraction * PROGRESS_STEPS as f32) as u64),
        total: reported.fraction.map_or(0, |_| PROGRESS_STEPS),
        detail: reported.message.clone(),
        ..ProvisionProgress::default()
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
