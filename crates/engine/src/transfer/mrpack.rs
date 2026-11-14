//! Exporting an instance as a Modrinth pack.
//!
//! A `.mrpack` is not really an instance — it is a *pack*: an index of files to
//! fetch, plus the configuration they need. Importing one is the modpack flow's
//! job ([`crate::content::pack`] detects and reads it); this module only writes
//! one.
//!
//! The export is a lossy projection, and deliberately so. A pack names its mods
//! by URL and hash, so only pool items hestia knows the origin of survive as
//! references; a local import has no URL to write, and rides along inside the
//! archive with a warning saying so.

use std::path::Path;

use anyhow::{Context, Result};
use proto::content::InstalledContent;
use proto::warning::WarningInfo;
use serde_json::json;

use super::archive::{self, Member, Writer, Written};
use super::exclude::Rules;
use super::{pool, Source};
use crate::cancel::Job;
use crate::checksum;
use crate::content::install;
use crate::content::pack::INDEX;
use crate::minecraft::profile::InstanceProfile;
use crate::minecraft::Loaders;

/// The load directories whose files a pack can name. `overrides/` members under
/// one of these are content the pack ships rather than configuration, which is
/// what makes them worth warning about on export.
const LOAD_DIRS: &[&str] = &["mods/", "resourcepacks/", "shaderpacks/", "plugins/"];

/// Write the instance out as a pack: an index of everything that can be named
/// by download, and an `overrides/` tree holding the rest of the game directory.
pub(crate) fn export(
    source: &Source<'_>,
    destination: &Path,
    loaders: &Loaders,
    job: &Job<'_>,
) -> Result<(Written, Vec<WarningInfo>)> {
    let rules = Rules::new(source.entry_dir, source.data_dir, source.exclude);
    let (files, embedded) = partition_pool(source.entry_dir, &rules)?;

    let mut members = archive::plan(source.data_dir, "overrides/", &|relative| {
        rules.keeps_in_data(relative)
    });
    members.extend(embedded);

    let index = serde_json::to_vec_pretty(&json!({
        "formatVersion": 1,
        "game": "minecraft",
        "versionId": common::utc_stamp(common::now_unix()),
        "name": source.record.name,
        "files": files,
        "dependencies": dependencies(&source.record.profile, loaders),
    }))
    .context("the pack index serializes")?;

    let mut writer = Writer::create(destination)?;
    writer.add_bytes(INDEX, &index)?;
    writer.add_all(&members, job)?;
    let written = writer.finish()?;
    Ok((written, embedded_warning(&members)))
}

/// Split the content pool into what the index can reference and what has to
/// travel inside the archive.
fn partition_pool(
    entry_dir: &Path,
    rules: &Rules,
) -> Result<(Vec<serde_json::Value>, Vec<Member>)> {
    let mut files = Vec::new();
    let mut embedded = Vec::new();
    for pooled in pool::exported(entry_dir, rules)? {
        match index_entry(&pooled.item, &pooled.managed, &pooled.path)? {
            Some(entry) => files.push(entry),
            None => embedded.push(Member {
                name: format!("overrides/{}", pooled.path),
                source: pooled.managed,
            }),
        }
    }
    Ok((files, embedded))
}

/// One `files[]` entry, or `None` when the item cannot be referenced — a local
/// import has no download, and a file with no recorded hash cannot be verified
/// by whoever installs the pack.
fn index_entry(
    item: &InstalledContent,
    managed: &Path,
    path: &str,
) -> Result<Option<serde_json::Value>> {
    if !install::referenceable(item) {
        return Ok(None);
    }
    let (size, sha512) = sha512_file(managed)?;
    // A disabled item is exported as optional rather than dropped: whoever
    // installs the pack should get the same choice, not a silently shorter mod
    // list. Prism's own exporter makes the same call for a `.disabled` file.
    let env = match item.enabled {
        true => json!({ "client": "required", "server": "required" }),
        false => json!({ "client": "optional", "server": "optional" }),
    };
    Ok(Some(json!({
        "path": path,
        "hashes": { "sha1": item.sha1, "sha512": sha512 },
        "env": env,
        "downloads": [item.url],
        "fileSize": size,
    })))
}

/// The pack index's `dependencies`: the game version, plus the loader under the
/// key the format names it by. Vanilla pins only Minecraft.
fn dependencies(profile: &InstanceProfile, loaders: &Loaders) -> serde_json::Value {
    let mut deps = serde_json::Map::new();
    deps.insert("minecraft".to_string(), json!(profile.game_version));
    let key = loaders.key_of(crate::content::pack::MRPACK, &profile.flavor);
    if let (Some(key), Some(version)) = (key, profile.loader_version.as_ref()) {
        deps.insert(key.to_string(), json!(version));
    }
    serde_json::Value::Object(deps)
}

