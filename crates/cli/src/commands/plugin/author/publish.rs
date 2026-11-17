//! `hestia plugin publish`: a signed package added to a repository's index.
//!
//! A repository is a directory a web server serves: `index.json`, its
//! signature `index.json.minisig`, and the packages the index lists, each under
//! the file name of its URL.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use clap::Args;
use kernel::{Hplugin, Index, Listing, Release, Signer, LIFETIME_DAYS};
use sha2::{Digest, Sha256};

use super::inspect::analyze;
use super::key;
use super::lock;
use crate::ui::{self, View};

const INDEX: &str = "index.json";
const SIGNATURE: &str = "index.json.minisig";
const DAY: u64 = 24 * 3600;

#[derive(Args)]
pub struct PublishArgs {
    /// The package `hestia plugin pack --sign` wrote
    pub(super) package: PathBuf,
    /// The repository directory
    #[arg(long, value_name = "DIR")]
    pub(super) repository: PathBuf,
    /// The repository's minisign secret key, which signs the index
    #[arg(long, value_name = "FILE")]
    pub(super) key: PathBuf,
    /// The https URL the package will be served at; its file name is the
    /// package's name in the repository directory
    #[arg(long)]
    pub(super) url: String,
    /// The author's minisign public key (the key or its file); needed the first
    /// time a plugin is published
    #[arg(long, value_name = "KEY")]
    pub(super) author: Option<String>,
    /// The repository's name, when it has no index yet
    #[arg(long)]
    pub(super) name: Option<String>,
    /// Publish a version that asks for more than the last one did
    #[arg(long)]
    pub(super) allow_widen: bool,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn file_name(url: &str) -> Result<&str> {
    let name = url.rsplit('/').next().unwrap_or_default();
    match name.is_empty() || name.contains(['\\', ':']) || name == ".." {
        true => bail!("'{url}' does not end in a file name"),
        false => Ok(name),
    }
}

fn author_key(text: &str) -> Result<Signer> {
    let text = std::fs::read_to_string(text).unwrap_or_else(|_| text.to_owned());
    Ok(Signer::parse(&text)?)
}

/// A package's manifest and grant, as Hestia derives them at install.
fn described(archive: &Hplugin) -> Result<(kernel::Manifest, proto::plugin::Grant)> {
    let analysis = analyze(&kernel::table(), &archive.manifest, &archive.wasm, false);
    if !analysis.errors.is_empty() {
        bail!("the package is not valid: {}", analysis.errors.join("; "));
    }
    match (analysis.manifest, analysis.grant) {
        (Some(manifest), Some(grant)) => Ok((manifest, grant)),
        _ => bail!("the package is not valid"),
    }
}

fn read_index(path: &Path, name: Option<&str>) -> Result<Index> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .with_context(|| format!("{} is not an index", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Index {
            repository: name
                .context("the repository has no index yet; give its --name")?
                .to_owned(),
            sequence: 0,
            expires: 0,
            hosts: Vec::new(),
            plugins: Vec::new(),
            revoked: Vec::new(),
        }),
        Err(error) => Err(error).with_context(|| format!("cannot read {}", path.display())),
    }
}

fn newest(listing: &Listing) -> Option<&Release> {
    listing
        .releases
        .iter()
        .filter_map(|release| Some((semver::Version::parse(&release.version).ok()?, release)))
        .max_by(|(a, _), (b, _)| a.cmp(b))
        .map(|(_, release)| release)
}

/// Adds `package` to the repository's index as of `now` (unix seconds), signs
/// the new index and checks it verifies as a client would verify it.
pub fn publish(args: &PublishArgs, now: u64) -> Result<String> {
    if !args.url.starts_with("https://") {
        bail!("the package URL must be https");
    }
    let name = file_name(&args.url)?;
    let bytes = std::fs::read(&args.package)
        .with_context(|| format!("cannot read {}", args.package.display()))?;
    let archive = Hplugin::read(&bytes)?;
    let (manifest, grant) = described(&archive)?;
    let index_path = args.repository.join(INDEX);
    let mut index = read_index(&index_path, args.name.as_deref())?;
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
    if let Some(last) = held.and_then(|at| newest(&index.plugins[at])) {
        let previous =
            std::fs::read(args.repository.join(file_name(&last.url)?)).with_context(|| {
                format!(
                    "cannot compare with {}, the last published version",
                    last.version
                )
            })?;
        let (_, before) = described(&Hplugin::read(&previous)?)?;
        if kernel::widens(&before, &grant) && !args.allow_widen {
            bail!(
                "{} asks for more than {} (- {}, + {}):\n{}\npass --allow-widen to publish it; users will be asked again",
                manifest.version,
                last.version,
                last.version,
                manifest.version,
                lock::changes(&before, &grant)
            );
        }
    }
    let release = Release {
        version: manifest.version.clone(),
        url: args.url.clone(),
        sha256: hex(&Sha256::digest(&bytes)),
        size: bytes.len() as u64,
    };
    match held {
        Some(at) => index.plugins[at].releases.push(release),
        None => index.plugins.push(Listing {
            id: manifest.id.clone(),
            author: author.key().to_owned(),
            releases: vec![release],
        }),
    }
    index.sequence += 1;
    index.expires = now + (LIFETIME_DAYS - 1) * DAY;
    let body = serde_json::to_vec_pretty(&index)?;
    let secret = key::load(&args.key)?;
    let signature = key::sign(
        &secret,
        &body,
        &format!(
            "hestia-index {} {} {}",
            index.repository, index.sequence, index.expires
        ),
    )?;
    Index::verify(
        &body,
        &signature,
        &key::signer(&secret)?,
        &index.repository,
        index.sequence,
        now,
    )
    .context("the new index would not be accepted")?;
    let write = |file: &str, bytes: &[u8]| {
        common::atomic::write(&args.repository.join(file), bytes)
            .with_context(|| format!("cannot write {file}"))
    };
    write(name, &bytes)?;
    write(INDEX, &body)?;
    write(SIGNATURE, signature.as_bytes())?;
    tracing::info!(plugin = %manifest.id, version = %manifest.version, sequence = index.sequence, "published");
    Ok(format!(
        "published {} {} as index sequence {}",
        manifest.id, manifest.version, index.sequence
    ))
}

pub fn run(args: PublishArgs) -> Result<()> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    ui::show(View::line(publish(&args, now)?))
}
