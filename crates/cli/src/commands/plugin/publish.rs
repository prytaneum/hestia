use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};
use proto::plugin::Manifest;
use proto::repository::{
    Build, IndexedPlugin, RepositoryIndex, Revocation, INDEX_FILE, INDEX_LIFETIME_SECONDS,
    INDEX_SCHEMA_VERSION,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tempfile::NamedTempFile;

use super::check::{self, Checked};
use super::pack;
use super::repo::{PluginDir, MANIFEST, PLUGINS_DIR};
use super::sign::Signer;
use crate::ui::{self, View};

const PACKAGES_DIR: &str = "packages";
const PACKAGE_EXTENSION: &str = "hplugin";
const REVOKED_FILE: &str = "revoked.toml";

/// A package already in the output directory, as its own bytes describe it.
struct Package {
    manifest: Manifest,
    sha256: String,
}

/// Where the source lives, which commit the listing was built from, and when
/// it is signed.
struct Source {
    repository: String,
    commit: String,
    signed_unix: u64,
}

/// What a publish run changed.
#[derive(Debug)]
struct Outcome {
    built: Vec<String>,
    index: RepositoryIndex,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RevokedFile {
    revoked: Vec<Revocation>,
}

/// Builds the plugins whose version has no package in `<out>/packages/` yet,
/// then regenerates and signs `<out>/plugins.json` from the packages there, the
/// repository's sources and `revoked.toml`.
pub fn run(out: &Path, sign: &Path, repository: Option<String>) -> Result<()> {
    let repo = check::find(None)?;
    let checked = check::check(&repo)?;
    let signer = Signer::load(sign, &checked.meta.key)?;
    let source = Source {
        repository: match repository {
            Some(url) => normalize_repository(&url)?,
            None => normalize_repository(&git(repo.root(), &["remote", "get-url", "origin"])?)?,
        },
        commit: git(repo.root(), &["rev-parse", "HEAD"])?,
        signed_unix: common::now_unix().max(0) as u64,
    };
    let revoked = read_revoked(repo.root())?;
    let root = repo.root().to_path_buf();
    let outcome = publish(
        &checked,
        out,
        &signer,
        &source,
        revoked,
        |plugins, packages| pack::build(&root, plugins, packages),
    )?;
    tracing::info!(
        version = outcome.index.version,
        built = outcome.built.len(),
        plugins = outcome.index.plugins.len(),
        "repository published"
    );
    for built in &outcome.built {
        ui::show(View::line(format!("built {built}")))?;
    }
    ui::show(View::line(format!(
        "wrote {} (version {}, {} plugin(s), {} built)",
        out.join(INDEX_FILE).display(),
        outcome.index.version,
        outcome.index.plugins.len(),
        outcome.built.len()
    )))
}

fn publish(
    checked: &Checked,
    out: &Path,
    signer: &Signer,
    source: &Source,
    revoked: Vec<Revocation>,
    build: impl FnOnce(&[PluginDir], &Path) -> Result<Vec<PathBuf>>,
) -> Result<Outcome> {
    let packages_dir = out.join(PACKAGES_DIR);
    std::fs::create_dir_all(&packages_dir)?;
    let previous = read_previous(out)?;
    verify_published(out, &previous)?;

    let published = read_packages(&packages_dir)?;
    let unpublished: Vec<_> = checked
        .plugins
        .iter()
        .filter(|plugin| {
            !published.iter().any(|package| {
                package.manifest.id == plugin.manifest.id
                    && package.manifest.version == plugin.manifest.version
            })
        })
        .cloned()
        .collect();
    let mut built = Vec::new();
    if !unpublished.is_empty() {
        let paths = build(&unpublished, &packages_dir)?;
        for (plugin, path) in unpublished.iter().zip(paths) {
            pack::sign_package(signer, &plugin.manifest, &path)?;
            built.push(format!(
                "{}-{}",
                plugin.manifest.id, plugin.manifest.version
            ));
        }
    }

    let packages = read_packages(&packages_dir)?;
    let index = build_index(checked, source, previous.version, &packages, revoked)?;
    write_index(out, &index, signer)?;
    Ok(Outcome { built, index })
}

/// A published version is never rebuilt, so the index on the branch must still
/// describe the bytes beside it; anything else was edited after signing.
fn verify_published(out: &Path, previous: &RepositoryIndex) -> Result<()> {
    for plugin in &previous.plugins {
        for build in &plugin.builds {
            let path = out.join(&build.file);
            let bytes = std::fs::read(&path).with_context(|| {
                format!(
                    "{} is listed in {INDEX_FILE} but cannot be read",
                    path.display()
                )
            })?;
            if sha256_hex(&bytes) != build.sha256 {
                bail!(
                    "'{}' {} no longer matches the sha256 in {INDEX_FILE}; a published version is never rebuilt or replaced, bump the version",
                    plugin.id,
                    build.version
                );
            }
        }
    }
    Ok(())
}

fn build_index(
    checked: &Checked,
    source: &Source,
    previous_version: u64,
    packages: &[Package],
    revoked: Vec<Revocation>,
) -> Result<RepositoryIndex> {
    let version = source.signed_unix.max(previous_version + 1);
    let plugins = checked
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
                builds: packages
                    .iter()
                    .filter(|package| package.manifest.id == manifest.id)
                    .map(|package| Build {
                        version: package.manifest.version.clone(),
                        api: package.manifest.api.clone(),
                        file: format!(
                            "{PACKAGES_DIR}/{}-{}.{PACKAGE_EXTENSION}",
                            package.manifest.id, package.manifest.version
                        ),
                        sha256: package.sha256.clone(),
                    })
                    .collect(),
            }
        })
        .collect();

    Ok(RepositoryIndex {
        schema_version: INDEX_SCHEMA_VERSION,
        version,
        expires: version as i64 + INDEX_LIFETIME_SECONDS,
        repository: source.repository.clone(),
        commit: source.commit.clone(),
        name: checked.meta.name.clone(),
        label: checked.meta.label.clone(),
        website: checked.meta.website.clone(),
        key: checked.meta.key.clone(),
        next: checked.meta.next.clone(),
        plugins,
        revoked,
    })
}

