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
use super::provider::{
    FlavorInfo, FlavorProvider, InstallRequest, Prerequisite, ResolveRequest, Side, SERVER_ONLY,
};
use super::REQUIRED_JAVA_MAJORS;

/// BuildTools bootstraps its own Git on Windows and nowhere else.
pub(crate) const PREREQUISITES: &[Prerequisite] = if cfg!(windows) {
    &[]
} else {
    &[Prerequisite::Git]
};

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

/// The plugin flavor ids are Modrinth's own loader names, because the flavor id
/// *is* the plugin loader a content lookup filters by. Neither loader is widened
/// to the other: a Spigot build carries API a plugin written against it may
/// require.
pub struct BuildTools {
    info: FlavorInfo,
    product: Product,
}

impl BuildTools {
    pub const fn bukkit() -> Self {
        BuildTools {
            info: FlavorInfo {
                id: "bukkit",
                name: "CraftBukkit",
                summary: "The plugin server Spigot is built on, without its speed-ups. Pick Spigot unless you need this exactly.",
                loads: Some(ContentKind::Plugin),
                requires: PREREQUISITES,
                sides: SERVER_ONLY,
            },
            product: Product::Bukkit,
        }
    }

    pub const fn spigot() -> Self {
        BuildTools {
            info: FlavorInfo {
                id: "spigot",
                name: "Spigot",
                summary: "The classic plugin server, with the largest plugin library. Nobody may hand it out ready-made, so Hestia builds it here the first time — allow a few minutes.",
                loads: Some(ContentKind::Plugin),
                requires: PREREQUISITES,
                sides: SERVER_ONLY,
            },
            product: Product::Spigot,
        }
    }
}

#[async_trait]
impl FlavorProvider for BuildTools {
    fn info(&self) -> &FlavorInfo {
        &self.info
    }

    async fn versions(&self) -> Result<Vec<GameVersion>> {
        game_versions().await
    }

    async fn resolve_server(&self, request: &ResolveRequest<'_>) -> Result<ServerProfile> {
        Ok(ServerProfile {
            flavor: self.info.id.to_string(),
            game_version: request.version.clone(),
            loader_version: None,
            jar: ServerJar::Built {
                filename: self.product.jar_name(&request.version),
            },
            launch: ServerLaunch::Jar,
            java_major: java_major(&request.version).await?,
            jvm_args: Vec::new(),
        })
    }

    async fn install(
        &self,
        _side: Side,
        request: &InstallRequest<'_>,
        on: OnProgress<'_>,
    ) -> Result<()> {
        buildtools::ensure(self.product, request, on).await
    }
}
