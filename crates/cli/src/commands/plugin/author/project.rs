//! A plugin project on disk: its files, and how its component is built.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};
use clap::Args;

pub const MANIFEST: &str = "plugin.toml";
pub const COMPONENT: &str = "plugin.wasm";
pub const LOCK: &str = "plugin.grant";
const TARGET: &str = "wasm32-wasip2";

/// Which project to act on, and whether to build it.
#[derive(Args, Clone)]
pub struct Where {
    /// The project directory
    #[arg(default_value = ".")]
    pub dir: PathBuf,
    /// Use this built component instead of building one
    #[arg(long, value_name = "FILE")]
    pub wasm: Option<PathBuf>,
}

/// A project's manifest and component, read.
pub struct Source {
    pub dir: PathBuf,
    pub manifest: Vec<u8>,
    pub wasm: Vec<u8>,
}

impl Where {
    pub fn load(&self) -> Result<Source> {
        let dir = std::fs::canonicalize(&self.dir)
            .with_context(|| format!("cannot read {}", self.dir.display()))?;
        let manifest = read(&dir.join(MANIFEST))?;
        let wasm = match &self.wasm {
            Some(file) => read(file)?,
            None => read(&build(&dir, Profile::Release)?)?,
        };
        Ok(Source {
            dir,
            manifest,
            wasm,
        })
    }
}

fn read(path: &Path) -> Result<Vec<u8>> {
    std::fs::read(path).with_context(|| format!("cannot read {}", path.display()))
}

/// How `build` compiles: what a package ships, or what `dev` rebuilds on every
/// save, without the LTO and the stripping that cost time and the names a
/// backtrace needs.
#[derive(Clone, Copy)]
pub enum Profile {
    Release,
    Dev,
}

/// `cargo` run in `dir`, with the toolchain of the project there: the variables
/// a `cargo run` of Hestia exports would pin it to Hestia's own.
pub fn cargo(dir: &Path) -> Command {
    let mut cargo = Command::new("cargo");
    cargo
        .current_dir(dir)
        .env_remove("CARGO")
        .env_remove("RUSTUP_TOOLCHAIN");
    cargo
}

/// Builds the project for `wasm32-wasip2` and copies the component beside its
/// manifest, where a dev install and a package read it.
pub fn build(dir: &Path, profile: Profile) -> Result<PathBuf> {
    tracing::info!(dir = %dir.display(), "building the plugin");
    let mut command = cargo(dir);
    command.args([
        "build",
        "--release",
        "--target",
        TARGET,
        "--message-format=json-render-diagnostics",
    ]);
    if matches!(profile, Profile::Dev) {
        command.args([
            "--config",
            "profile.release.lto=false",
            "--config",
            "profile.release.strip=false",
        ]);
    }
    let mut cargo = command
        .stdout(Stdio::piped())
        .spawn()
        .context("cannot run cargo")?;
    let manifest_path = std::fs::canonicalize(dir.join("Cargo.toml"))?;
    let mut built = None;
    let stdout = cargo.stdout.take().context("cargo has no output")?;
    for line in BufReader::new(stdout).lines() {
        let Ok(message) = serde_json::from_str::<serde_json::Value>(&line?) else {
            continue;
        };
        let ours = message["manifest_path"]
            .as_str()
            .and_then(|path| std::fs::canonicalize(path).ok())
            .is_some_and(|path| path == manifest_path);
        let cdylib = message["target"]["kind"]
            .as_array()
            .is_some_and(|kinds| kinds.iter().any(|kind| kind == "cdylib"));
        let file = message["filenames"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|name| name.as_str())
            .find(|name| name.ends_with(".wasm"));
        if let (true, true, Some(file)) = (ours, cdylib, file) {
            built = Some(PathBuf::from(file));
        }
    }
    if !cargo.wait()?.success() {
        bail!("the build failed");
    }
    let built = built.with_context(|| format!("the build produced no .wasm for {TARGET}"))?;
    let component = dir.join(COMPONENT);
    std::fs::copy(&built, &component)
        .with_context(|| format!("cannot copy {} to {}", built.display(), component.display()))?;
    Ok(component)
}