fn read_packages(dir: &Path) -> Result<Vec<Package>> {
    let mut paths = std::fs::read_dir(dir)
        .with_context(|| format!("cannot read {}", dir.display()))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    paths.retain(|path| path.extension().is_some_and(|ext| ext == PACKAGE_EXTENSION));
    paths.sort();
    paths.iter().map(|path| read_package(path)).collect()
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
        sha256: sha256_hex(&bytes),
    })
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn read_previous(out: &Path) -> Result<RepositoryIndex> {
    let path = out.join(INDEX_FILE);
    if !path.is_file() {
        return Ok(RepositoryIndex::default());
    }
    let previous: RepositoryIndex = serde_json::from_slice(&std::fs::read(&path)?)
        .with_context(|| format!("{} is not a Hestia index", path.display()))?;
    if previous.schema_version > INDEX_SCHEMA_VERSION {
        bail!(
            "{} has schema {}, newer than this build understands ({INDEX_SCHEMA_VERSION})",
            path.display(),
            previous.schema_version
        );
    }
    Ok(previous)
}

fn read_revoked(root: &Path) -> Result<Vec<Revocation>> {
    let path = root.join(REVOKED_FILE);
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let file: RevokedFile = toml::from_str(&std::fs::read_to_string(&path)?)
        .with_context(|| format!("invalid {REVOKED_FILE}"))?;
    Ok(file.revoked)
}

