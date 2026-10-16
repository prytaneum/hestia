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
use proto::content::{ContentKind, InstalledContent};
use proto::warning::WarningInfo;
use serde_json::json;

use super::archive::{self, Member, Writer, Written};
use super::exclude::Rules;
use super::Source;
use crate::cancel::Job;
use crate::checksum::Hasher;
use crate::content::install;
use crate::content::pack::INDEX;
use crate::minecraft::profile::InstanceProfile;
use crate::registry;

/// The load directories whose files a pack can name. `overrides/` members under
/// one of these are content the pack ships rather than configuration, which is
/// what makes them worth warning about on export.
const LOAD_DIRS: &[&str] = &["mods/", "resourcepacks/", "shaderpacks/", "plugins/"];

/// Write the instance out as a pack: an index of everything that can be named
/// by download, and an `overrides/` tree holding the rest of the game directory.
pub(crate) fn export(
    source: &Source<'_>,
    destination: &Path,
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
        "versionId": registry::utc_stamp(registry::now_unix()),
        "name": source.record.name,
        "files": files,
        "dependencies": dependencies(&source.record.profile),
    }))
    .context("the pack index serializes")?;

    let mut writer = Writer::create(destination)?;
    writer.add_bytes(INDEX, &index)?;
    writer.add_all(&members, job)?;
    let written = writer.finish()?;
    Ok((written, embedded_warning(&members)))
}

/// Split the content pool into what the index can reference and what has to
/// travel inside the archive. A pool item the caller excluded is in neither:
/// an exclusion names an entry-relative path, and it has to mean the same
/// thing whichever format is being written.
fn partition_pool(
    entry_dir: &Path,
    rules: &Rules,
) -> Result<(Vec<serde_json::Value>, Vec<Member>)> {
    let mut files = Vec::new();
    let mut embedded = Vec::new();
    for item in &install::load(entry_dir) {
        // A datapack lives inside a world, not in a load directory the pack
        // format can name; it travels with `saves/` in the overrides instead.
        if item.kind == ContentKind::DataPack {
            continue;
        }
        let Ok(managed) = install::managed_path(entry_dir, item) else {
            continue;
        };
        if !managed.is_file() {
            continue;
        }
        let path = format!("{}/{}", install::kind_dir(item.kind)?, item.filename);
        if !rules.keeps(&path) {
            continue;
        }
        match index_entry(item, &managed, &path)? {
            Some(entry) => files.push(entry),
            None => embedded.push(Member {
                name: format!("overrides/{path}"),
                source: managed,
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
    let size = std::fs::metadata(managed)
        .with_context(|| format!("cannot stat {}", managed.display()))?
        .len();
    // A disabled item is exported as optional rather than dropped: whoever
    // installs the pack should get the same choice, not a silently shorter mod
    // list. Prism's own exporter makes the same call for a `.disabled` file.
    let env = match item.enabled {
        true => json!({ "client": "required", "server": "required" }),
        false => json!({ "client": "optional", "server": "optional" }),
    };
    Ok(Some(json!({
        "path": path,
        "hashes": { "sha1": item.sha1, "sha512": sha512_file(managed)? },
        "env": env,
        "downloads": [item.url],
        "fileSize": size,
    })))
}

/// The pack index's `dependencies`: the game version, plus the loader under the
/// key the format names it by. Vanilla pins only Minecraft.
fn dependencies(profile: &InstanceProfile) -> serde_json::Value {
    let mut deps = serde_json::Map::new();
    deps.insert("minecraft".to_string(), json!(profile.game_version));
    let key = match profile.flavor.as_str() {
        "fabric" => Some("fabric-loader"),
        "quilt" => Some("quilt-loader"),
        "neoforge" => Some("neoforge"),
        "forge" => Some("forge"),
        _ => None,
    };
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

/// The pack format names its files by SHA-512 beside the SHA-1 hestia stores,
/// so this is hashed on demand rather than kept — it is wanted once, at export.
fn sha512_file(path: &Path) -> Result<String> {
    use sha2::Digest;
    use std::io::Read;

    let mut file =
        std::fs::File::open(path).with_context(|| format!("cannot read {}", path.display()))?;
    let mut hasher = sha2::Sha512::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .with_context(|| format!("cannot hash {}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(Hasher::hex(&hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(dependencies(&vanilla), json!({ "minecraft": "1.21.1" }));

        let fabric = InstanceProfile {
            flavor: "fabric".to_string(),
            game_version: "1.21.1".to_string(),
            loader_version: Some("0.16.9".to_string()),
            ..Default::default()
        };
        assert_eq!(
            dependencies(&fabric),
            json!({ "minecraft": "1.21.1", "fabric-loader": "0.16.9" })
        );

        let neoforge = InstanceProfile {
            flavor: "neoforge".to_string(),
            game_version: "1.21.1".to_string(),
            loader_version: Some("21.1.66".to_string()),
            ..Default::default()
        };
        assert_eq!(
            dependencies(&neoforge),
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
    fn a_disabled_item_is_exported_as_optional_rather_than_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let managed = managed_file(dir.path(), "off.jar");
        let item = InstalledContent {
            url: "https://cdn.modrinth.com/off.jar".to_string(),
            sha1: "abc".to_string(),
            filename: "off.jar".to_string(),
            enabled: false,
            ..Default::default()
        };
        let entry = index_entry(&item, &managed, "mods/off.jar")
            .unwrap()
            .unwrap();
        assert_eq!(entry["env"]["client"], "optional");
        assert_eq!(entry["env"]["server"], "optional");
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
