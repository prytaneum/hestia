//! Bukkit and Spigot — server platforms whose jar may not be redistributed, so
//! the profile names a jar that does not exist yet (carrying no URL) and the
//! `install` hook compiles it with BuildTools. Server-side only, and one build
//! per game version, so neither has a loader version to pin.

mod buildtools;

use anyhow::Result;
use async_trait::async_trait;
use proto::content::ContentKind;
use proto::minecraft::GameVersion;

use self::buildtools::Product;
use super::materialize::OnProgress;
use super::meta::{mojang, spigot};
use super::profile::{ServerJar, ServerLaunch, ServerProfile};
use super::provider::{InstallRequest, Loads, Prerequisite, ResolveRequest, ServerProvider};
use super::REQUIRED_JAVA_MAJORS;

/// BuildTools bootstraps its own Git on Windows and nowhere else.
pub(crate) fn prerequisites() -> &'static [Prerequisite] {
    if cfg!(windows) {
        &[]
    } else {
        &[Prerequisite::Git]
    }
}

/// What the versions predating the hub's `javaVersions` field (the 1.8 line and
/// older) were built with.
const LEGACY_JAVA_MAJOR: i32 = 8;

/// The game versions BuildTools can build, ordered and typed by Mojang's
/// manifest.
///
/// Filtering against the manifest is not presentation here as it is for Paper —
/// it is the *selection*. The hub indexes its metadata by Jenkins build number
/// as well as by game version, so all but a few dozen of the four thousand
/// names it publishes are build numbers; a name Mojang does not list is one of
/// those, never a version to offer.
async fn game_versions() -> Result<Vec<GameVersion>> {
    let published = spigot::versions().await?;
    Ok(mojang::versions()
        .await?
        .into_iter()
        .filter(|v| published.iter().any(|p| p == &v.id))
        .collect())
}

/// The Java a version is built and run with. The hub states a range of
/// class-file majors; the low end is the version's real requirement, narrowed
/// to a runtime the launcher can actually install so a create fails at
/// resolution rather than at the Java step.
async fn java_major(version: &str) -> Result<i32> {
    let supported = spigot::java_versions(version).await?;
    let (Some(&low), Some(&high)) = (supported.first(), supported.last()) else {
        return Ok(LEGACY_JAVA_MAJOR);
    };
    Ok(REQUIRED_JAVA_MAJORS
        .into_iter()
        .find(|major| (low..=high).contains(major))
        .unwrap_or(low))
}

async fn resolve(product: Product, request: &ResolveRequest) -> Result<ServerProfile> {
    Ok(ServerProfile {
        flavor: product.flavor().to_string(),
        game_version: request.version.clone(),
        loader_version: None,
        jar: ServerJar::Built {
            filename: product.jar_name(&request.version),
        },
        launch: ServerLaunch::Jar,
        java_major: java_major(&request.version).await?,
        jvm_args: Vec::new(),
    })
}

impl Product {
    fn flavor(self) -> &'static str {
        match self {
            Product::Bukkit => BUKKIT_ID,
            Product::Spigot => SPIGOT_ID,
        }
    }
}

/// Modrinth's own loader names, because the flavor id *is* the plugin loader a
/// content lookup filters by.
const BUKKIT_ID: &str = "bukkit";
const SPIGOT_ID: &str = "spigot";

pub struct BukkitServer;

#[async_trait]
impl ServerProvider for BukkitServer {
    fn id(&self) -> &'static str {
        BUKKIT_ID
    }
    fn name(&self) -> &'static str {
        "CraftBukkit"
    }
    fn summary(&self) -> &'static str {
        "The plugin server Spigot is built on, without its speed-ups. Pick Spigot unless you need this exactly."
    }
    fn loads(&self) -> Loads {
        Some(ContentKind::Plugin)
    }
    fn requires(&self) -> &'static [Prerequisite] {
        prerequisites()
    }

    async fn versions(&self) -> Result<Vec<GameVersion>> {
        game_versions().await
    }

    async fn resolve(&self, request: &ResolveRequest) -> Result<ServerProfile> {
        resolve(Product::Bukkit, request).await
    }

    async fn install(&self, request: &InstallRequest<'_>, on: OnProgress<'_>) -> Result<()> {
        buildtools::ensure(Product::Bukkit, request, on).await
    }
}

pub struct SpigotServer;

#[async_trait]
impl ServerProvider for SpigotServer {
    fn id(&self) -> &'static str {
        SPIGOT_ID
    }
    fn name(&self) -> &'static str {
        "Spigot"
    }
    fn summary(&self) -> &'static str {
        "The classic plugin server, with the largest plugin library. Nobody may hand it out ready-made, so Hestia builds it here the first time — allow a few minutes."
    }
    /// Spigot's plugins are filtered as `spigot`, never widened to `bukkit`:
    /// the two are separate Modrinth loaders, and a Spigot build carries API a
    /// plugin written against it may require.
    fn loads(&self) -> Loads {
        Some(ContentKind::Plugin)
    }
    fn requires(&self) -> &'static [Prerequisite] {
        prerequisites()
    }

    async fn versions(&self) -> Result<Vec<GameVersion>> {
        game_versions().await
    }

    async fn resolve(&self, request: &ResolveRequest) -> Result<ServerProfile> {
        resolve(Product::Spigot, request).await
    }

    async fn install(&self, request: &InstallRequest<'_>, on: OnProgress<'_>) -> Result<()> {
        buildtools::ensure(Product::Spigot, request, on).await
    }
}
