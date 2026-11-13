//! Building plugin components and reading what cargo resolved for them.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};
use serde_json::Value;

use super::sdk::{Sdk, SDK_PACKAGE};

const TARGET: &str = "wasm32-wasip2";
const LOCKFILE: &str = "Cargo.lock";

/// A built component and the SDK it was built against.
#[derive(Clone)]
pub struct Component {
    pub wasm: PathBuf,
    pub sdk_version: String,
}

pub struct Metadata(Value);

impl Metadata {
    pub fn read(root: &Path, sdk: Option<&Sdk>) -> Result<Metadata> {
        let _lock = LockGuard::hold(root, sdk);
        let output = cargo(root, sdk)
            .args(["metadata", "--format-version", "1"])
            .stderr(Stdio::inherit())
            .output()
            .map_err(spawn_error)?;
        if !output.status.success() {
            bail!("cargo metadata failed");
        }
        Metadata::parse(&output.stdout)
    }

    fn parse(json: &[u8]) -> Result<Metadata> {
        Ok(Metadata(
            serde_json::from_slice(json).context("unreadable cargo metadata")?,
        ))
    }

    /// The package whose manifest sits in `crate_dir`.
    pub fn package_name(&self, crate_dir: &Path) -> Result<&str> {
        self.package_in(crate_dir)
            .and_then(|package| package["name"].as_str())
            .with_context(|| format!("{} is not a member of the workspace", crate_dir.display()))
    }

    /// Where the release build of the crate in `crate_dir` leaves its
    /// component, and the `hestia-sdk` version it resolved.
    pub fn component(&self, crate_dir: &Path) -> Result<Component> {
        let package = self
            .package_in(crate_dir)
            .with_context(|| format!("{} is not a member of the workspace", crate_dir.display()))?;
        let library = package["targets"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|target| has_crate_type(target, "cdylib"))
            .and_then(|target| target["name"].as_str())
            .with_context(|| format!("{} has no cdylib target", crate_dir.display()))?;
        let target_directory = self.0["target_directory"]
            .as_str()
            .context("cargo metadata has no target_directory")?;
        let sdk_version = self
            .sdk_version(package["id"].as_str().unwrap_or_default())
            .with_context(|| format!("{} does not depend on {SDK_PACKAGE}", crate_dir.display()))?;
        Ok(Component {
            wasm: Path::new(target_directory)
                .join(TARGET)
                .join("release")
                .join(format!("{}.wasm", library.replace('-', "_"))),
            sdk_version,
        })
    }

    fn package_in(&self, crate_dir: &Path) -> Option<&Value> {
        self.0["packages"].as_array()?.iter().find(|package| {
            package["manifest_path"]
                .as_str()
                .and_then(|path| Path::new(path).parent())
                == Some(crate_dir)
        })
    }

    fn sdk_version(&self, package_id: &str) -> Option<String> {
        let node = self.0["resolve"]["nodes"]
            .as_array()?
            .iter()
            .find(|node| node["id"] == package_id)?;
        let packages = self.0["packages"].as_array()?;
        node["deps"]
            .as_array()?
            .iter()
            .filter_map(|dep| packages.iter().find(|package| package["id"] == dep["pkg"]))
            .find(|package| package["name"] == SDK_PACKAGE)
            .and_then(|package| package["version"].as_str())
            .map(str::to_string)
    }
}

fn has_crate_type(target: &Value, wanted: &str) -> bool {
    target["crate_types"]
        .as_array()
        .is_some_and(|types| types.iter().any(|kind| kind == wanted))
}

/// The caret requirement a plugin built against `sdk_version` writes into its
/// manifest. The SDK version is the WIT package version, which the host's API
/// is checked against; a pre-release suffix does not change the ABI.
pub fn api_requirement(sdk_version: &str) -> String {
    let core = sdk_version.split(['-', '+']).next().unwrap_or(sdk_version);
    format!("^{core}")
}

/// Builds the release component of each named package, cargo's own output
/// going to the terminal.
pub fn build(root: &Path, packages: &[&str], sdk: Option<&Sdk>) -> Result<()> {
    ensure_target(root)?;
    let _lock = LockGuard::hold(root, sdk);
    tracing::info!(?packages, "building plugin components");
    let status = build_command(root, packages, sdk)
        .status()
        .map_err(spawn_error)?;
    if !status.success() {
        bail!("cargo build failed");
    }
    Ok(())
}

/// Like [`build`], but cargo's diagnostics come back in the error instead of
/// reaching the terminal.
pub fn build_captured(root: &Path, packages: &[&str], sdk: Option<&Sdk>) -> Result<()> {
    ensure_target(root)?;
    let _lock = LockGuard::hold(root, sdk);
    tracing::info!(?packages, "building plugin components");
    let output = build_command(root, packages, sdk)
        .args(["--message-format", "short", "--color", "never"])
        .output()
        .map_err(spawn_error)?;
    if !output.status.success() {
        bail!("{}", diagnostics(&String::from_utf8_lossy(&output.stderr)));
    }
    Ok(())
}

fn build_command(root: &Path, packages: &[&str], sdk: Option<&Sdk>) -> Command {
    let mut command = cargo(root, sdk);
    command.args(["build", "--release", "--target", TARGET]);
    for package in packages {
        command.args(["-p", package]);
    }
    command
}

