//! `hestia plugin publish`: a signed package added to a repository's index, and
//! the two other ways the index changes: a refresh, which signs it again for a
//! new life, and a revocation.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use clap::Args;
use kernel::vetting::Hosts;
use kernel::{Hplugin, Listing, Release, Revocation, Signer};
use sha2::{Digest, Sha256};

use super::inspect::{analyze, lock_of, Analysis};
use super::lock;
use super::repo::{Repository, INDEX, SIGNATURE};
use crate::ui::{self, View};

#[derive(Args)]
pub struct PublishArgs {
    /// The package `hestia plugin pack` wrote
    #[arg(
        required_unless_present_any = ["refresh", "revoke"],
        conflicts_with_all = ["refresh", "revoke"],
        requires = "url"
    )]
    pub(super) package: Option<PathBuf>,
    /// The repository directory
    #[arg(long, value_name = "DIR")]
    pub(super) repository: PathBuf,
    /// The repository's minisign secret key, which signs the index
    #[arg(long, value_name = "FILE")]
    pub(super) key: PathBuf,
    /// The https URL the package will be served at; its file name is the
    /// package's name in the repository directory
    #[arg(long, requires = "package")]
    pub(super) url: Option<String>,
    /// The author's minisign public key (the key or its file); needed the first
    /// time a plugin is published
    #[arg(long, value_name = "KEY", requires = "package")]
    pub(super) author: Option<String>,
    /// The repository's name, when it has no index yet
    #[arg(long)]
    pub(super) name: Option<String>,
    /// A host the index lists as serving packages, beyond those it lists
    /// already; the package URL's host must be one of them
    #[arg(long, value_name = "HOST", requires = "package")]
    pub(super) host: Vec<String>,
    /// Publish a version that asks for more than the last one did
    #[arg(long, requires = "package")]
    pub(super) allow_widen: bool,
    /// The project whose plugin.grant the package is held to
    #[arg(long, value_name = "DIR", default_value = ".", requires = "package")]
    pub(super) project: PathBuf,
    /// Sign the index again for a new life and a new sequence
    #[arg(long, conflicts_with = "revoke")]
    pub(super) refresh: bool,
    /// Revoke a published version, written `<plugin>@<version>`: clients
    /// disable it and never run it again
    #[arg(long, value_name = "PLUGIN@VERSION")]
    pub(super) revoke: Option<String>,
    /// Why it is revoked
    #[arg(long, requires = "revoke")]
    pub(super) reason: Option<String>,
}

/// The host and the file name of a release URL: https, a host, a path ending
/// in a name the repository directory can hold that is not the index's.
fn split(url: &str) -> Result<(&str, &str)> {
    let rest = url
        .strip_prefix("https://")
        .context("the package URL must be https")?;
    if url.contains(['?', '#']) {
        bail!("the package URL must not carry a query or a fragment");
    }
    let (authority, path) = rest
        .split_once('/')
        .with_context(|| format!("'{url}' does not end in a file name"))?;
    let host = authority.split(':').next().unwrap_or_default();
    let name = path.rsplit('/').next().unwrap_or_default();
    let reserved = [INDEX, SIGNATURE]
        .iter()
        .any(|r| name.eq_ignore_ascii_case(r));
    if host.is_empty()
        || name.is_empty()
        || name == ".."
        || name == "."
        || name.contains(['\\', ':'])
        || reserved
    {
        bail!("'{url}' does not end in a file name a repository can hold");
    }
    Ok((host, name))
}

fn author_key(text: &str) -> Result<Signer> {
    let text = std::fs::read_to_string(text).unwrap_or_else(|_| text.to_owned());
    Ok(Signer::parse(&text)?)
}

/// A package as Hestia derives it at install, for every platform.
fn described(archive: &Hplugin) -> Result<Analysis> {
    let analysis = analyze(&kernel::table(), &archive.manifest, &archive.wasm, false);
    if !analysis.errors.is_empty() {
        bail!("the package is not valid: {}", analysis.errors.join("; "));
    }
    Ok(analysis)
}

fn newest(listing: &Listing) -> Option<&Release> {
    listing
        .releases
        .iter()
        .filter_map(|release| Some((semver::Version::parse(&release.version).ok()?, release)))
        .max_by(|(a, _), (b, _)| a.cmp(b))
        .map(|(_, release)| release)
}

/// The last published version of `listing`, read from the repository and held
/// to the digest and size the signed index gives it, so a package swapped in
/// the directory cannot lower the widening gate.
fn previous(repository: &Repository, last: &Release) -> Result<Analysis> {
    let (_, name) = split(&last.url)?;
    let bytes = std::fs::read(repository.file(name)).with_context(|| {
        format!(
            "cannot compare with {}, the last published version",
            last.version
        )
    })?;
    if bytes.len() as u64 != last.size || hex::encode(Sha256::digest(&bytes)) != last.sha256 {
        bail!(
            "{name} is not the package the index lists for {}",
            last.version
        );
    }
    described(&Hplugin::read(&bytes)?)
}