/// Content that ended up inside the archive rather than referenced by it. The
/// export is complete and installs correctly; it is only no longer a pack
/// Modrinth would accept for publishing, which is worth saying once.
fn embedded_warning(members: &[Member]) -> Vec<WarningInfo> {
    let files: Vec<String> = members
        .iter()
        .filter_map(|m| m.name.strip_prefix("overrides/"))
        .filter(|name| LOAD_DIRS.iter().any(|dir| name.starts_with(dir)))
        .map(str::to_string)
        .collect();
    match files.is_empty() {
        true => Vec::new(),
        false => vec![WarningInfo::ExportFilesEmbedded {
            count: files.len() as u32,
            files,
        }],
    }
}

/// A file's size and SHA-512, in one read. Pack formats name their files by
/// SHA-512 beside the SHA-1 hestia stores, so it is hashed on demand rather
/// than kept — it is wanted once, at export.
pub(super) fn sha512_file(path: &Path) -> Result<(u64, String)> {
    checksum::digest_sized::<sha2::Sha512>(path)
        .with_context(|| format!("cannot hash {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loaders() -> Loaders {
        crate::minecraft::Minecraft::default().loaders()
    }

    fn managed_file(dir: &Path, name: &str) -> std::path::PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, "bytes").unwrap();
        path
    }

    #[test]
    fn dependencies_name_the_loader_the_pack_format_knows() {
        let vanilla = InstanceProfile {
            flavor: "vanilla".to_string(),
            game_version: "1.21.1".to_string(),
            ..Default::default()
        };
        assert_eq!(
            dependencies(&vanilla, &loaders()),
            json!({ "minecraft": "1.21.1" })
        );

        let fabric = InstanceProfile {
            flavor: "fabric".to_string(),
            game_version: "1.21.1".to_string(),
            loader_version: Some("0.16.9".to_string()),
            ..Default::default()
        };
        assert_eq!(
            dependencies(&fabric, &loaders()),
            json!({ "minecraft": "1.21.1", "fabric-loader": "0.16.9" })
        );

        let neoforge = InstanceProfile {
            flavor: "neoforge".to_string(),
            game_version: "1.21.1".to_string(),
            loader_version: Some("21.1.66".to_string()),
            ..Default::default()
        };
        assert_eq!(
            dependencies(&neoforge, &loaders()),
            json!({ "minecraft": "1.21.1", "neoforge": "21.1.66" })
        );
    }

    #[test]
    fn an_item_with_no_download_cannot_be_referenced() {
        let dir = tempfile::tempdir().unwrap();
        let managed = managed_file(dir.path(), "local.jar");
        let local = InstalledContent {
            source: "file".to_string(),
            filename: "local.jar".to_string(),
            sha1: "abc".to_string(),
            enabled: true,
            ..Default::default()
        };
        assert!(index_entry(&local, &managed, "mods/local.jar")
            .unwrap()
            .is_none());

        let tracked = InstalledContent {
            url: "https://cdn.modrinth.com/x.jar".to_string(),
            ..local
        };
        let entry = index_entry(&tracked, &managed, "mods/local.jar")
            .unwrap()
            .expect("a tracked item is a reference");
        assert_eq!(entry["path"], "mods/local.jar");
        assert_eq!(entry["fileSize"], 5);
        assert_eq!(entry["downloads"][0], "https://cdn.modrinth.com/x.jar");
        assert_eq!(entry["env"]["client"], "required");
        assert_eq!(
            entry["hashes"]["sha512"].as_str().unwrap().len(),
            128,
            "the format wants sha512 beside the sha1 hestia stores"
        );
    }

    #[test]
    fn only_embedded_content_warns_not_embedded_configuration() {
        let members = |names: &[&str]| -> Vec<Member> {
            names
                .iter()
                .map(|name| Member {
                    name: name.to_string(),
                    source: std::path::PathBuf::new(),
                })
                .collect()
        };
        assert!(embedded_warning(&members(&[
            "overrides/options.txt",
            "overrides/config/sodium.json",
        ]))
        .is_empty());

        let warnings = embedded_warning(&members(&[
            "overrides/options.txt",
            "overrides/mods/handmade.jar",
        ]));
        assert!(matches!(
            warnings.as_slice(),
            [WarningInfo::ExportFilesEmbedded { count: 1, .. }]
        ));
    }
}
