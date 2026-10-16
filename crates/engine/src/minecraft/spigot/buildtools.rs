//! Compiling a Bukkit or Spigot server with SpigotMC's BuildTools: it clones
//! the upstream repositories, decompiles the vanilla server, applies the
//! CraftBukkit and Spigot patch sets and compiles the result.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use proto::error::ErrorInfo;
use proto::minecraft::ProvisionPhase;

use super::super::materialize::{self, OnProgress};
use super::super::meta::spigot;
use super::super::provider::InstallRequest;
use crate::download::Downloader;
use crate::process::Task;

const TOOLS_JAR: &str = "BuildTools.jar";

/// Which of the two jars one build produces. Both come out of the same run, so
/// a flavor names the one it launches and inherits the other for free.
#[derive(Clone, Copy)]
pub enum Product {
    Bukkit,
    Spigot,
}

impl Product {
    pub fn jar_name(self, version: &str) -> String {
        format!("{}-{version}.jar", self.prefix())
    }

    fn prefix(self) -> &'static str {
        match self {
            Product::Bukkit => "craftbukkit",
            Product::Spigot => "spigot",
        }
    }
}

/// Put this product's jar in the entry's directory, building it first if no
/// build has produced it yet.
pub async fn ensure(
    product: Product,
    request: &InstallRequest<'_>,
    on_progress: OnProgress<'_>,
) -> Result<()> {
    materialize::validate_filename(request.game_version)?;
    let target = request.root.join(product.jar_name(request.game_version));
    if target.is_file() {
        return Ok(());
    }

    let jars = jars_dir(request.meta, request.game_version);
    if built(&jars, product).is_none() {
        build(product, request, on_progress).await?;
    }
    let built = built(&jars, product).with_context(|| {
        format!(
            "the build produced no {} jar for Minecraft {}",
            product.prefix(),
            request.game_version
        )
    })?;
    std::fs::copy(&built, &target).with_context(|| format!("cannot place {}", target.display()))?;
    Ok(())
}

/// One tree shared by every version and both flavors: BuildTools keeps the
/// upstream clones and its own maven repository here and reuses them.
fn work_dir(meta: &Path) -> PathBuf {
    meta.join("spigot")
}

fn jars_dir(meta: &Path, version: &str) -> PathBuf {
    work_dir(meta).join("jars").join(version)
}

/// Found by prefix rather than exact name: BuildTools names its output after
/// the version *its* metadata reports, not always the string asked for.
fn built(jars: &Path, product: Product) -> Option<PathBuf> {
    let prefix = format!("{}-", product.prefix());
    std::fs::read_dir(jars).ok()?.flatten().find_map(|entry| {
        let name = entry.file_name().to_string_lossy().into_owned();
        (name.starts_with(&prefix) && name.ends_with(".jar")).then(|| entry.path())
    })
}

async fn build(
    product: Product,
    request: &InstallRequest<'_>,
    on_progress: OnProgress<'_>,
) -> Result<()> {
    ensure_prerequisites(product).await?;
    let work = work_dir(request.meta);
    let jars = jars_dir(request.meta, request.game_version);
    std::fs::create_dir_all(&jars).with_context(|| format!("cannot create {}", jars.display()))?;

    let tools = work.join(TOOLS_JAR);
    if !tools.is_file() {
        // No published checksum, so a truncated jar fails the run below rather
        // than being trusted.
        Downloader::new(request.cache)
            .fetch(spigot::BUILDTOOLS_URL, &tools, None, &|_| Ok(()))
            .await
            .context("cannot download BuildTools")?;
    }

    on_progress.check()?;
    run(request, &tools, &work, &jars, on_progress).await
}

/// Refuse before the long work starts when a tool the build shells out to is
/// missing, naming where to get it — the launcher cannot install it.
async fn ensure_prerequisites(product: Product) -> Result<()> {
    for prerequisite in super::PREREQUISITES {
        if !prerequisite.installed().await {
            bail!(ErrorInfo::MissingRequirement {
                flavor: product.prefix().to_string(),
                name: prerequisite.name().to_string(),
                url: prerequisite.url().to_string(),
            });
        }
    }
    Ok(())
}

/// The supervisor id a version's build runs under. Deriving it from the version
/// rather than allocating one means two creates racing on the same version — or
/// a create after a daemon restart — join the build already running instead of
/// starting a second.
fn build_id(version: &str) -> String {
    format!("build-spigot-{version}")
}

async fn run(
    request: &InstallRequest<'_>,
    tools: &Path,
    work: &Path,
    jars: &Path,
    on_progress: OnProgress<'_>,
) -> Result<()> {
    let version = request.game_version;
    tracing::info!(version, "compiling bukkit and spigot with BuildTools");
    let outcome = request
        .processes
        .run(
            Task {
                id: build_id(version),
                program: request.java,
                args: vec![
                    "-jar".into(),
                    tools.to_string_lossy().into_owned(),
                    "--rev".into(),
                    version.to_string(),
                    "--output-dir".into(),
                    jars.to_string_lossy().into_owned(),
                    // Both, always: the run costs the same and it leaves the
                    // other flavor on this version free.
                    "--compile".into(),
                    "CRAFTBUKKIT".into(),
                    "--compile".into(),
                    "SPIGOT".into(),
                    "--nogui".into(),
                ],
                cwd: work.to_path_buf(),
                phase: ProvisionPhase::Server,
                narrates: is_step,
                deadline: None,
            },
            on_progress,
        )
        .await?;
    if !outcome.succeeded() {
        bail!("the Minecraft {version} build failed");
    }
    tracing::info!(version, "bukkit and spigot built");
    Ok(())
}

/// Whether a line is BuildTools narrating its own progress rather than passing
/// maven's, git's or the decompiler's through: a build emits tens of thousands
/// of lines, most of them bracketed levels or one per patched file.
fn is_step(line: &str) -> bool {
    let line = line.trim();
    !line.is_empty()
        && !line.starts_with('[')
        && !line.starts_with("Patching ")
        && !line.starts_with("Extracted:")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_product_names_the_jar_buildtools_writes() {
        assert_eq!(Product::Spigot.jar_name("1.21.4"), "spigot-1.21.4.jar");
        assert_eq!(Product::Bukkit.jar_name("1.21.4"), "craftbukkit-1.21.4.jar");
    }

    #[test]
    fn only_buildtools_own_narration_is_reported() {
        assert!(is_step("Compiling Spigot & Spigot-API"));
        assert!(is_step("Applying CraftBukkit Patches"));
        assert!(!is_step("[INFO] Building Spigot-API 1.21.4-R0.1-SNAPSHOT"));
        assert!(!is_step(
            "Patching net/minecraft/server/MinecraftServer.java"
        ));
        assert!(!is_step(""));
    }

    #[test]
    fn a_version_maps_to_one_build_id() {
        assert_eq!(build_id("1.21.4"), "build-spigot-1.21.4");
    }
}
