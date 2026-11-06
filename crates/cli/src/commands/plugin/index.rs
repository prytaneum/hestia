use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};
use proto::plugin::Manifest;
use proto::repository::{Build, IndexedPlugin, RepositoryIndex, INDEX_FILE, INDEX_SCHEMA_VERSION};
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;

use super::check::{self, Checked};
use super::repo::{MANIFEST, PLUGINS_DIR};
use crate::ui::{self, View};

const DIST: &str = "dist";
const MAX_PREVIOUS_BYTES: usize = 16 * 1024 * 1024;

/// A packed plugin about to join the index.
struct Package {
    manifest: Manifest,
    sha256: String,
}

/// Where the source lives and which commit the listing was built from.
struct Source {
    repository: String,
    commit: String,
}

/// Regenerates `dist/hestia-index.json` from the working tree, keeping the
/// previous index's builds and revocations and adding the given packages.
pub async fn run(
    previous: Option<&str>,
    add: &[PathBuf],
    repository: Option<String>,
) -> Result<()> {
    let repo = check::find(None)?;
    let checked = check::check(&repo)?;
    let previous = match previous {
        Some(location) => Some(read_previous(location).await?),
        None => None,
    };
    let packages = add
        .iter()
        .map(|path| read_package(path))
        .collect::<Result<Vec<_>>>()?;
    let source = Source {
        repository: match repository {
            Some(url) => normalize_repository(&url)?,
            None => normalize_repository(&git(repo.root(), &["remote", "get-url", "origin"])?)?,
        },
        commit: git(repo.root(), &["rev-parse", "HEAD"])?,
    };
    let index = build_index(&checked, &source, previous, packages)?;

    let dist = repo.root().join(DIST);
    std::fs::create_dir_all(&dist)?;
    let target = dist.join(INDEX_FILE);
    write_index(&dist, &target, &index)?;
    tracing::info!(
        version = index.version,
        plugins = index.plugins.len(),
        "index written"
    );
    ui::show(View::line(format!(
        "wrote {} (version {}, {} plugin(s))",
        target
            .strip_prefix(repo.root())
            .unwrap_or(&target)
            .display(),
        index.version,
        index.plugins.len()
    )))
}

fn build_index(
    checked: &Checked,
    source: &Source,
    previous: Option<RepositoryIndex>,
    packages: Vec<Package>,
) -> Result<RepositoryIndex> {
    let previous = previous.unwrap_or_default();
    if previous.schema_version > INDEX_SCHEMA_VERSION {
        bail!(
            "the previous index has schema {}, newer than this build understands ({INDEX_SCHEMA_VERSION})",
            previous.schema_version
        );
    }
    let mut plugins: Vec<IndexedPlugin> = checked
        .plugins
        .iter()
        .map(|plugin| {
            let manifest = &plugin.manifest;
            let icon = "assets/icon.png".to_string();
            IndexedPlugin {
                id: manifest.id.clone(),
                name: manifest.name.clone(),
                description: manifest.description.clone(),
                icon: plugin.path.join(&icon).is_file().then_some(icon),
                path: format!("{PLUGINS_DIR}/{}", plugin.dir_name),
                version: manifest.version.clone(),
                permissions: manifest.permissions.clone(),
                commit: source.commit.clone(),
                builds: previous
                    .plugins
                    .iter()
                    .find(|old| old.id == manifest.id)
                    .map(|old| old.builds.clone())
                    .unwrap_or_default(),
            }
        })
        .collect();

    for package in packages {
        let manifest = &package.manifest;
        let plugin = plugins
            .iter_mut()
            .find(|plugin| plugin.id == manifest.id)
            .with_context(|| {
                format!(
                    "package '{}' is not a plugin of this repository",
                    manifest.id
                )
            })?;
        let build = Build {
            version: manifest.version.clone(),
            api: manifest.api.clone(),
            url: format!(
                "{}/releases/download/{id}-v{version}/{id}-{version}.hplugin",
                source.repository,
                id = manifest.id,
                version = manifest.version
            ),
            sha256: package.sha256,
        };
        match plugin
            .builds
            .iter_mut()
            .find(|existing| existing.version == build.version)
        {
            Some(existing) if existing.sha256 != build.sha256 => bail!(
                "'{}' {} is already built with a different sha256; a released version is never rebuilt, bump the version",
                manifest.id,
                build.version
            ),
            Some(existing) => *existing = build,
            None => plugin.builds.push(build),
        }
    }

    Ok(RepositoryIndex {
        schema_version: INDEX_SCHEMA_VERSION,
        version: previous.version + 1,
        repository: source.repository.clone(),
        commit: source.commit.clone(),
        name: checked.meta.name.clone(),
        label: checked.meta.label.clone(),
        website: checked.meta.website.clone(),
        key: checked.meta.key.clone(),
        plugins,
        revoked: previous.revoked,
    })
}

fn read_package(path: &Path) -> Result<Package> {
    let bytes = std::fs::read(path).with_context(|| format!("cannot read {}", path.display()))?;
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(&bytes))
        .with_context(|| format!("{} is not a .hplugin package", path.display()))?;
    let mut text = String::new();
    archive
        .by_name(MANIFEST)
        .with_context(|| format!("{} has no {MANIFEST}", path.display()))?
        .read_to_string(&mut text)?;
    let manifest: Manifest = toml::from_str(&text)
        .with_context(|| format!("invalid {MANIFEST} in {}", path.display()))?;
    manifest
        .validate()
        .with_context(|| format!("invalid {MANIFEST} in {}", path.display()))?;
    Ok(Package {
        manifest,
        sha256: Sha256::digest(&bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
    })
}

