//! Minecraft flavors contributed by plugins. A plugin's `flavor` export is
//! wrapped in a [`PluginFlavorProvider`], which the Minecraft aggregate treats
//! like any built-in flavor. A flavor decides the code the game runs, so every
//! profile the guest returns is vetted here before the engine acts on it: a
//! profile with one refused artifact is unusable and is refused whole.
//!
//! What a profile names as built is produced by the guest's optional `build`,
//! which works in an area of its own; [`super::build`] places the outputs.

use std::borrow::Cow;
use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::{Context, Result};
use async_trait::async_trait;
use proto::minecraft::{Artifact, GameVersion, ProvisionPhase};
use proto::plugin::{Manifest, BUILD_MOUNT, VANILLA_MOUNT};
use proto::profile::{FlavorDescription, ServerProfile};
use proto::profile::{InstanceProfile, Library, ServerJar, ServerLaunch};
use proto::provider::flavor;
use proto::provider::flavor::{
    self as op, BuildOutput, BuildQuery, ClientResolution, Flavor, GameQuery, ResolveQuery,
    ServerResolution, Target, VersionList,
};
use proto::provider::ProviderOp;
use proto::Empty;
use tokio::sync::watch;

use super::build::{self, Expected, Roots};
use super::normalise::{Failure, Normaliser};
use super::plan;
use super::{connect_each, registered, Guest};
use crate::cancel::Cancelled;
use crate::engine::Engine;
use crate::minecraft::materialize::{safe_join, validate_filename, OnProgress};
use crate::minecraft::overlay::{self, ClientLayer, ServerLayer};
use crate::minecraft::rules::host_target;
use crate::minecraft::{
    serves_vanilla, FlavorInfo, FlavorProvider, InstallRequest, ResolveRequest, Side, BOTH_SIDES,
    CLIENT_ONLY, REQUIRED_JAVA_MAJORS, SERVER_ONLY,
};
use crate::plugins::context::{CallContext, Mount};
use crate::plugins::imports::CommandRun;
use crate::plugins::prerequisites;
use crate::plugins::runtime::{LoadedPlugin, CANCEL_GRACE, COMMAND_TIMEOUT};
use crate::plugins::vouch::{Policy, Refused};

const MAX_VERSIONS: usize = 4096;
const MAX_VERSION_ID_CHARS: usize = 64;
const MAX_NAME_CHARS: usize = 64;
const MAX_COMPATIBLE: usize = 8;
const MAX_INTERCHANGE: usize = 16;
const MAX_INTERCHANGE_KEY_CHARS: usize = 128;

/// A loader name or a format id: lowercase letters, digits and `-_.`, as a
/// content platform and a plugin id spell them.
fn is_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME_CHARS
        && name.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'_' | b'.')
        })
}

fn is_version_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= MAX_VERSION_ID_CHARS && !id.chars().any(char::is_control)
}

/// What a flavor declares about its loader, vetted: the content-platform name,
/// the further loaders it runs, and its key in each interchange format.
struct Declared {
    loader: Option<String>,
    compatible: Vec<String>,
    interchange: Vec<(String, String)>,
}

fn declared(description: &FlavorDescription) -> Result<Declared, String> {
    if let Some(loader) = description.loader.as_deref().filter(|l| !is_name(l)) {
        return Err(format!("`{loader}` is not a usable loader name"));
    }
    if description.compatible.len() > MAX_COMPATIBLE {
        return Err(format!(
            "it names more than {MAX_COMPATIBLE} compatible loaders"
        ));
    }
    if let Some(bad) = description.compatible.iter().find(|l| !is_name(l)) {
        return Err(format!("`{bad}` is not a usable loader name"));
    }
    if description.interchange.len() > MAX_INTERCHANGE {
        return Err(format!(
            "it names more than {MAX_INTERCHANGE} interchange keys"
        ));
    }
    for (format, key) in &description.interchange {
        if !is_name(format) {
            return Err(format!("`{format}` is not a usable format id"));
        }
        if key.is_empty()
            || key.len() > MAX_INTERCHANGE_KEY_CHARS
            || key.chars().any(char::is_control)
        {
            return Err(format!("the `{format}` interchange key is not usable"));
        }
    }
    Ok(Declared {
        loader: description.loader.clone(),
        compatible: description.compatible.clone(),
        interchange: description
            .interchange
            .iter()
            .map(|(format, key)| (format.clone(), key.clone()))
            .collect(),
    })
}

