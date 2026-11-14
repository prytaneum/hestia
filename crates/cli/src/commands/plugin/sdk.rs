//! Building plugins against a local checkout of the SDK, without touching the
//! plugins' own files: cargo is handed the patch on its command line.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use clap::Args;

use super::repo::{CRATE_MANIFEST, PLUGINS_DIR};

pub const SDK_PACKAGE: &str = "hestia-sdk";
const CRATES_IO: &str = "crates-io";

/// The `--sdk` option of every command that compiles a plugin.
#[derive(Args, Clone, Debug, Default)]
pub struct SdkOption {
    /// Build against this local checkout of the SDK crate (the folder holding its
    /// Cargo.toml) instead of the version the plugins depend on
    #[arg(long = "sdk", value_name = "DIR", env = "HESTIA_SDK")]
    path: Option<PathBuf>,
}

/// A local SDK checkout that cargo is told to resolve `hestia-sdk` from.
#[derive(Debug, Clone)]
pub struct Sdk {
    dir: PathBuf,
}

impl Sdk {
    /// The checkout named by `--sdk` (or `$HESTIA_SDK`), checked to be the SDK
    /// crate; `None` when neither is set.
    pub fn resolve(option: &SdkOption) -> Result<Option<Sdk>> {
        let Some(path) = option
            .path
            .as_deref()
            .filter(|path| !path.as_os_str().is_empty())
        else {
            return Ok(None);
        };
        let dir = std::fs::canonicalize(path)
            .with_context(|| format!("--sdk {}: no such folder", path.display()))?;
        let text = std::fs::read_to_string(dir.join(CRATE_MANIFEST)).with_context(|| {
            format!(
                "--sdk {}: no {CRATE_MANIFEST} there; point at the folder of the {SDK_PACKAGE} crate (crates/sdk)",
                path.display()
            )
        })?;
        let name = text.parse::<toml::Table>().ok().and_then(|table| {
            table
                .get("package")?
                .get("name")?
                .as_str()
                .map(str::to_string)
        });
        if name.as_deref() != Some(SDK_PACKAGE) {
            bail!(
                "--sdk {}: {CRATE_MANIFEST} is not the {SDK_PACKAGE} crate (found {})",
                path.display(),
                name.as_deref().unwrap_or("no package name")
            );
        }
        tracing::info!(sdk = %dir.display(), "building against a local SDK");
        Ok(Some(Sdk { dir }))
    }

    #[cfg(test)]
    pub fn for_test(dir: &Path) -> Sdk {
        Sdk {
            dir: dir.to_path_buf(),
        }
    }

    /// The `--config` arguments that patch every source the repository's
    /// plugins take `hestia-sdk` from.
    pub fn config_args(&self, root: &Path) -> Vec<String> {
        patch_args(&plain_path(&self.dir), &sources(root))
    }
}

/// Cargo patch tables are keyed by source: `crates-io`, or the git URL a
/// plugin names. Only a source in use is patched, because cargo warns about a
/// patch that is not.
fn sources(root: &Path) -> Vec<String> {
    let mut manifests = vec![root.join(CRATE_MANIFEST)];
    if let Ok(entries) = std::fs::read_dir(root.join(PLUGINS_DIR)) {
        let mut members: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path().join(CRATE_MANIFEST))
            .collect();
        members.sort();
        manifests.extend(members);
    }
    let mut found = Vec::new();
    for manifest in manifests {
        let Some(table) = std::fs::read_to_string(&manifest)
            .ok()
            .and_then(|text| text.parse::<toml::Table>().ok())
        else {
            continue;
        };
        let workspace = table
            .get("workspace")
            .and_then(|table| table.get("dependencies"));
        for dependencies in [table.get("dependencies"), workspace].into_iter().flatten() {
            let Some(dependency) = dependencies.get(SDK_PACKAGE) else {
                continue;
            };
            let source = dependency
                .get("git")
                .and_then(toml::Value::as_str)
                .unwrap_or(CRATES_IO)
                .to_string();
            if !found.contains(&source) {
                found.push(source);
            }
        }
    }
    if found.is_empty() {
        found.push(CRATES_IO.to_string());
    }
    found
}

fn patch_args(dir: &str, sources: &[String]) -> Vec<String> {
    let path = quoted(dir);
    sources
        .iter()
        .flat_map(|source| {
            let key = if source == CRATES_IO {
                source.clone()
            } else {
                quoted(source)
            };
            [
                "--config".to_string(),
                format!("patch.{key}.{SDK_PACKAGE}.path={path}"),
            ]
        })
        .collect()
}