fn add(args: &PublishArgs, package: &Path, url: &str, now: u64) -> Result<String> {
    let (host, name) = split(url)?;
    let bytes =
        std::fs::read(package).with_context(|| format!("cannot read {}", package.display()))?;
    let archive = Hplugin::read(&bytes)?;
    let analysis = described(&archive)?;
    let (Some(manifest), Some(grant), Some(built)) =
        (&analysis.manifest, &analysis.grant, lock_of(&analysis))
    else {
        bail!("the package is not valid");
    };
    if let Some(problem) = lock::mismatch(&args.project, &built) {
        bail!("{problem}");
    }
    let mut repository = Repository::open(&args.repository, &args.key, args.name.as_deref())?;
    for added in &args.host {
        Hosts::new(std::slice::from_ref(added), &[])
            .with_context(|| format!("'{added}' is not a host"))?;
        if !repository
            .index
            .hosts
            .iter()
            .any(|h| h.eq_ignore_ascii_case(added))
        {
            repository.index.hosts.push(added.to_ascii_lowercase());
        }
    }
    let served = Hosts::new(&repository.index.hosts, &[])?;
    if !served.reaches(&host.to_ascii_lowercase()) {
        bail!("the index does not list '{host}' as serving packages; pass --host {host} to add it");
    }
    let index = &mut repository.index;
    let held = index.plugins.iter().position(|l| l.id == manifest.id);
    let given = args.author.as_deref().map(author_key).transpose()?;
    let author = match (&given, held) {
        (Some(given), Some(at)) => {
            if !given.same_key(&Signer::parse(&index.plugins[at].author)?) {
                bail!("'{}' is listed under another author key", manifest.id);
            }
            given.clone()
        }
        (Some(given), None) => given.clone(),
        (None, Some(at)) => Signer::parse(&index.plugins[at].author)?,
        (None, None) => bail!("'{}' is new here; give its --author key", manifest.id),
    };
    archive
        .verify(&author, &manifest.id, &manifest.version)
        .context("the package is not signed by the author key")?;
    if let Some(at) = held {
        if index.plugins[at]
            .releases
            .iter()
            .any(|release| release.version == manifest.version)
        {
            bail!("{} {} is already published", manifest.id, manifest.version);
        }
    }
    let last = held.and_then(|at| newest(&index.plugins[at])).cloned();
    if let Some(last) = last {
        let before = previous(&repository, &last)?;
        let (Some(before_grant), Some(before_lock)) = (&before.grant, lock_of(&before)) else {
            bail!("the last published version is not valid");
        };
        if kernel::widens(before_grant, grant) && !args.allow_widen {
            bail!(
                "{} asks for more than {} (- {}, + {}):\n{}\npass --allow-widen to publish it; users will be asked again",
                manifest.version,
                last.version,
                last.version,
                manifest.version,
                lock::changes(&before_lock, &built)
            );
        }
    }
    let release = Release {
        version: manifest.version.clone(),
        url: url.to_owned(),
        sha256: hex::encode(Sha256::digest(&bytes)),
        size: bytes.len() as u64,
    };
    let index = &mut repository.index;
    match held {
        Some(at) => index.plugins[at].releases.push(release),
        None => index.plugins.push(Listing {
            id: manifest.id.clone(),
            author: author.key().to_owned(),
            releases: vec![release],
        }),
    }
    let sequence = repository.commit(now, &[(name, &bytes)])?;
    tracing::info!(plugin = %manifest.id, version = %manifest.version, sequence, "published");
    Ok(format!(
        "published {} {} as index sequence {sequence}",
        manifest.id, manifest.version
    ))
}

fn revoke(args: &PublishArgs, target: &str, now: u64) -> Result<String> {
    let (id, version) = target
        .split_once('@')
        .context("name the release as <plugin>@<version>")?;
    let mut repository = Repository::open(&args.repository, &args.key, args.name.as_deref())?;
    let index = &mut repository.index;
    let published = index
        .listing(id)
        .is_some_and(|listing| listing.releases.iter().any(|r| r.version == version));
    if !published {
        bail!("{id} {version} is not published here");
    }
    if index.revocation(id, version).is_none() {
        index.revoked.push(Revocation {
            id: id.to_owned(),
            version: version.to_owned(),
            reason: args.reason.clone().unwrap_or_default(),
        });
    }
    let sequence = repository.commit(now, &[])?;
    tracing::info!(plugin = id, version, sequence, "revoked");
    Ok(format!(
        "revoked {id} {version} as index sequence {sequence}"
    ))
}

/// Applies what `args` asks for to the repository as of `now` (unix seconds),
/// signs the new index and checks it verifies as a client would.
pub fn publish(args: &PublishArgs, now: u64) -> Result<String> {
    match (&args.package, &args.url, &args.revoke) {
        (Some(package), Some(url), _) => add(args, package, url, now),
        (_, _, Some(target)) => revoke(args, target, now),
        _ => {
            let repository = Repository::open(&args.repository, &args.key, args.name.as_deref())?;
            let sequence = repository.commit(now, &[])?;
            Ok(format!("refreshed the index as sequence {sequence}"))
        }
    }
}

pub fn run(args: PublishArgs) -> Result<()> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    ui::show(View::line(publish(&args, now)?))
}