/// Why a guest's profile was refused.
#[derive(Debug, thiserror::Error)]
enum Refusal {
    #[error(transparent)]
    Download(#[from] Refused),
    #[error("{0}")]
    Path(String),
    #[error("Java {0} is not a major this launcher knows")]
    Java(i32),
    #[error("the profile names no main class")]
    MainClass,
    #[error("the flavor builds on vanilla, which this launcher does not offer")]
    NoVanilla,
    #[error("{0}")]
    Declared(String),
}

fn artifact(policy: &Policy, artifact: &Artifact) -> Result<(), Refusal> {
    validate_filename(&artifact.filename).map_err(|e| Refusal::Path(e.to_string()))?;
    Ok(policy.vet_artifact(artifact)?)
}

fn library(policy: &Policy, library: &Library) -> Result<(), Refusal> {
    safe_join(Path::new(""), &library.path).map_err(|e| Refusal::Path(e.to_string()))?;
    if library.built {
        return Ok(());
    }
    artifact(policy, &library.artifact)
}

fn java(major: i32) -> Result<(), Refusal> {
    REQUIRED_JAVA_MAJORS
        .contains(&major)
        .then_some(())
        .ok_or(Refusal::Java(major))
}

fn vet_server(policy: &Policy, profile: &ServerProfile) -> Result<(), Refusal> {
    match &profile.jar {
        ServerJar::Download(jar) => artifact(policy, jar)?,
        ServerJar::Built { filename } => {
            validate_filename(filename).map_err(|e| Refusal::Path(e.to_string()))?
        }
    }
    launch(&profile.launch)?;
    java(profile.java_major)
}

fn launch(launch: &ServerLaunch) -> Result<(), Refusal> {
    if let ServerLaunch::ArgsFile(file) = launch {
        for path in file.paths() {
            safe_join(Path::new(""), path).map_err(|e| Refusal::Path(e.to_string()))?;
        }
    }
    Ok(())
}

fn vet_server_addition(policy: &Policy, addition: &ServerLayer) -> Result<(), Refusal> {
    addition.java_major.map(java).transpose()?;
    addition.launch.as_ref().map(launch).transpose()?;
    match &addition.jar {
        Some(ServerJar::Download(jar)) => artifact(policy, jar),
        Some(ServerJar::Built { filename }) => {
            validate_filename(filename).map_err(|e| Refusal::Path(e.to_string()))
        }
        None => Ok(()),
    }
}

fn vet_client_addition(policy: &Policy, addition: &ClientLayer) -> Result<(), Refusal> {
    addition.java_major.map(java).transpose()?;
    addition
        .libraries
        .iter()
        .try_for_each(|lib| library(policy, lib))
}

fn vet_client(policy: &Policy, profile: &InstanceProfile) -> Result<(), Refusal> {
    artifact(policy, &profile.client)?;
    for lib in &profile.libraries {
        library(policy, lib)?;
    }
    for native in &profile.natives {
        library(policy, &native.library)?;
    }
    validate_filename(&profile.asset_index.id).map_err(|e| Refusal::Path(e.to_string()))?;
    artifact(policy, &profile.asset_index.artifact)?;
    if profile.main_class.is_empty() {
        return Err(Refusal::MainClass);
    }
    java(profile.java_major)
}

/// A flavor a plugin contributes, registered as `<plugin>.<id>`, or bare when official.
pub struct PluginFlavorProvider {
    guest: Guest,
    info: FlavorInfo,
    /// Cleared once the guest answers `build` with unsupported, so a flavor with
    /// nothing to build costs no guest call per launch.
    builds: AtomicBool,
    /// The flavor a profile that builds on vanilla is layered over; set when the
    /// provider is registered.
    vanilla: Option<Arc<dyn FlavorProvider>>,
    policy: Arc<Policy>,
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
        let requires = prerequisites(guest.plugin().grant(), &described.requires)
            .map_err(|detail| guest.failed(detail))?;
        let declared = declared(&described).map_err(|detail| guest.failed(detail))?;
        let info = FlavorInfo {
            id: Cow::Owned(guest.id().to_string()),
            name: Cow::Owned(described.name),
            summary: Cow::Owned(described.summary),
            loads: described.loads,
            requires: Cow::Owned(requires),
            sides,
            loader: declared.loader.map(Cow::Owned),
            compatible: Cow::Owned(declared.compatible.into_iter().map(Cow::Owned).collect()),
            interchange: Cow::Owned(
                declared
                    .interchange
                    .into_iter()
                    .map(|(format, key)| (Cow::Owned(format), Cow::Owned(key)))
                    .collect(),
            ),
        };
        let policy = Arc::new(Policy::new(guest.plugin().grant()));
        Ok(PluginFlavorProvider {
            guest,
            info,
            builds: AtomicBool::new(true),
            vanilla: None,
            policy,
        })
    }

