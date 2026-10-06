//! Minecraft provider aggregate: the server and instance (client) flavor
//! registries and the flavors/versions/resolve entry points over them. Stateless
//! (every result is fetched from upstream), so it needs no data directory.

mod fabric;
pub(crate) mod launch;
pub(crate) mod log4j;
pub(crate) mod materialize;
mod meta;
mod neoforge;
mod paper;
pub(crate) mod ping;
mod provider;
pub(crate) mod quickplay;
pub(crate) mod rcon;
pub(crate) mod servers;
mod spigot;
mod vanilla;
pub(crate) mod world;

use anyhow::{Context, Result};
use proto::minecraft::{Flavor, GameVersion, InstanceProfile, ServerProfile};

pub use provider::{accepted_kinds, unmet, InstallRequest, Prerequisite, Side};
pub use provider::{InstanceProvider, Loads, ResolveRequest, ServerProvider};

/// The Java majors Minecraft launch profiles ever require: 8 (pre-1.17),
/// 16 (1.17), 17 (1.18–1.20.4), 21 (1.20.5+). Catalogue surfaces (the
/// installable-releases list) are filtered to these.
pub const REQUIRED_JAVA_MAJORS: [i32; 4] = [8, 16, 17, 21];

pub struct Minecraft {
    servers: Vec<Box<dyn ServerProvider>>,
    instances: Vec<Box<dyn InstanceProvider>>,
}

impl Default for Minecraft {
    fn default() -> Self {
        Minecraft {
            servers: vec![
                Box::new(vanilla::VanillaServer),
                Box::new(fabric::FabricServer),
                Box::new(paper::PaperServer),
                Box::new(paper::FoliaServer),
                Box::new(spigot::SpigotServer),
                Box::new(spigot::BukkitServer),
                Box::new(neoforge::NeoForgeServer),
            ],
            instances: vec![
                Box::new(vanilla::VanillaInstance),
                Box::new(fabric::FabricInstance),
                Box::new(neoforge::NeoForgeInstance),
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
    pub fn with_providers(
        servers: Vec<Box<dyn ServerProvider>>,
        instances: Vec<Box<dyn InstanceProvider>>,
    ) -> Self {
        Minecraft { servers, instances }
    }

    pub fn server_flavors(&self) -> Vec<Flavor> {
        self.servers
            .iter()
            .map(|p| flavor(p.id(), p.name(), p.summary(), Side::Server, p.loads()))
            .collect()
    }

    pub fn instance_flavors(&self) -> Vec<Flavor> {
        self.instances
            .iter()
            .map(|p| flavor(p.id(), p.name(), p.summary(), Side::Client, p.loads()))
            .collect()
    }

    /// What a flavor needs on the machine. Whether it is *there* is a question
    /// about this computer, not about the catalogue — `Engine::server_flavors`
    /// answers that.
    pub fn server_requires(&self, flavor: &str) -> &'static [Prerequisite] {
        self.server(flavor).map(|p| p.requires()).unwrap_or(&[])
    }

    pub fn instance_requires(&self, flavor: &str) -> &'static [Prerequisite] {
        self.instance(flavor).map(|p| p.requires()).unwrap_or(&[])
    }

    /// The content kind a server flavor's own loader consumes. An unregistered
    /// flavor — a record written by a build that had one we no longer ship —
    /// loads nothing rather than failing the read.
    pub fn server_loads(&self, flavor: &str) -> Loads {
        self.server(flavor).ok().and_then(|p| p.loads())
    }

    pub fn instance_loads(&self, flavor: &str) -> Loads {
        self.instance(flavor).ok().and_then(|p| p.loads())
    }

    pub async fn server_versions(&self, flavor: &str) -> Result<Vec<GameVersion>> {
        self.server(flavor)?.versions().await
    }

    pub async fn server_loader_versions(&self, flavor: &str, game: &str) -> Result<Vec<String>> {
        self.server(flavor)?.loader_versions(game).await
    }

    pub async fn instance_loader_versions(&self, flavor: &str, game: &str) -> Result<Vec<String>> {
        self.instance(flavor)?.loader_versions(game).await
    }

    pub async fn resolve_server(
        &self,
        flavor: &str,
        version: &str,
        loader_version: Option<String>,
    ) -> Result<ServerProfile> {
        tracing::info!(flavor, version, ?loader_version, "resolving server profile");
        self.server(flavor)?
            .resolve(&ResolveRequest {
                version: version.to_string(),
                loader_version,
            })
            .await
    }

    /// Build whatever a flavor's profile could not simply name (NeoForge's
    /// locally-patched game jar). Idempotent, so the launch path calls it every
    /// time rather than tracking whether it has run.
    pub async fn install_instance(
        &self,
        flavor: &str,
        request: &provider::InstallRequest<'_>,
        on_progress: materialize::OnProgress<'_>,
    ) -> Result<()> {
        self.instance(flavor)?.install(request, on_progress).await
    }

    /// The server twin of [`Minecraft::install_instance`].
    pub async fn install_server(
        &self,
        flavor: &str,
        request: &provider::InstallRequest<'_>,
        on_progress: materialize::OnProgress<'_>,
    ) -> Result<()> {
        self.server(flavor)?.install(request, on_progress).await
    }

    pub async fn instance_versions(&self, flavor: &str) -> Result<Vec<GameVersion>> {
        self.instance(flavor)?.versions().await
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
        self.instance(flavor)?
            .resolve(&ResolveRequest {
                version: version.to_string(),
                loader_version,
            })
            .await
    }

    fn server(&self, flavor: &str) -> Result<&dyn ServerProvider> {
        self.servers
            .iter()
            .map(AsRef::as_ref)
            .find(|p| p.id() == flavor)
            .with_context(|| format!("unknown server flavor: {flavor}"))
    }

    fn instance(&self, flavor: &str) -> Result<&dyn InstanceProvider> {
        self.instances
            .iter()
            .map(AsRef::as_ref)
            .find(|p| p.id() == flavor)
            .with_context(|| format!("unknown instance flavor: {flavor}"))
    }
}

fn flavor(id: &str, name: &str, summary: &str, side: Side, loads: Loads) -> Flavor {
    Flavor {
        id: id.to_string(),
        name: name.to_string(),
        summary: summary.to_string(),
        accepts: accepted_kinds(side, loads),
        requires: Vec::new(),
    }
}
