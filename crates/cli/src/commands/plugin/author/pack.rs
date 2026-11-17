//! `hestia plugin pack`: the `.hplugin` a repository serves.

use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use clap::Args;
use kernel::{Hplugin, Table};
use minisign::SecretKey;

use super::check::report;
use super::inspect::{api_of, review};
use super::key;
use super::project::{Source, Where, MANIFEST};
use crate::ui::{self, View};

#[derive(Args)]
pub struct PackArgs {
    #[command(flatten)]
    source: Where,
    /// The author's minisign secret key, which signs the package
    #[arg(long, value_name = "FILE")]
    key: PathBuf,
    /// Where to write the package; by default `<id>-<version>.hplugin` in the project
    #[arg(long, value_name = "FILE")]
    out: Option<PathBuf>,
}

#[derive(Debug)]
pub struct Packed {
    pub archive: Vec<u8>,
    /// The manifest the package carries: the project's, with `api` written.
    pub manifest: Vec<u8>,
    pub file_name: String,
}

/// `api = "<version>"` among the manifest's top-level keys, which end at the
/// first table.
fn with_api(manifest: &[u8], api: &str) -> Result<Vec<u8>> {
    let text = std::str::from_utf8(manifest)?;
    let at = text
        .match_indices('\n')
        .map(|(newline, _)| newline + 1)
        .chain(std::iter::once(0))
        .filter(|&start| text[start..].starts_with('['))
        .min()
        .unwrap_or(text.len());
    let (top, tables) = text.split_at(at);
    let separator = if top.is_empty() || top.ends_with('\n') {
        ""
    } else {
        "\n"
    };
    let written = format!("{top}{separator}api = {api:?}\n{tables}");
    match kernel::Manifest::parse(&written)?.api.as_deref() == Some(api) {
        true => Ok(written.into_bytes()),
        false => bail!("cannot write `api` into {MANIFEST}; add `api = \"{api}\"` yourself"),
    }
}

/// The package of a project: checked against its lockfile, which refuses the
/// interfaces that are still unstable, `api` written, and signed by `author`.
pub fn package(table: &Table, source: &Source, author: &SecretKey) -> Result<Packed> {
    let analysis = review(table, source, false, false);
    report(&analysis)?;
    let (Some(manifest), Some(shape)) = (&analysis.manifest, &analysis.shape) else {
        bail!("nothing to pack");
    };
    let bytes = match (&manifest.api, api_of(shape)) {
        (None, Some(api)) => with_api(&source.manifest, &api)?,
        _ => source.manifest.clone(),
    };
    let signature = key::sign(
        author,
        &kernel::statement(&manifest.id, &manifest.version, &bytes, &source.wasm),
        "hestia-hplugin",
    )?;
    let archive = Hplugin::pack(&bytes, &source.wasm, &signature)?;
    Hplugin::read(&archive)?
        .verify(&key::signer(author)?, &manifest.id, &manifest.version)
        .context("the package does not verify under the key it was signed with")?;
    Ok(Packed {
        archive,
        manifest: bytes,
        file_name: format!("{}-{}.hplugin", manifest.id, manifest.version),
    })
}

pub fn run(args: PackArgs) -> Result<()> {
    let source = args.source.load()?;
    let author = key::load(&args.key)?;
    let packed = package(&kernel::table(), &source, &author)?;
    if packed.manifest != source.manifest {
        common::atomic::write(&source.dir.join(MANIFEST), &packed.manifest)
            .context("cannot write plugin.toml")?;
    }
    let out = args
        .out
        .unwrap_or_else(|| source.dir.join(&packed.file_name));
    common::atomic::write(&out, &packed.archive)
        .with_context(|| format!("cannot write {}", out.display()))?;
    ui::show(View::line(format!("wrote {} (signed)", out.display())))
}
