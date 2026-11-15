//! Minecraft provider aggregate: the flavor registry and the
//! flavors/versions/resolve entry points over it, each taking the side. It holds
//! no state of its own, but the catalogue readers keep regenerable copies of
//! what they fetch under `meta/`.

pub(crate) mod bedrock;
pub(crate) mod launch;
mod loaders;
pub(crate) mod log4j;
pub(crate) mod materialize;
pub(crate) mod maven;
mod meta;
pub(crate) mod overlay;
pub(crate) mod ping;
pub(crate) mod plan;
pub mod profile;
mod provider;
pub(crate) mod quickplay;
pub(crate) mod rcon;
pub(crate) mod rules;
pub(crate) mod servers;
mod vanilla;
pub(crate) use vanilla::serves as serves_vanilla;
pub(crate) mod world;

use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use proto::minecraft::{Edition, Flavor, GameVersion};

use crate::cache::Cache;
use crate::roster::{RegisterError, Roster, Standing};
use profile::{InstanceProfile, ServerProfile};

pub use loaders::Loaders;
pub use provider::{unmet, InstallRequest, JavaInstall, Prerequisite, Side};
pub use provider::{FlavorInfo, FlavorProvider, Loads, ResolveRequest};
pub use provider::{BOTH_SIDES, CLIENT_ONLY, SERVER_ONLY};

/// The Java majors Minecraft launch profiles ever require: 8 (pre-1.17),
/// 16 (1.17), 17 (1.18–1.20.4), 21 (1.20.5+). Catalogue surfaces (the
/// installable-releases list) are filtered to these.
pub const REQUIRED_JAVA_MAJORS: [i32; 4] = [8, 16, 17, 21];

pub struct Minecraft {
    flavors: Roster<dyn FlavorProvider>,
}

impl Default for Minecraft {
    fn default() -> Self {
        Self::with_providers(vec![Box::new(vanilla::Vanilla), Box::new(bedrock::Bedrock)])
    }
}

impl Minecraft {
    pub fn new() -> Self {
        Self::default()
    }

    /// Build over a given registry rather than the shipped one. The seam a test
    /// crosses to resolve a profile without reaching upstream.
    pub fn with_providers(flavors: Vec<Box<dyn FlavorProvider>>) -> Self {
        Minecraft {
            flavors: Roster::builtin(flavors.into_iter().map(|p| {
                let provider: Arc<dyn FlavorProvider> = p.into();
                (provider.info().id.to_string(), provider)
            })),
        }
    }

    /// Add `owner`'s flavor, whose id must read `<owner>.<id>`.
    pub fn register(
        &self,
        owner: &str,
        standing: Standing,
        provider: Arc<dyn FlavorProvider>,
    ) -> Result<(), RegisterError> {
        let id = provider.info().id.to_string();
        self.flavors.register(owner, standing, &id, provider)
    }

    /// Remove every flavor `owner` registered. An entry already created from
    /// one keeps its record and loads nothing, as for any unregistered flavor.
    pub fn unregister(&self, owner: &str) -> usize {
        self.flavors.unregister(owner)
    }

    /// How the interchange formats name the registered flavors' loaders.
    pub fn loaders(&self) -> Loaders {
        Loaders::of(&self.flavors.snapshot())
    }

    pub fn flavors(&self, side: Side, edition: Edition) -> Vec<Flavor> {
        self.flavors
            .snapshot()
            .iter()
            .map(|p| p.info())
            .filter(|info| info.edition == edition && info.sides.contains(&side))
            .map(|info| Flavor {
                id: info.id.to_string(),
                name: info.name.to_string(),
                summary: info.summary.to_string(),
                accepts: crate::edition::accepted_kinds(side, edition, info.loads),
                requires: Vec::new(),
            })
            .collect()
    }

    /// What a flavor needs on the machine. Whether it is *there* is a question
    /// about this computer, not about the catalogue — `Engine::flavors`
    /// answers that.
    pub fn requires(&self, side: Side, edition: Edition, flavor: &str) -> Vec<Prerequisite> {
        self.provider(side, edition, flavor)
            .map(|p| p.info().requires.to_vec())
            .unwrap_or_default()
    }

    /// The content kind a flavor's own loader consumes. An unregistered
    /// flavor — a record written by a build that had one we no longer ship —
    /// loads nothing rather than failing the read.
    pub fn loads(&self, side: Side, edition: Edition, flavor: &str) -> Loads {
        self.provider(side, edition, flavor)
            .ok()
            .and_then(|p| p.info().loads)
    }

