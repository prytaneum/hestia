//! The PaperMC server platforms — Paper and Folia. Both are one self-contained
//! jar per build, so a profile is the vanilla shape with the jar swapped; they
//! differ only in which project of the same service they read, and in that
//! Folia publishes far fewer versions.
//!
//! Neither has a client, so neither lists `Side::Client`: `instance create
//! --flavor paper` fails as an unknown instance flavor, which is the truth.
//!
//! A *build* is what the flavor's `loader_version` names. Paper publishes many
//! builds per game version and a server operator routinely needs a specific one
//! — pinning a known-good build, or taking an experimental one deliberately —
//! so the existing loader-version seam carries it, exactly as Fabric's loader
//! builds do.

use anyhow::Result;
use async_trait::async_trait;
use proto::content::ContentKind;
use proto::minecraft::{GameVersion, VersionKind};

use super::meta::{mojang, paper};
use super::profile::{ServerJar, ServerLaunch, ServerProfile};
use super::provider::{FlavorInfo, FlavorProvider, ResolveRequest, SERVER_ONLY};

/// The game versions a project publishes, ordered and typed by Mojang's own
/// manifest: PaperMC states neither, and the manifest is already the ordering
/// ground truth every other flavor is judged against. A version Mojang does not
/// list (an April Fools' build, a rename) keeps its place at the end rather than
/// vanishing from the catalogue, typed as a snapshot since it is not a release.
async fn game_versions(project: &str) -> Result<Vec<GameVersion>> {
    let published = paper::game_versions(project).await?;
    let mut versions: Vec<GameVersion> = mojang::versions()
        .await?
        .into_iter()
        .filter(|v| published.iter().any(|p| p == &v.id))
        .collect();
    let unlisted: Vec<GameVersion> = published
        .into_iter()
        .filter(|p| !versions.iter().any(|v| &v.id == p))
        .map(|id| GameVersion {
            id,
            kind: VersionKind::Snapshot,
            stable: false,
        })
        .collect();
    versions.extend(unlisted);
    Ok(versions)
}

async fn resolve(project: &str, request: &ResolveRequest) -> Result<ServerProfile> {
    let build = match &request.loader_version {
        Some(number) => paper::build(project, &request.version, number).await?,
        None => paper::newest_build(project, &request.version).await?,
    };
    let java = paper::java(project, &request.version).await?;
    Ok(ServerProfile {
        flavor: project.to_string(),
        game_version: request.version.clone(),
        loader_version: Some(build.number.to_string()),
        jar: ServerJar::Download(build.download),
        launch: ServerLaunch::Jar,
        java_major: java.major,
        jvm_args: java.flags,
    })
}

async fn build_numbers(project: &str, game: &str) -> Result<Vec<String>> {
    Ok(paper::builds(project, game)
        .await?
        .into_iter()
        .map(|b| b.number.to_string())
        .collect())
}

pub struct PaperProject {
    info: FlavorInfo,
}

impl PaperProject {
    pub const fn paper() -> Self {
        PaperProject {
            info: FlavorInfo {
                id: "paper",
                name: "Paper",
                summary: "A faster server that runs Bukkit plugins, tuned by the Paper team for each version.",
                loads: Some(ContentKind::Plugin),
                requires: &[],
                sides: SERVER_ONLY,
            },
        }
    }

    /// Folia's plugins are filtered as `folia`, never widened to `paper`: a
    /// plugin that never claimed Folia support breaks on its regionised
    /// scheduler, and the catalogue is the only place that is knowable.
    pub const fn folia() -> Self {
        PaperProject {
            info: FlavorInfo {
                id: "folia",
                name: "Folia",
                summary: "Paper spread across CPU cores, for very large worlds. Only plugins made for Folia will work.",
                loads: Some(ContentKind::Plugin),
                requires: &[],
                sides: SERVER_ONLY,
            },
        }
    }
}

#[async_trait]
impl FlavorProvider for PaperProject {
    fn info(&self) -> &FlavorInfo {
        &self.info
    }

    async fn versions(&self) -> Result<Vec<GameVersion>> {
        game_versions(self.info.id).await
    }

    async fn loader_versions(&self, game: &str) -> Result<Vec<String>> {
        build_numbers(self.info.id, game).await
    }

    async fn resolve_server(&self, request: &ResolveRequest) -> Result<ServerProfile> {
        resolve(self.info.id, request).await
    }
}