fn write_index(out: &Path, index: &RepositoryIndex, signer: &Signer) -> Result<()> {
    let target = out.join(INDEX_FILE);
    let mut temp = NamedTempFile::new_in(out)?;
    serde_json::to_writer_pretty(temp.as_file_mut(), index)?;
    temp.persist(&target)?;
    signer.write(
        &std::fs::read(&target)?,
        &proto::repository::index_comment(index.version, index.expires),
        &target,
    )?;
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
    use minisign::{KeyPair, SignatureBox};

    use super::super::check::tests::{add_plugin, repo_with};
    use super::super::repo::Repository;
    use super::*;

    struct Fixture {
        _root: tempfile::TempDir,
        repo: Repository,
        out: tempfile::TempDir,
        signer: Signer,
        public: minisign::PublicKey,
    }

    fn fixture() -> Fixture {
        let pair = KeyPair::generate_unencrypted_keypair().unwrap();
        let meta = format!(
            "\n[workspace.metadata.hestia]\nname = \"Sample\"\nlabel = \"SMP\"\nwebsite = \"https://example.com\"\nkey = \"{}\"\n",
            pair.pk.to_base64()
        );
        let (root, repo) = repo_with(&meta, &[]);
        add_plugin(&repo, "aa", "aa", true);
        add_plugin(&repo, "bb", "bb", true);
        let key_file = root.path().join("hestia.key");
        std::fs::write(&key_file, pair.sk.to_box(None).unwrap().to_string()).unwrap();
        let signer = Signer::load(&key_file, &pair.pk.to_base64()).unwrap();
        Fixture {
            _root: root,
            repo,
            out: tempfile::tempdir().unwrap(),
            signer,
            public: pair.pk,
        }
    }

    fn source(signed_unix: u64) -> Source {
        Source {
            repository: "https://github.com/o/r".to_string(),
            commit: "abc".to_string(),
            signed_unix,
        }
    }

    fn run_publish(fixture: &Fixture, signed_unix: u64) -> Result<Outcome> {
        let checked = check::check(&fixture.repo)?;
        publish(
            &checked,
            fixture.out.path(),
            &fixture.signer,
            &source(signed_unix),
            read_revoked(fixture.repo.root())?,
            |plugins, packages| {
                plugins
                    .iter()
                    .map(|plugin| pack::pack_plugin(plugin, None, packages))
                    .collect()
            },
        )
    }

    fn bump(fixture: &Fixture, id: &str, version: &str) {
        std::fs::write(
            fixture.repo.plugin_path(id).join(MANIFEST),
            format!("id = \"{id}\"\nname = \"{id}\"\nversion = \"{version}\"\n"),
        )
        .unwrap();
    }

    #[test]
    fn the_first_publish_builds_everything_and_signs_it() {
        let fixture = fixture();

        let outcome = run_publish(&fixture, 1_700_000_000).unwrap();

        assert_eq!(outcome.built, ["aa-0.1.0", "bb-0.1.0"]);
        assert_eq!(outcome.index.version, 1_700_000_000);
        assert_eq!(
            outcome.index.expires,
            1_700_000_000 + INDEX_LIFETIME_SECONDS
        );
        let build = &outcome.index.plugins[0].builds[0];
        assert_eq!(build.file, "packages/aa-0.1.0.hplugin");
        let out = fixture.out.path();
        assert!(out.join("packages/aa-0.1.0.hplugin.minisig").is_file());
        let signature = SignatureBox::from_file(out.join("plugins.json.minisig")).unwrap();
        minisign::verify(
            &fixture.public,
            &signature,
            std::io::Cursor::new(std::fs::read(out.join("plugins.json")).unwrap()),
            true,
            false,
            false,
        )
        .unwrap();
        assert_eq!(
            signature.trusted_comment().unwrap(),
            proto::repository::index_comment(outcome.index.version, outcome.index.expires)
        );
    }

    #[test]
    fn publishing_again_builds_nothing_and_only_re_signs() {
        let fixture = fixture();
        let first = run_publish(&fixture, 1_700_000_000).unwrap();
        let package = fixture.out.path().join("packages/aa-0.1.0.hplugin");
        let before = std::fs::read(&package).unwrap();

        let second = run_publish(&fixture, 1_700_000_100).unwrap();

        assert!(second.built.is_empty());
        assert_eq!(second.index.version, 1_700_000_100);
        assert_eq!(second.index.expires, first.index.expires + 100);
        assert_eq!(second.index.plugins, first.index.plugins);
        assert_eq!(std::fs::read(&package).unwrap(), before);
    }

    #[test]
    fn a_version_bump_builds_only_that_plugin_and_keeps_the_old_build() {
        let fixture = fixture();
        run_publish(&fixture, 1_700_000_000).unwrap();
        bump(&fixture, "aa", "0.2.0");

        let outcome = run_publish(&fixture, 1_700_000_100).unwrap();

        assert_eq!(outcome.built, ["aa-0.2.0"]);
        let aa = &outcome.index.plugins[0];
        assert_eq!(aa.version, "0.2.0");
        assert_eq!(aa.builds.len(), 2);
        assert_eq!(outcome.index.plugins[1].builds.len(), 1);
    }

    #[test]
    fn a_tampered_published_package_is_refused() {
        let fixture = fixture();
        run_publish(&fixture, 1_700_000_000).unwrap();
        let package = fixture.out.path().join("packages/aa-0.1.0.hplugin");
        let mut bytes = std::fs::read(&package).unwrap();
        bytes.push(0);
        std::fs::write(&package, bytes).unwrap();

        let error = run_publish(&fixture, 1_700_000_100).unwrap_err();

        assert!(error.to_string().contains("no longer matches"));
    }

    #[test]
    fn revoked_toml_lands_in_the_index() {
        let fixture = fixture();
        std::fs::write(
            fixture.repo.root().join(REVOKED_FILE),
            "[[revoked]]\nid = \"aa\"\nversions = [\"0.0.1\"]\nreason = \"bad\"\n",
        )
        .unwrap();

        let outcome = run_publish(&fixture, 1_700_000_000).unwrap();

        assert_eq!(
            outcome.index.revoked,
            [Revocation {
                id: "aa".to_string(),
                versions: vec!["0.0.1".to_string()],
                reason: "bad".to_string(),
            }]
        );
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