    /// The loader names a content version may carry to run on `flavor`, its own
    /// first. An unregistered flavor — a record written by a build that had one
    /// we no longer ship — is filtered by its id.
    pub fn content_loaders(&self, side: Side, edition: Edition, flavor: &str) -> Vec<String> {
        self.provider(side, edition, flavor)
            .map(|p| p.info().loader_names())
            .unwrap_or_else(|_| vec![flavor.to_string()])
    }

    /// The loader names a content platform files content under for each of
    /// `names`: a flavor's id widens to its declared loader and the ones it is
    /// compatible with, and a name that is no flavor's id is already a loader.
    pub fn declared_loaders(&self, names: &[String]) -> Vec<String> {
        let mut loaders: Vec<String> = Vec::new();
        for name in names.iter().filter(|name| !name.is_empty()) {
            let declared = self
                .flavors
                .resolve(name, |p| p.info().edition == Edition::Java)
                .map(|(_, p)| p.info().loader_names())
                .unwrap_or_else(|_| vec![name.clone()]);
            for loader in declared {
                if !loaders.contains(&loader) {
                    loaders.push(loader);
                }
            }
        }
        loaders
    }

    /// The Java flavor a content platform's loader name stands for on `side`:
    /// the flavor of that id when there is one, else one declaring that loader.
    pub fn flavor_of_loader(&self, side: Side, loader: &str) -> Option<String> {
        if let Ok(id) = self.canonical(side, Edition::Java, loader) {
            return Some(id);
        }
        self.flavors
            .snapshot()
            .iter()
            .map(|p| p.info())
            .filter(|info| info.edition == Edition::Java && info.sides.contains(&side))
            .find(|info| info.loader_name() == loader)
            .map(|info| info.id.to_string())
    }

    /// The full id of the flavor `name` means on `side`, for what is stored.
    pub fn canonical(&self, side: Side, edition: Edition, name: &str) -> Result<String> {
        self.resolve(side, edition, name).map(|(id, _)| id)
    }

    pub async fn versions(
        &self,
        side: Side,
        edition: Edition,
        flavor: &str,
    ) -> Result<Vec<GameVersion>> {
        self.provider(side, edition, flavor)?.versions().await
    }

    pub async fn loader_versions(
        &self,
        side: Side,
        edition: Edition,
        flavor: &str,
        game: &str,
    ) -> Result<Vec<String>> {
        self.provider(side, edition, flavor)?
            .loader_versions(game)
            .await
    }

    pub async fn resolve_server(
        &self,
        edition: Edition,
        flavor: &str,
        version: &str,
        loader_version: Option<String>,
        meta: &Path,
        cache: Option<&Cache>,
    ) -> Result<ServerProfile> {
        tracing::info!(flavor, version, ?loader_version, "resolving server profile");
        self.provider(Side::Server, edition, flavor)?
            .resolve_server(&ResolveRequest {
                version: version.to_string(),
                loader_version,
                meta,
                cache,
            })
            .await
    }

    pub async fn resolve_instance(
        &self,
        flavor: &str,
        version: &str,
        loader_version: Option<String>,
        meta: &Path,
        cache: Option<&Cache>,
    ) -> Result<InstanceProfile> {
        tracing::info!(
            flavor,
            version,
            ?loader_version,
            "resolving instance profile"
        );
        self.provider(Side::Client, Edition::Java, flavor)?
            .resolve_client(&ResolveRequest {
                version: version.to_string(),
                loader_version,
                meta,
                cache,
            })
            .await
    }

    /// Build whatever a flavor's profile could not simply name (NeoForge's
    /// locally-patched game jar). Idempotent, so the launch path calls it every
    /// time rather than tracking whether it has run.
    pub async fn install(
        &self,
        side: Side,
        edition: Edition,
        flavor: &str,
        request: &provider::InstallRequest<'_>,
        on_progress: materialize::OnProgress<'_>,
    ) -> Result<()> {
        self.provider(side, edition, flavor)?
            .install(side, request, on_progress)
            .await
    }

    /// The flavor a plugin's profile builds on: Mojang's game, unchanged.
    pub(crate) fn vanilla(&self) -> Option<Arc<dyn FlavorProvider>> {
        self.provider(Side::Client, Edition::Java, vanilla::ID).ok()
    }

    pub(crate) fn provider(
        &self,
        side: Side,
        edition: Edition,
        flavor: &str,
    ) -> Result<Arc<dyn FlavorProvider>> {
        self.resolve(side, edition, flavor)
            .map(|(_, provider)| provider)
    }

    fn resolve(
        &self,
        side: Side,
        edition: Edition,
        name: &str,
    ) -> Result<(String, Arc<dyn FlavorProvider>)> {
        self.flavors
            .resolve(name, |p| {
                let info = p.info();
                info.edition == edition && info.sides.contains(&side)
            })
            .map_err(|unresolved| unresolved.into_error(name))
    }
}
