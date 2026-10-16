//! NeoForge — a modloader whose launch profile layers over the base game
//! version like Fabric's, but whose game jar has to be *built* locally first.
//!
//! Everything a version needs comes out of its installer jar on
//! `maven.neoforged.net` (see `meta::neoforge`), and the patched jar the loader
//! runs is produced by the processor chain in [`processors`] — there is no
//! published artifact for it. That is why this flavor uses the provider's
//! `install` hook where every other flavor only names downloads.
//!
//! The catalogue needs no service either: a NeoForge version *is* its game
//! version plus a build number, so both lists come from the `maven-metadata.xml`
//! of the two artifacts NeoForge publishes under.

pub(crate) mod processors;

use std::path::Path;

use anyhow::{Context, Result};
use async_trait::async_trait;
use proto::content::ContentKind;
use proto::minecraft::GameVersion;

use super::materialize::{self, OnProgress};
use super::meta::{mojang, neoforge};
use super::profile::{InstanceProfile, ServerJar, ServerLaunch, ServerProfile};
use super::provider::{InstallRequest, InstanceProvider, Loads, ResolveRequest, ServerProvider};
use crate::download::Downloader;

const ID: &str = "neoforge";
const NAME: &str = "NeoForge";
const SUMMARY: &str =
    "Mod loader for big, content-heavy modpacks. The first launch prepares the game files, so it takes a few minutes.";

/// The game versions NeoForge builds exist for, newest first and typed by
/// Mojang's manifest — the same ground truth every other flavor is ordered by.
/// A derived game version the manifest does not list is dropped: the mapping is
/// arithmetic on the version string, so a result naming no real version is a
/// failed derivation rather than a version to offer.
async fn game_versions() -> Result<Vec<GameVersion>> {
    let targeted: Vec<String> = neoforge::versions()
        .await?
        .iter()
        .filter_map(|v| neoforge::game_version(v))
        .collect();
    Ok(mojang::versions()
        .await?
        .into_iter()
        .filter(|v| targeted.iter().any(|t| t == &v.id))
        .collect())
}

/// Every NeoForge build for a game version, newest first. Maven lists versions
/// oldest-first, so the order is reversed.
async fn builds(game: &str) -> Result<Vec<String>> {
    let mut builds: Vec<String> = neoforge::versions()
        .await?
        .into_iter()
        .filter(|v| neoforge::game_version(v).as_deref() == Some(game))
        .collect();
    builds.reverse();
    Ok(builds)
}

/// The build a resolve uses: the pinned one, else simply the newest.
///
/// NeoForge is the one flavor with no stability preference, following
/// modrinth/code: daedalus marks *every* NeoForge build unstable, so the
/// launcher's stable/latest choice is permanently disabled for it and always
/// resolves latest. Preferring a release build over a newer `-beta` would also
/// mean pinning many game versions to a months-old build — NeoForge leaves a
/// whole line on `-beta` for its lifetime (26.2 has 34 builds and no release),
/// so the suffix tracks the game version's own maturity more than the build's.
async fn resolve_loader(request: &ResolveRequest) -> Result<String> {
    if let Some(pinned) = &request.loader_version {
        return Ok(pinned.clone());
    }
    let builds = builds(&request.version).await?;
    builds.first().cloned().with_context(|| {
        format!(
            "no neoforge build is published for Minecraft {}",
            request.version
        )
    })
}

/// Where a version's installer jar is kept once fetched. It is not a launch
/// artifact — it is the *source* of the profile and the patches — so it lives
/// beside the other derived game files under `meta/`.
fn installer_path(root: &Path, loader: &str) -> std::path::PathBuf {
    root.join("neoforge").join(format!("{loader}.jar"))
}

async fn read_installer(
    request: &InstallRequest<'_>,
    loader: &str,
) -> Result<(std::path::PathBuf, neoforge::Installer)> {
    let path = installer_path(request.root, loader);
    if !path.is_file() {
        // No published checksum to verify against, so the fetch is plain; a
        // truncated jar fails to parse below rather than being trusted.
        Downloader::new(request.cache)
            .fetch(&neoforge::installer_url(loader), &path, None, &|_| Ok(()))
            .await
            .with_context(|| format!("cannot fetch the neoforge {loader} installer"))?;
    }
    let bytes = std::fs::read(&path).with_context(|| format!("cannot read {}", path.display()))?;
    let installer = neoforge::read_installer(&bytes)?;
    Ok((path, installer))
}