/// A TOML basic string: JSON's escapes are valid TOML ones.
fn quoted(text: &str) -> String {
    serde_json::Value::String(text.to_string()).to_string()
}

/// A canonicalised Windows path carries a `\\?\` prefix that cargo's config
/// does not need and that some tools mishandle.
fn plain_path(path: &Path) -> String {
    let text = path.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(rest) if rest.as_bytes().get(1) == Some(&b':') => rest.to_string(),
        _ => text.into_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn patch(dir: &str, sources: &[&str]) -> Vec<String> {
        let sources: Vec<String> = sources.iter().map(|source| source.to_string()).collect();
        patch_args(dir, &sources)
    }

    fn patched_path(args: &[String], source_key: &str) -> String {
        let table: toml::Table = args[1].parse().unwrap();
        table["patch"][source_key][SDK_PACKAGE]["path"]
            .as_str()
            .unwrap()
            .to_string()
    }

    #[test]
    fn a_registry_or_git_dependency_is_patched_under_its_source() {
        assert_eq!(
            patch("/work/hestia/crates/sdk", &["crates-io"]),
            [
                "--config",
                "patch.crates-io.hestia-sdk.path=\"/work/hestia/crates/sdk\""
            ]
        );
        assert_eq!(
            patch("/sdk", &["https://github.com/prytaneum/hestia"]),
            [
                "--config",
                "patch.\"https://github.com/prytaneum/hestia\".hestia-sdk.path=\"/sdk\""
            ]
        );
    }

    #[test]
    fn a_windows_path_and_one_with_quotes_survive_the_toml_round_trip() {
        for dir in [
            r"C:\Users\me\hestia\crates\sdk",
            r#"/home/me/my "sdk" copy/crates/sdk"#,
        ] {
            assert_eq!(patched_path(&patch(dir, &["crates-io"]), "crates-io"), dir);
        }
    }

    #[test]
    fn every_source_in_use_is_patched_once() {
        let args = patch("/sdk", &["crates-io", "https://example.com/fork"]);
        assert_eq!(args.iter().filter(|arg| *arg == "--config").count(), 2);
    }

    #[test]
    fn the_verbatim_prefix_of_a_canonical_windows_path_is_dropped() {
        assert_eq!(plain_path(Path::new(r"\\?\C:\sdk")), r"C:\sdk");
        assert_eq!(
            plain_path(Path::new(r"\\?\UNC\host\share")),
            r"\\?\UNC\host\share"
        );
        assert_eq!(plain_path(Path::new("/sdk")), "/sdk");
    }

    fn sdk_folder(name: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            format!("[package]\nname = \"{name}\"\nversion = \"1.0.0\"\n"),
        )
        .unwrap();
        dir
    }

    fn option(path: &Path) -> SdkOption {
        SdkOption {
            path: Some(path.to_path_buf()),
        }
    }

    #[test]
    fn the_sdk_folder_must_hold_the_sdk_crate() {
        let sdk = sdk_folder("hestia-sdk");
        assert!(Sdk::resolve(&option(sdk.path())).unwrap().is_some());
        assert!(Sdk::resolve(&SdkOption::default()).unwrap().is_none());

        let other = sdk_folder("something-else");
        let error = Sdk::resolve(&option(other.path())).unwrap_err();
        assert!(error.to_string().contains("not the hestia-sdk crate"));

        let empty = tempfile::tempdir().unwrap();
        let error = Sdk::resolve(&option(empty.path())).unwrap_err();
        assert!(error.to_string().contains("no Cargo.toml there"));

        let error = Sdk::resolve(&option(&empty.path().join("gone"))).unwrap_err();
        assert!(error.to_string().contains("no such folder"));
    }

    #[test]
    fn the_sources_come_from_what_the_plugins_declare() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("plugins/aa")).unwrap();
        std::fs::create_dir_all(root.path().join("plugins/bb")).unwrap();
        std::fs::write(
            root.path().join("Cargo.toml"),
            "[workspace]\nmembers = [\"plugins/*\"]\n",
        )
        .unwrap();
        std::fs::write(
            root.path().join("plugins/aa/Cargo.toml"),
            "[dependencies]\nhestia-sdk = { git = \"https://github.com/prytaneum/hestia\", rev = \"abc\" }\n",
        )
        .unwrap();
        std::fs::write(
            root.path().join("plugins/bb/Cargo.toml"),
            "[dependencies]\nhestia-sdk = \"1\"\n",
        )
        .unwrap();

        assert_eq!(
            sources(root.path()),
            ["https://github.com/prytaneum/hestia", "crates-io"]
        );
        assert_eq!(sources(tempfile::tempdir().unwrap().path()), ["crates-io"]);
    }
}
