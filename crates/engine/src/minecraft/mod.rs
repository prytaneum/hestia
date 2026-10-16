//! Minecraft provider aggregate: the flavor registry and the
//! flavors/versions/resolve entry points over it, each taking the side. Stateless
//! (every result is fetched from upstream), so it needs no data directory.

mod fabric;
pub(crate) mod launch;
pub(crate) mod log4j;
pub(crate) mod materialize;
mod meta;
mod neoforge;
mod paper;
pub(crate) mod ping;
pub mod profile;
mod provider;
pub(crate) mod quickplay;
pub(crate) mod rcon;
pub(crate) mod servers;
mod spigot;
mod vanilla;
pub(crate) mod world;

use anyhow::{Context, Result};
use proto::minecraft::{Flavor, GameVersion};

use profile::{InstanceProfile, ServerProfile};

pub use provider::{accepted_kinds, unmet, InstallRequest, Prerequisite, Side};
pub use provider::{FlavorInfo, FlavorProvider, Loads, ResolveRequest};

/// The Java majors Minecraft launch profiles ever require: 8 (pre-1.17),
/// 16 (1.17), 17 (1.18–1.20.4), 21 (1.20.5+). Catalogue surfaces (the
/// installable-releases list) are filtered to these.
pub const REQUIRED_JAVA_MAJORS: [i32; 4] = [8, 16, 17, 21];

pub struct Minecraft {
    flavors: Vec<Box<dyn FlavorProvider>>,
}

impl Default for Minecraft {
    fn default() -> Self {
        Minecraft {
            flavors: vec![
                Box::new(vanilla::Vanilla),
                Box::new(fabric::Fabric),
                Box::new(paper::PaperProject::paper()),
                Box::new(paper::PaperProject::folia()),
                Box::new(spigot::BuildTools::spigot()),
                Box::new(spigot::BuildTools::bukkit()),
                Box::new(neoforge::NeoForge),
            ],
        }
    }
}

impl Minecraft {
    pub fn new() -> Self {
        Self::default()
    }

    /// Build over a given registry rather than the shipped one. The seam a test
    /// crosses to resolve a profile without reaching upstream.
    pub fn with_providers(flavors: Vec<Box<dyn FlavorProvider>>) -> Self {
        Minecraft { flavors }
    }

    pub fn flavors(&self, side: Side) -> Vec<Flavor> {
        self.flavors
            .iter()
            .map(|p| p.info())
            .filter(|info| info.sides.contains(&side))
            .map(|info| Flavor {
                id: info.id.to_string(),
                name: info.name.to_string(),
                summary: info.summary.to_string(),
                accepts: accepted_kinds(side, info.loads),
                requires: Vec::new(),
            })
            .collect()
    }

    /// What a flavor needs on the machine. Whether it is *there* is a question
    /// about this computer, not about the catalogue — `Engine::flavors`
    /// answers that.
    pub fn requires(&self, side: Side, flavor: &str) -> &'static [Prerequisite] {
        self.provider(side, flavor)
            .map(|p| p.info().requires)
            .unwrap_or(&[])
    }

    /// The content kind a flavor's own loader consumes. An unregistered
    /// flavor — a record written by a build that had one we no longer ship —
    /// loads nothing rather than failing the read.
    pub fn loads(&self, side: Side, flavor: &str) -> Loads {
        self.provider(side, flavor)
            .ok()
            .and_then(|p| p.info().loads)
    }

    pub async fn versions(&self, side: Side, flavor: &str) -> Result<Vec<GameVersion>> {
        self.provider(side, flavor)?.versions().await
    }

    pub async fn loader_versions(
        &self,
        side: Side,
        flavor: &str,
        game: &str,
    ) -> Result<Vec<String>> {
        self.provider(side, flavor)?.loader_versions(game).await
    }

    pub async fn resolve_server(
        &self,
        flavor: &str,
        version: &str,
        loader_version: Option<String>,
    ) -> Result<ServerProfile> {
        tracing::info!(flavor, version, ?loader_version, "resolving server profile");
        self.provider(Side::Server, flavor)?
            .resolve_server(&ResolveRequest {
                version: version.to_string(),
                loader_version,
            })
            .await
    }

    pub async fn resolve_instance(
        &self,
        flavor: &str,
        version: &str,
        loader_version: Option<String>,
    ) -> Result<InstanceProfile> {
        tracing::info!(
            flavor,
            version,
            ?loader_version,
            "resolving instance profile"
        );
        self.provider(Side::Client, flavor)?
            .resolve_client(&ResolveRequest {
                version: version.to_string(),
                loader_version,
            })
            .await
    }

    /// Build whatever a flavor's profile could not simply name (NeoForge's
    /// locally-patched game jar). Idempotent, so the launch path calls it every
    /// time rather than tracking whether it has run.
    pub async fn install(
        &self,
        side: Side,
        flavor: &str,
        request: &provider::InstallRequest<'_>,
        on_progress: materialize::OnProgress<'_>,
    ) -> Result<()> {
        self.provider(side, flavor)?
            .install(side, request, on_progress)
            .await
    }

    fn provider(&self, side: Side, flavor: &str) -> Result<&dyn FlavorProvider> {
        self.flavors
            .iter()
            .map(AsRef::as_ref)
            .find(|p| p.info().id == flavor && p.info().sides.contains(&side))
            .with_context(|| format!("unknown {} flavor: {flavor}", side.noun()))
    }
}