async fn read_previous(location: &str) -> Result<RepositoryIndex> {
    let bytes = if location.contains("://") {
        if !location.starts_with("https://") {
            bail!("--previous must be an https URL or a file");
        }
        let mut response = reqwest::Client::builder()
            .https_only(true)
            .build()?
            .get(location)
            .send()
            .await
            .with_context(|| format!("cannot fetch {location}"))?
            .error_for_status()
            .with_context(|| format!("cannot fetch {location}"))?;
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            if bytes.len() + chunk.len() > MAX_PREVIOUS_BYTES {
                bail!("the previous index is larger than {MAX_PREVIOUS_BYTES} bytes");
            }
            bytes.extend_from_slice(&chunk);
        }
        bytes
    } else {
        std::fs::read(location).with_context(|| format!("cannot read {location}"))?
    };
    serde_json::from_slice(&bytes).with_context(|| format!("{location} is not a Hestia index"))
}

fn write_index(dist: &Path, target: &Path, index: &RepositoryIndex) -> Result<()> {
    let mut temp = NamedTempFile::new_in(dist)?;
    serde_json::to_writer_pretty(temp.as_file_mut(), index)?;
    temp.persist(target)?;
    Ok(())
}

fn git(root: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .context("git was not found on PATH")?;
    if !output.status.success() {
        bail!(
            "`git {}` failed; run inside a git repository with an `origin` remote, or pass --repository",
            args.join(" ")
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// An https repository URL without a trailing `.git` or slash; an ssh remote
/// of the `git@host:owner/repo` form is rewritten to https.
fn normalize_repository(remote: &str) -> Result<String> {
    let https = match remote
        .strip_prefix("git@")
        .and_then(|rest| rest.split_once(':'))
    {
        Some((host, path)) => format!("https://{host}/{path}"),
        None => remote.to_string(),
    };
    let url = https.trim_end_matches('/').trim_end_matches(".git");
    if !url.starts_with("https://") {
        bail!("the repository URL must be https, found `{remote}`; pass --repository");
    }
    Ok(url.to_string())
}

#[cfg(test)]
mod tests {
    use super::super::check::tests::{add_plugin, repo_with, valid_meta};
    use super::*;

    fn source() -> Source {
        Source {
            repository: "https://github.com/o/r".to_string(),
            commit: "abc".to_string(),
        }
    }

    fn package(id: &str, version: &str, sha256: &str) -> Package {
        Package {
            manifest: Manifest {
                id: id.to_string(),
                version: version.to_string(),
                api: Some("^1.0.0".to_string()),
                ..Manifest::default()
            },
            sha256: sha256.to_string(),
        }
    }

    fn checked() -> (tempfile::TempDir, Checked) {
        let (root, repo) = repo_with(valid_meta(), &[]);
        add_plugin(&repo, "aa", "aa", true);
        add_plugin(&repo, "bb", "bb", true);
        let checked = check::check(&repo).unwrap();
        (root, checked)
    }

    #[test]
    fn builds_and_revocations_carry_over_and_the_version_bumps() {
        let (_root, checked) = checked();
        let first = build_index(
            &checked,
            &source(),
            None,
            vec![package("aa", "0.1.0", "aa")],
        )
        .unwrap();
        assert_eq!(first.version, 1);
        let build = &first.plugins[0].builds[0];
        assert_eq!(
            build.url,
            "https://github.com/o/r/releases/download/aa-v0.1.0/aa-0.1.0.hplugin"
        );

        let mut previous = first;
        previous.revoked.push(proto::repository::Revocation {
            id: "aa".to_string(),
            versions: vec!["0.0.1".to_string()],
            reason: "bad".to_string(),
        });
        let second = build_index(
            &checked,
            &source(),
            Some(previous),
            vec![package("bb", "0.1.0", "bb")],
        )
        .unwrap();

        assert_eq!(second.version, 2);
        assert_eq!(second.plugins[0].builds.len(), 1);
        assert_eq!(second.plugins[1].builds[0].sha256, "bb");
        assert_eq!(second.revoked.len(), 1);
    }

    #[test]
    fn a_rebuilt_version_with_another_hash_is_refused() {
        let (_root, checked) = checked();
        let first = build_index(
            &checked,
            &source(),
            None,
            vec![package("aa", "0.1.0", "aa")],
        )
        .unwrap();

        let same = build_index(
            &checked,
            &source(),
            Some(first.clone()),
            vec![package("aa", "0.1.0", "aa")],
        )
        .unwrap();
        assert_eq!(same.plugins[0].builds.len(), 1);

        let error = build_index(
            &checked,
            &source(),
            Some(first),
            vec![package("aa", "0.1.0", "zz")],
        )
        .unwrap_err();
        assert!(error.to_string().contains("different sha256"));
    }

    #[test]
    fn a_package_of_another_repository_is_refused() {
        let (_root, checked) = checked();

        assert!(build_index(
            &checked,
            &source(),
            None,
            vec![package("zzz", "0.1.0", "aa")]
        )
        .is_err());
    }

    #[test]
    fn remotes_normalize_to_https() {
        assert_eq!(
            normalize_repository("git@github.com:o/r.git").unwrap(),
            "https://github.com/o/r"
        );
        assert_eq!(
            normalize_repository("https://github.com/o/r.git/").unwrap(),
            "https://github.com/o/r"
        );
        assert!(normalize_repository("http://github.com/o/r").is_err());
    }
}