    /// The directory this plugin builds in, shared by every build of every
    /// flavor it contributes.
    fn area(&self, meta: &Path) -> std::path::PathBuf {
        meta.join("flavors").join(self.guest.plugin().id())
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
            .join(self.guest.plugin().id())
            .join(name);
        build::copy_into(jar, &staged)
            .with_context(|| format!("cannot stage {}", jar.display()))?;
        Ok(staged)
    }

    /// Asks the guest for its `build` plan, vets it, and runs it in the area:
    /// the host's downloads, then its steps under the supervisor. Returns what
    /// the build produced for the host to place; none when the guest has nothing
    /// to build.
    async fn build(
        &self,
        side: Side,
        request: &InstallRequest<'_>,
        on: OnProgress<'_>,
    ) -> Result<Vec<BuildOutput>> {
        let area = self.area(request.meta);
        let mut mounts = vec![Mount {
            host: area.clone(),
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
            target: host_target(Some(request.java_major)),
        };
        tracing::info!(
            flavor = %self.guest.id(),
            side = side.noun(),
            version = request.game_version,
            "building"
        );
        let phase = phase_of(side);
        let game_java = u32::try_from(request.java_major).ok();
        let (progress, reports) = watch::channel(None);
        let context = CallContext {
            run: Some(CommandRun {
                cancel: on.cancel().clone(),
                progress,
                grace: CANCEL_GRACE,
            }),
            mounts: mounts.clone(),
            game_java,
        };
        let call = self
            .guest
            .try_ask_within::<op::Build>(&query, COMMAND_TIMEOUT, context);
        let plan = match plan::relay(reports, call, on, phase).await {
            Ok(Some(plan)) => plan,
            Ok(None) => {
                self.builds.store(false, Ordering::Relaxed);
                return Ok(Vec::new());
            }
            Err(_) if on.is_cancelled() => return Err(Cancelled.into()),
            Err(error) => return Err(error),
        };
        plan::vet(self.guest.plugin(), &plan, &mounts, game_java).map_err(|fault| {
            tracing::warn!(flavor = %self.guest.id(), %fault, "build plan refused");
            self.guest.failed(format!("build plan refused: {fault}"))
        })?;
        plan::execute(
            &plan::Run {
                guest: &self.guest,
                area: &area,
                mounts: &mounts,
                game_java,
                version: request.game_version,
                phase,
                on,
            },
            &plan,
        )
        .await?;
        Ok(plan.outputs)
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
        let area = self.area(request.meta);
        let roots = Roots {
            entry: matches!(side, Side::Server).then(|| request.root.to_path_buf()),
            libraries,
        };
        tokio::task::spawn_blocking(move || build::place(&area, &outputs, &expected, &roots))
            .await?
            .map_err(|fault| {
                tracing::warn!(flavor = %self.guest.id(), %fault, "build outputs refused");
                self.guest.failed(format!("build: {fault}"))
            })
    }

    fn bounded(&self, count: usize) -> Result<()> {
        match count > MAX_VERSIONS {
            true => Err(self
                .guest
                .failed(format!("it lists more than {MAX_VERSIONS} versions"))),
            false => Ok(()),
        }
    }

    fn refused(&self, op: &str, refusal: Refusal) -> anyhow::Error {
        tracing::warn!(flavor = %self.guest.id(), op, %refusal, "flavor profile refused");
        self.guest
            .failed(format!("{op} returned a refused profile: {refusal}"))
    }

    fn vanilla(&self, op: &str) -> Result<&Arc<dyn FlavorProvider>> {
        self.vanilla
            .as_ref()
            .ok_or_else(|| self.refused(op, Refusal::NoVanilla))
    }

    fn resolve_query(request: &ResolveRequest<'_>, target: &Target) -> ResolveQuery {
        ResolveQuery {
            version: request.version.clone(),
            loader_version: request.loader_version.clone(),
            target: target.clone(),
        }
    }
}

#[async_trait]
impl FlavorProvider for PluginFlavorProvider {
    fn info(&self) -> &FlavorInfo {
        &self.info
    }

    async fn versions(&self) -> Result<Vec<GameVersion>> {
        match self.guest.ask::<op::Versions>(&Empty {}).await? {
            VersionList::Listed(versions) => {
                self.bounded(versions.len())?;
                match versions.iter().find(|v| !is_version_id(&v.id)) {
                    Some(bad) => Err(self
                        .guest
                        .failed(format!("`{}` is not a version id", bad.id))),
                    None => Ok(versions),
                }
            }
            VersionList::Ids(ids) => {
                self.bounded(ids.len())?;
                if let Some(bad) = ids.iter().find(|id| !is_version_id(id)) {
                    return Err(self.guest.failed(format!("`{bad}` is not a version id")));
                }
                let wanted: HashSet<&str> = ids.iter().map(String::as_str).collect();
                let typed: Vec<GameVersion> = self
                    .vanilla(op::Versions::OP)?
                    .versions()
                    .await?
                    .into_iter()
                    .filter(|version| wanted.contains(version.id.as_str()))
                    .collect();
                let dropped = wanted.len() - typed.len();
                if dropped > 0 {
                    tracing::warn!(
                        flavor = %self.guest.id(),
                        dropped,
                        "versions the vanilla catalogue does not list were dropped"
                    );
                }
                Ok(typed)
            }
        }
    }