/// The lines that explain a failure; cargo's progress lines are noise.
fn diagnostics(stderr: &str) -> String {
    const PROGRESS: [&str; 6] = [
        "Compiling",
        "Finished",
        "Downloading",
        "Downloaded",
        "Locking",
        "Updating",
    ];
    let lines: Vec<&str> = stderr
        .lines()
        .filter(|line| {
            let line = line.trim_start();
            !line.is_empty() && !PROGRESS.iter().any(|word| line.starts_with(word))
        })
        .collect();
    if lines.is_empty() {
        "cargo build failed".to_string()
    } else {
        lines.join("\n")
    }
}

/// Patching changes what cargo resolves, so cargo rewrites `Cargo.lock`; a
/// plugin's files must come out of an `--sdk` run as they went in.
struct LockGuard {
    path: PathBuf,
    original: Option<Option<Vec<u8>>>,
}

impl LockGuard {
    fn hold(root: &Path, sdk: Option<&Sdk>) -> LockGuard {
        let path = root.join(LOCKFILE);
        let original = sdk.map(|_| std::fs::read(&path).ok());
        LockGuard { path, original }
    }
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        let restored = match self.original.take() {
            None => return,
            Some(Some(bytes)) => std::fs::write(&self.path, bytes),
            Some(None) => match std::fs::remove_file(&self.path) {
                Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
                other => other,
            },
        };
        if let Err(error) = restored {
            tracing::warn!(%error, "could not restore Cargo.lock after the --sdk build");
        }
    }
}

fn ensure_target(root: &Path) -> Result<()> {
    let output = Command::new("rustc")
        .current_dir(root)
        .args(["--print", "sysroot"])
        .output()
        .map_err(spawn_error)?;
    let sysroot = String::from_utf8_lossy(&output.stdout);
    let installed = Path::new(sysroot.trim())
        .join("lib")
        .join("rustlib")
        .join(TARGET)
        .is_dir();
    if !installed {
        bail!("the {TARGET} target is not installed; run `rustup target add {TARGET}`");
    }
    Ok(())
}

fn cargo(root: &Path, sdk: Option<&Sdk>) -> Command {
    let mut command = Command::new("cargo");
    command.current_dir(root);
    if let Some(sdk) = sdk {
        command.args(sdk.config_args(root));
    }
    command
}

fn spawn_error(error: std::io::Error) -> anyhow::Error {
    if error.kind() == ErrorKind::NotFound {
        anyhow::anyhow!(
            "cargo was not found on PATH; install Rust from https://rustup.rs, then run `rustup target add {TARGET}`"
        )
    } else {
        error.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_explanation_of_a_failed_build_is_kept() {
        let stderr = "   Compiling plug v0.1.0\nsrc/lib.rs:3:5: error[E0425]: cannot find value `x`\nerror: could not compile `plug`\n";
        assert_eq!(
            diagnostics(stderr),
            "src/lib.rs:3:5: error[E0425]: cannot find value `x`\nerror: could not compile `plug`"
        );
        assert_eq!(diagnostics("   Compiling a\n"), "cargo build failed");
    }

    #[test]
    fn the_lockfile_is_put_back_after_an_sdk_build() {
        let root = tempfile::tempdir().unwrap();
        let lock = root.path().join(LOCKFILE);
        let sdk = Sdk::for_test(root.path());

        std::fs::write(&lock, "original").unwrap();
        {
            let _guard = LockGuard::hold(root.path(), Some(&sdk));
            std::fs::write(&lock, "rewritten by cargo").unwrap();
        }
        assert_eq!(std::fs::read_to_string(&lock).unwrap(), "original");

        std::fs::remove_file(&lock).unwrap();
        {
            let _guard = LockGuard::hold(root.path(), Some(&sdk));
            std::fs::write(&lock, "created by cargo").unwrap();
        }
        assert!(!lock.exists());

        {
            let _guard = LockGuard::hold(root.path(), None);
            std::fs::write(&lock, "kept").unwrap();
        }
        assert_eq!(std::fs::read_to_string(&lock).unwrap(), "kept");
    }

    #[test]
    fn the_sdk_version_is_the_one_this_plugin_resolved() {
        let json = serde_json::json!({
            "target_directory": "/work/target",
            "packages": [
                {"id": "plug 0.1.0", "name": "plug", "version": "0.1.0",
                 "manifest_path": "/work/plugins/plug/Cargo.toml",
                 "targets": [{"name": "plug-x", "crate_types": ["cdylib"]}]},
                {"id": "sdk 1.0.0", "name": "hestia-sdk", "version": "1.0.0", "manifest_path": "/a/Cargo.toml", "targets": []},
                {"id": "sdk 0.9.0", "name": "hestia-sdk", "version": "0.9.0", "manifest_path": "/b/Cargo.toml", "targets": []},
                {"id": "other 2.0.0", "name": "other", "version": "2.0.0", "manifest_path": "/c/Cargo.toml", "targets": []}
            ],
            "resolve": {"nodes": [
                {"id": "plug 0.1.0", "deps": [{"pkg": "other 2.0.0"}, {"pkg": "sdk 1.0.0"}]},
                {"id": "other 2.0.0", "deps": [{"pkg": "sdk 0.9.0"}]}
            ]}
        });
        let metadata = Metadata::parse(json.to_string().as_bytes()).unwrap();

        let component = metadata.component(Path::new("/work/plugins/plug")).unwrap();

        assert_eq!(component.sdk_version, "1.0.0");
        assert_eq!(
            component.wasm,
            Path::new("/work/target/wasm32-wasip2/release/plug_x.wasm")
        );
    }
}
