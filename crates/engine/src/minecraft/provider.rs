//! The provider seam. A flavor implements one trait for the sides it serves,
//! listing the game versions it supports and resolving a request into a full
//! launch profile. The `Minecraft` aggregate holds one boxed registry —
//! adding a flavor is a new impl plus one line in `Minecraft::default`.

use std::path::Path;

use anyhow::{bail, Result};
use async_trait::async_trait;
use proto::content::ContentKind;
use proto::minecraft::{GameVersion, Requirement};

use crate::cache::Cache;
use crate::minecraft::materialize::OnProgress;
use crate::minecraft::profile::{InstanceProfile, ServerProfile};
use crate::process::ProcessSupervisor;

/// A resolution request: a game version and, for modloaders, an optional pinned
/// loader version (the newest stable loader is chosen when absent).
pub struct ResolveRequest {
    pub version: String,
    pub loader_version: Option<String>,
}

/// What a flavor needs to build whatever its profile cannot simply name — the
/// patched jar a NeoForge loader runs, or a Spigot server nobody may
/// redistribute. Given to [`FlavorProvider::install`].
pub struct InstallRequest<'a> {
    pub game_version: &'a str,
    pub loader_version: Option<&'a str>,
    /// Where this entry's install writes: `meta/` for a client, the server's
    /// own data directory otherwise.
    pub root: &'a Path,
    /// The shared `meta/` root, for work no single entry owns.
    pub meta: &'a Path,
    /// The vanilla jar for this side, already materialized.
    pub minecraft_jar: &'a Path,
    pub java: &'a Path,
    pub cache: Option<&'a Cache>,
    pub processes: &'a ProcessSupervisor,
}

/// The third-party content a flavor's own loader consumes: mods for a
/// modloader, plugins for a server platform, nothing for vanilla. Datapacks
/// (and, client-side, resourcepacks and shaders) belong to the game rather than
/// the flavor, so they are not named here — [`accepted_kinds`] adds them.
pub type Loads = Option<ContentKind>;

/// Which half of the game an entry is, for the rules that differ between them.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Server,
    Client,
}

impl Side {
    pub fn noun(self) -> &'static str {
        match self {
            Side::Server => "server",
            Side::Client => "instance",
        }
    }
}

/// Everything an entry of this flavor and side accepts, composed from two
/// independent facts: whatever the flavor's own loader takes, plus what the
/// side reads for itself — a client its resourcepacks and shaders, either side
/// the datapacks that are world data rather than loader content.
///
/// It lives beside `Loads` because it is the same vocabulary: adding a flavor
/// is one impl plus one registry line, with no table to update here or in any
/// front-end.
pub fn accepted_kinds(side: Side, loads: Loads) -> Vec<ContentKind> {
    let mut kinds: Vec<ContentKind> = loads.into_iter().collect();
    if side == Side::Client {
        kinds.push(ContentKind::ResourcePack);
        kinds.push(ContentKind::Shader);
    }
    kinds.push(ContentKind::DataPack);
    kinds
}

/// A tool a flavor needs on the machine that Hestia cannot install itself.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Prerequisite {
    Git,
}

impl Prerequisite {
    pub fn name(self) -> &'static str {
        match self {
            Prerequisite::Git => "Git",
        }
    }

    pub fn url(self) -> &'static str {
        match self {
            Prerequisite::Git => "https://git-scm.com/downloads",
        }
    }

    pub async fn installed(self) -> bool {
        tokio::process::Command::new(match self {
            Prerequisite::Git => "git",
        })
        .arg("--version")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .is_ok_and(|status| status.success())
    }
}

/// Which of `required` are not on this machine, in the shape the wire carries.
pub async fn unmet(required: &[Prerequisite]) -> Vec<Requirement> {
    let mut missing = Vec::new();
    for prerequisite in required {
        if !prerequisite.installed().await {
            missing.push(Requirement {
                name: prerequisite.name().to_string(),
                url: prerequisite.url().to_string(),
            });
        }
    }
    missing
}

pub struct FlavorInfo {
    pub id: &'static str,
    pub name: &'static str,
    /// One line describing the distribution — what it is, and anything about
    /// running it a user should know before choosing it. Rendered by every
    /// front-end, so a new flavor needs no front-end change to explain itself.
    pub summary: &'static str,
    pub loads: Loads,
    /// Tools this flavor needs on the machine; none for all but a flavor that
    /// builds its own artifacts.
    pub requires: &'static [Prerequisite],
    pub sides: &'static [Side],
}

pub const BOTH_SIDES: &[Side] = &[Side::Server, Side::Client];
pub const SERVER_ONLY: &[Side] = &[Side::Server];

#[async_trait]
pub trait FlavorProvider: Send + Sync {
    fn info(&self) -> &FlavorInfo;
    async fn versions(&self) -> Result<Vec<GameVersion>>;
    /// The loader builds available for a game version, newest first. A flavor
    /// with no loader concept (vanilla) reports none — the default.
    async fn loader_versions(&self, _game: &str) -> Result<Vec<String>> {
        Ok(Vec::new())
    }
    async fn resolve_server(&self, _request: &ResolveRequest) -> Result<ServerProfile> {
        bail!("unknown server flavor: {}", self.info().id)
    }
    async fn resolve_client(&self, _request: &ResolveRequest) -> Result<InstanceProfile> {
        bail!("unknown instance flavor: {}", self.info().id)
    }

    /// Build whatever the profile cannot name — the patched jar a NeoForge
    /// client runs, which exists nowhere to download. Idempotent: a client
    /// install runs on every launch and must cost nothing once done.
    async fn install(
        &self,
        _side: Side,
        _request: &InstallRequest<'_>,
        _on: OnProgress<'_>,
    ) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_flavor_contributes_its_loader_kind_and_the_side_the_rest() {
        assert_eq!(
            accepted_kinds(Side::Server, Some(ContentKind::Plugin)),
            vec![ContentKind::Plugin, ContentKind::DataPack],
            "a paper or spigot server takes plugins, never mods"
        );
        assert_eq!(
            accepted_kinds(Side::Server, Some(ContentKind::Mod)),
            vec![ContentKind::Mod, ContentKind::DataPack]
        );
        assert_eq!(
            accepted_kinds(Side::Server, None),
            vec![ContentKind::DataPack],
            "vanilla loads nothing of its own, but a world still takes datapacks"
        );
        assert_eq!(
            accepted_kinds(Side::Client, None),
            vec![
                ContentKind::ResourcePack,
                ContentKind::Shader,
                ContentKind::DataPack
            ],
            "a client reads packs whatever its flavor loads"
        );
    }
}