/// Download the tools and inputs the processors need, then run them. The install
/// profile's libraries are a superset of the launch profile's — the extra ones
/// are the processor tools themselves — and they share the libraries root, so
/// what the launch already fetched is skipped.
async fn run_install(
    request: &InstallRequest<'_>,
    side: processors::Side,
    on_progress: OnProgress<'_>,
) -> Result<()> {
    let loader = request
        .loader_version
        .context("a neoforge install needs its build")?;
    // The installer is read before the idempotence check rather than after: it
    // is the only thing that states where the patched jar lands, and that
    // coordinate has already changed once between NeoForge generations. It is
    // on disk after the first fetch, so the re-read is cheap next to the chain
    // it guards.
    let (installer_jar, installer) = read_installer(request, loader).await?;
    let libraries = request.root.join("libraries");
    if processors::patched_output(&installer, side, &libraries).is_some_and(|jar| jar.is_file()) {
        return Ok(());
    }

    materialize::ensure_libraries(
        request.cache,
        &mojang::libraries(&installer.profile),
        &libraries,
        on_progress,
    )
    .await?;

    tracing::info!(
        loader,
        side = side.as_str(),
        "building the neoforge game jar"
    );
    processors::run(
        &installer,
        &processors::Install {
            root: request.root,
            installer: &installer_jar,
            minecraft_jar: request.minecraft_jar,
            side,
            java: request.java,
            processes: request.processes,
        },
        on_progress,
    )
    .await
}

/// The arg file the server-side install generates, relative to the directory
/// the server launches from. Its contents name every library relatively, so the
/// path is stated the same way.
pub(crate) fn server_args_file(loader: &str) -> String {
    let name = if cfg!(windows) {
        "win_args.txt"
    } else {
        "unix_args.txt"
    };
    format!(
        "libraries/net/neoforged/{}/{loader}/{name}",
        neoforge::artifact(loader)
    )
}

pub struct NeoForgeServer;

#[async_trait]
impl ServerProvider for NeoForgeServer {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }
    fn summary(&self) -> &'static str {
        SUMMARY
    }
    fn loads(&self) -> Loads {
        Some(ContentKind::Mod)
    }

    async fn versions(&self) -> Result<Vec<GameVersion>> {
        game_versions().await
    }

    async fn loader_versions(&self, game: &str) -> Result<Vec<String>> {
        builds(game).await
    }

    async fn resolve(&self, request: &ResolveRequest) -> Result<ServerProfile> {
        let loader = resolve_loader(request).await?;
        let base = mojang::version_json(&request.version).await?;
        Ok(ServerProfile {
            flavor: ID.to_string(),
            game_version: request.version.clone(),
            launch: ServerLaunch::ArgsFile(server_args_file(&loader)),
            loader_version: Some(loader),
            jar: ServerJar::Download(mojang::server_artifact(&base)?),
            java_major: mojang::java_major(&base),
            jvm_args: Vec::new(),
        })
    }

    async fn install(&self, request: &InstallRequest<'_>, on: OnProgress<'_>) -> Result<()> {
        run_install(request, processors::Side::Server, on).await
    }
}

pub struct NeoForgeInstance;

#[async_trait]
impl InstanceProvider for NeoForgeInstance {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }
    fn summary(&self) -> &'static str {
        SUMMARY
    }
    fn loads(&self) -> Loads {
        Some(ContentKind::Mod)
    }

    async fn versions(&self) -> Result<Vec<GameVersion>> {
        game_versions().await
    }

    async fn loader_versions(&self, game: &str) -> Result<Vec<String>> {
        builds(game).await
    }

    async fn resolve(&self, request: &ResolveRequest) -> Result<InstanceProfile> {
        let loader = resolve_loader(request).await?;
        let base = mojang::version_json(&request.version).await?;
        let installer = {
            // Resolution is a catalogue read with nowhere to cache to, so the
            // installer is fetched into memory here and again (from disk) at
            // install time.
            let response = crate::net::get(
                proto::error::Service::NeoForge,
                &neoforge::installer_url(&loader),
            )
            .await
            .with_context(|| format!("cannot fetch the neoforge {loader} installer"))?;
            let bytes = response.bytes().await.map_err(|e| {
                crate::net::stream_failure(Some(proto::error::Service::NeoForge), &e)
            })?;
            neoforge::read_installer(&bytes)?
        };

        let libraries = super::fabric::merge_libraries(
            mojang::libraries(&base),
            mojang::libraries(&installer.version),
        );
        let mut jvm_args = mojang::jvm_args(&base);
        jvm_args.extend(mojang::jvm_args(&installer.version));
        let mut game_args = mojang::game_args(&base);
        game_args.extend(mojang::game_args(&installer.version));

        Ok(InstanceProfile {
            flavor: ID.to_string(),
            game_version: request.version.clone(),
            loader_version: Some(loader),
            client: mojang::client_artifact(&base)?,
            libraries,
            natives: mojang::natives(&base),
            asset_index: mojang::asset_index(&base)?,
            java_major: mojang::java_major(&base),
            main_class: mojang::main_class(&installer.version),
            jvm_args,
            game_args,
        })
    }

    async fn install(&self, request: &InstallRequest<'_>, on: OnProgress<'_>) -> Result<()> {
        run_install(request, processors::Side::Client, on).await
    }
}