    /// A guest without `loader-versions` has no loader builds.
    async fn loader_versions(&self, game: &str) -> Result<Vec<String>> {
        let query = GameQuery {
            game: game.into(),
            target: host_target(None),
        };
        Ok(self
            .guest
            .try_ask::<op::LoaderVersions>(&query)
            .await?
            .unwrap_or_default())
    }

    async fn resolve_server(&self, request: &ResolveRequest<'_>) -> Result<ServerProfile> {
        let refused = |refusal| self.refused(op::ResolveServer::OP, refusal);
        let target = host_target(None);
        let mut profile = match self
            .guest
            .ask::<op::ResolveServer>(&Self::resolve_query(request, &target))
            .await?
        {
            ServerResolution::Full(profile) => {
                vet_server(&self.policy, &profile).map_err(refused)?;
                *profile
            }
            ServerResolution::OnVanilla(addition) => {
                let addition = Normaliser {
                    policy: &self.policy,
                    target: &target,
                }
                .server(addition);
                vet_server_addition(&self.policy, &addition).map_err(refused)?;
                let base = self
                    .vanilla(op::ResolveServer::OP)?
                    .resolve_server(request)
                    .await?;
                overlay::server(base, addition)
            }
        };
        profile.flavor = self.info.id.to_string();
        profile.game_version = request.version.clone();
        Ok(profile)
    }

    async fn resolve_client(&self, request: &ResolveRequest<'_>) -> Result<InstanceProfile> {
        let refused = |refusal| self.refused(op::ResolveClient::OP, refusal);
        let target = host_target(None);
        let mut profile = match self
            .guest
            .ask::<op::ResolveClient>(&Self::resolve_query(request, &target))
            .await?
        {
            ClientResolution::Full(profile) => {
                vet_client(&self.policy, &profile).map_err(refused)?;
                *profile
            }
            ClientResolution::OnVanilla(addition) => {
                let normaliser = Normaliser {
                    policy: &self.policy,
                    target: &target,
                };
                let addition = match normaliser.client(addition).await {
                    Ok(addition) => addition,
                    Err(Failure::Unresolved(why)) => {
                        return Err(refused(Refusal::Declared(why.to_string())))
                    }
                    Err(Failure::Other(error)) => return Err(error),
                };
                vet_client_addition(&self.policy, &addition).map_err(refused)?;
                let base = self
                    .vanilla(op::ResolveClient::OP)?
                    .resolve_client(request)
                    .await?;
                overlay::client(base, addition)
            }
        };
        profile.flavor = self.info.id.to_string();
        profile.game_version = request.version.clone();
        Ok(profile)
    }

    fn download_policy(&self, url: &str) -> Option<Arc<Policy>> {
        (!serves_vanilla(url)).then(|| self.policy.clone())
    }

    async fn install(
        &self,
        side: Side,
        request: &InstallRequest<'_>,
        on: OnProgress<'_>,
    ) -> Result<()> {
        if !self.builds.load(Ordering::Relaxed) {
            return self.place(side, request, Vec::new()).await;
        }
        let _held = build::lock_area(&self.area(request.meta), on.cancel()).await?;
        let outputs = self.build(side, request, on).await?;
        self.place(side, request, outputs).await
    }
}

fn phase_of(side: Side) -> ProvisionPhase {
    match side {
        Side::Client => ProvisionPhase::Libraries,
        Side::Server => ProvisionPhase::Server,
    }
}

pub(super) async fn connect(
    plugin: &Arc<LoadedPlugin>,
    manifest: &Manifest,
    official: bool,
) -> Vec<PluginFlavorProvider> {
    connect_each::<Flavor, _, _, _>(plugin, manifest, official, PluginFlavorProvider::connect).await
}

pub(super) fn register(
    engine: &Engine,
    owner: &str,
    official: bool,
    flavors: Vec<PluginFlavorProvider>,
) {
    for mut flavor in flavors {
        flavor.vanilla = engine.minecraft().vanilla();
        let id = flavor.info.id.to_string();
        let live = flavor.guest.live();
        let outcome = engine
            .minecraft()
            .register(owner, official, live, Arc::new(flavor));
        registered(owner, "flavor", &id, outcome);
    }
}
