//! Exporting an instance through a pack format a plugin contributes. The
//! guest is handed the instance's description, the game directory's files and
//! the content pool, and answers the manifests to store and which of those
//! files to carry; the host vets the answer and does the writing, so a plugin
//! never opens the instance or the archive.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use proto::provider::pack::{PackInstance, PoolItem, WriteQuery};
use proto::warning::WarningInfo;

use super::archive::{self, Member, Writer, Written};
use super::exclude::Rules;
use super::pool::Pooled;
use super::provider::PackProvider;
use super::{loader_of, pool, Source};
use crate::cancel::Job;
use crate::checksum;
use crate::content::install;
use crate::minecraft::Loaders;

/// A file's size and SHA-512, in one read. Pack formats name their files by
/// SHA-512 beside the SHA-1 hestia stores, so it is hashed on demand rather
/// than kept — it is wanted once, at export.
fn sha512_file(path: &Path) -> Result<(u64, String)> {
    checksum::digest_sized::<sha2::Sha512>(path)
        .with_context(|| format!("cannot hash {}", path.display()))
}

fn pool_item(pooled: &Pooled) -> Result<PoolItem> {
    let referenceable = install::referenceable(&pooled.item);
    let (size, sha512) = match referenceable {
        true => sha512_file(&pooled.managed)?,
        false => (0, String::new()),
    };
    Ok(PoolItem {
        path: pooled.path.clone(),
        referenceable,
        size,
        sha512,
        item: pooled.item.clone(),
    })
}

pub(crate) async fn export(
    source: &Source<'_>,
    pack: &dyn PackProvider,
    destination: &Path,
    loaders: &Loaders,
    job: &Job<'_>,
) -> Result<(Written, Vec<WarningInfo>)> {
    let rules = Rules::new(source.entry_dir, source.data_dir, source.exclude);
    let pooled = pool::exported(source.entry_dir, &rules)?;
    let in_pool: HashSet<&str> = pooled.iter().map(|item| item.path.as_str()).collect();

    let mut sources: HashMap<String, PathBuf> = HashMap::new();
    for member in archive::plan(source.data_dir, "", &|relative| {
        rules.keeps_in_data(relative)
    }) {
        if !in_pool.contains(member.name.as_str()) {
            sources.insert(member.name, member.source);
        }
    }
    let mut files: Vec<String> = sources.keys().cloned().collect();
    files.sort();

    let profile = &source.record.profile;
    let query = WriteQuery {
        instance: PackInstance {
            name: source.record.name.clone(),
            game_version: profile.game_version.clone(),
            loader: loaders
                .key_of(pack.id(), &profile.flavor)
                .map(str::to_string)
                .unwrap_or_else(|| loader_of(profile)),
            loader_version: profile.loader_version.clone().unwrap_or_default(),
        },
        files,
        content: pooled.iter().map(pool_item).collect::<Result<Vec<_>>>()?,
    };
    let written = pack.write(&query).await?;

    sources.extend(pooled.into_iter().map(|item| (item.path, item.managed)));
    let copies: Vec<Member> = written
        .copies
        .iter()
        .filter_map(|copy| {
            Some(Member {
                name: copy.archive_path.clone(),
                source: sources.get(&copy.entry_path)?.clone(),
            })
        })
        .collect();

    let mut writer = Writer::create(destination)?;
    for member in &written.members {
        writer.add_bytes(&member.path, member.text.as_bytes())?;
    }
    writer.add_all(&copies, job)?;
    let archive = writer.finish()?;
    tracing::info!(pack = %pack.id(), files = archive.files, "pack exported");

    let warnings = written
        .warnings
        .into_iter()
        .map(|detail| WarningInfo::PackExportWarned {
            pack: pack.info().name.clone(),
            detail,
        })
        .collect();
    Ok((archive, warnings))
}
