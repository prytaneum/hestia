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
            None => read(&build(&dir)?)?,
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

/// Builds the project for `wasm32-wasip2` and copies the component beside its
/// manifest, where a dev install and a package read it.
pub fn build(dir: &Path) -> Result<PathBuf> {
    tracing::info!(dir = %dir.display(), "building the plugin");
    let mut cargo = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .args([
            "build",
            "--release",
            "--target",
            TARGET,
            "--message-format=json-render-diagnostics",
        ])
        .current_dir(dir)
        .stdout(Stdio::piped())
        .spawn()
        .context("cannot run cargo")?;
    let manifest_path = dir.join("Cargo.toml");
    let mut built = None;
    let stdout = cargo.stdout.take().context("cargo has no output")?;
    for line in BufReader::new(stdout).lines() {
        let Ok(message) = serde_json::from_str::<serde_json::Value>(&line?) else {
            continue;
        };
        let ours = message["manifest_path"].as_str().map(Path::new) == Some(&manifest_path);
        let file = message["filenames"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|name| name.as_str())
            .find(|name| name.ends_with(".wasm"));
        if let (true, Some(file)) = (ours, file) {
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
