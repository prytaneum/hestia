//! Bedrock behaviour and resource packs. A pack is kept as its `.mcpack` in the
//! entry's managed dir, like any other content; Bedrock Dedicated Server reads
//! folders, so at every start [`sync`] extracts each enabled pack into
//! `data/<behavior|resource>_packs/<uuid>/` and lists it in the world's
//! `world_*_packs.json`. A folder Hestia extracted carries [`MARKER`]; one it
//! did not write is either shipped with the server (recorded in [`SHIPPED`]) or
//! the user's own, and is never touched.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use proto::content::{ContentKind, InstalledContent, UntrackedFile};
use proto::error::{ArchiveFault, ArchiveFormat, ErrorInfo};
use serde_json::{json, Value};

use super::install;
use crate::transfer::archive::{Reader, Writer};

const BEHAVIOR_DIR: &str = "behavior_packs";
const RESOURCE_DIR: &str = "resource_packs";
const MANIFEST: &str = "manifest.json";
const CONTENTS: &str = "contents.json";
const MARKER: &str = ".hestia";
const STAGING_SUFFIX: &str = ".staging";
pub(crate) const SHIPPED: &str = ".bds-packs";
const KINDS: [ContentKind; 2] = [ContentKind::BehaviorPack, ContentKind::ResourcePack];

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Pack {
    pub name: String,
    pub uuid: String,
    pub version: [u32; 3],
    pub kind: ContentKind,
}

impl Pack {
    pub(crate) fn version_string(&self) -> String {
        let [major, minor, patch] = self.version;
        format!("{major}.{minor}.{patch}")
    }
}

fn extension_of(name: &str) -> String {
    Path::new(name)
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

pub(crate) fn is_pack_file(path: &Path) -> bool {
    matches!(
        extension_of(&path.to_string_lossy()).as_str(),
        "mcpack" | "mcaddon"
    )
}

pub(crate) fn is_addon(path: &Path) -> bool {
    extension_of(&path.to_string_lossy()) == "mcaddon"
}

pub(crate) fn is_pack_item(item: &InstalledContent) -> bool {
    extension_of(&item.filename) == "mcpack"
}

/// The file name a direct `https` link to a pack names, or `None` for any other URL.
pub(crate) fn url_filename(url: &str) -> Option<String> {
    let rest = url.strip_prefix("https://")?;
    let path = rest.split(['?', '#']).next()?;
    let name = path.rsplit('/').next()?;
    (path.contains('/') && is_pack_file(Path::new(name))).then(|| name.to_string())
}

fn load_dir(kind: ContentKind) -> Option<&'static str> {
    match kind {
        ContentKind::BehaviorPack => Some(BEHAVIOR_DIR),
        ContentKind::ResourcePack => Some(RESOURCE_DIR),
        _ => None,
    }
}

fn malformed() -> anyhow::Error {
    ErrorInfo::ArchiveInvalid {
        format: ArchiveFormat::Zip,
        reason: ArchiveFault::ManifestMalformed,
    }
    .into()
}

fn missing() -> anyhow::Error {
    ErrorInfo::ArchiveInvalid {
        format: ArchiveFormat::Zip,
        reason: ArchiveFault::ManifestMissing,
    }
    .into()
}

/// Every pack a `.mcpack` (one) or `.mcaddon` (several) holds, each validated.
pub(crate) fn read(path: &Path) -> Result<Vec<Pack>> {
    if !is_addon(path) {
        return Ok(vec![read_single(path)?.0]);
    }
    let staging =
        std::env::temp_dir().join(format!("hestia-addon-{}", uuid::Uuid::now_v7().simple()));
    let _ = std::fs::remove_dir_all(&staging);
    let packs = split(path, &staging).map(|parts| parts.into_iter().map(|(p, _)| p).collect());
    let _ = std::fs::remove_dir_all(&staging);
    packs
}

/// A `.mcpack`'s pack and the archive prefix its files start under (empty, or
/// one top folder).
pub(crate) fn read_single(path: &Path) -> Result<(Pack, String)> {
    let mut reader = Reader::open(path)?;
    let root = pack_root(&reader.names())?;
    let pack = parse(&mut reader, &root)?;
    Ok((pack, root))
}

fn pack_root(names: &[String]) -> Result<String> {
    if names.iter().any(|n| n == MANIFEST) {
        return Ok(String::new());
    }
    let folders: Vec<&str> = names
        .iter()
        .filter_map(|n| n.strip_suffix("/manifest.json"))
        .filter(|dir| !dir.contains('/'))
        .collect();
    match folders[..] {
        [only] => Ok(format!("{only}/")),
        _ => Err(missing()),
    }
}

fn parse(reader: &mut Reader, root: &str) -> Result<Pack> {
    let encrypted = reader.names().contains(&format!("{root}{CONTENTS}"));
    let manifest = reader
        .read_text(&format!("{root}{MANIFEST}"))
        .ok()
        .and_then(|text| lenient_json(&text));
    let Some(manifest) = manifest else {
        return Err(match encrypted {
            true => ErrorInfo::ContentPackEncrypted {
                name: root.trim_end_matches('/').to_string(),
            }
            .into(),
            false => malformed(),
        });
    };
    let header = manifest.get("header").ok_or_else(malformed)?;
    let name = header
        .get("name")
        .and_then(Value::as_str)
        .filter(|n| !n.trim().is_empty())
        .ok_or_else(malformed)?
        .to_string();
    if encrypted && contents_are_keyed(reader, root) {
        return Err(ErrorInfo::ContentPackEncrypted { name }.into());
    }
    let uuid = header
        .get("uuid")
        .and_then(Value::as_str)
        .filter(|u| valid_uuid(u))
        .ok_or_else(malformed)?
        .to_ascii_lowercase();
    let version = header
        .get("version")
        .and_then(Value::as_array)
        .filter(|v| v.len() == 3)
        .and_then(|v| {
            let part = |i: usize| v[i].as_u64().and_then(|n| u32::try_from(n).ok());
            Some([part(0)?, part(1)?, part(2)?])
        })
        .ok_or_else(malformed)?;
    let modules: Vec<&str> = manifest
        .get("modules")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|m| m.get("type").and_then(Value::as_str))
        .collect();
    let refused = modules
        .iter()
        .find(|m| matches!(**m, "skin_pack" | "world_template"));
    let kind = match refused {
        Some(_) => None,
        None => modules.iter().find_map(|m| match *m {
            "data" | "script" => Some(ContentKind::BehaviorPack),
            "resources" => Some(ContentKind::ResourcePack),
            _ => None,
        }),
    };
    let Some(kind) = kind else {
        let module = refused.or(modules.first()).copied().unwrap_or("unknown");
        bail!(ErrorInfo::ContentPackUnsupported {
            name,
            module: module.to_string()
        });
    };
    Ok(Pack {
        name,
        uuid,
        version,
        kind,
    })
}

fn contents_are_keyed(reader: &mut Reader, root: &str) -> bool {
    let contents = reader
        .read_text(&format!("{root}{CONTENTS}"))
        .ok()
        .and_then(|text| lenient_json(&text));
    match contents {
        None => true,
        Some(contents) => contents
            .get("content")
            .and_then(Value::as_array)
            .is_some_and(|entries| entries.iter().any(|e| e.get("key").is_some())),
    }
}

fn valid_uuid(uuid: &str) -> bool {
    uuid.len() == 36
        && uuid.char_indices().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        })
}

fn lenient_json(text: &str) -> Option<Value> {
    serde_json::from_str(&strip_comments(text.trim_start_matches('\u{feff}'))).ok()
}

/// Manifests in the wild carry `//` and `/* */` comments, which JSON forbids.
/// Trailing commas are not tolerated.
fn strip_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let (mut in_string, mut escaped) = (false, false);
    while let Some(c) = chars.next() {
        if in_string {
            out.push(c);
            match (escaped, c) {
                (true, _) => escaped = false,
                (false, '\\') => escaped = true,
                (false, '"') => in_string = false,
                _ => {}
            }
            continue;
        }
        match (c, chars.peek()) {
            ('"', _) => {
                in_string = true;
                out.push(c);
            }
            ('/', Some('/')) => {
                for next in chars.by_ref() {
                    if next == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            ('/', Some('*')) => {
                chars.next();
                let mut previous = ' ';
                for next in chars.by_ref() {
                    if previous == '*' && next == '/' {
                        break;
                    }
                    previous = next;
                }
            }
            _ => out.push(c),
        }
    }
    out
}

/// Write each pack of an `.mcaddon` into `out` as its own `.mcpack`, named
/// `<name>-<uuid prefix>.mcpack`, and return it with its path.
pub(crate) fn split(addon: &Path, out: &Path) -> Result<Vec<(Pack, PathBuf)>> {
    std::fs::create_dir_all(out).with_context(|| format!("cannot create {}", out.display()))?;
    let mut reader = Reader::open(addon)?;
    let names = reader.names();
    let mut parts = Vec::new();
    for (index, nested) in names
        .iter()
        .filter(|n| extension_of(n) == "mcpack")
        .enumerate()
    {
        let part = out.join(format!("nested-{index}.mcpack"));
        std::fs::write(&part, reader.read_bytes(nested)?)
            .with_context(|| format!("cannot write {}", part.display()))?;
        parts.push(part);
    }
    let folders = names
        .iter()
        .filter_map(|n| n.strip_suffix("/manifest.json"))
        .filter(|dir| !dir.contains('/'));
    for (index, folder) in folders.enumerate() {
        let part = out.join(format!("folder-{index}.mcpack"));
        let mut writer = Writer::create(&part)?;
        let prefix = format!("{folder}/");
        for name in names.iter().filter(|n| !n.ends_with('/')) {
            if let Some(relative) = name.strip_prefix(&prefix) {
                writer.add_bytes(relative, &reader.read_bytes(name)?)?;
            }
        }
        writer.finish()?;
        parts.push(part);
    }
    if parts.is_empty() {
        return Err(missing());
    }
    let mut packs = Vec::new();
    for part in parts {
        let (pack, _) = read_single(&part)?;
        let slug: String = pack
            .name
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect();
        let named = out.join(format!("{slug}-{}.mcpack", &pack.uuid[..8]));
        std::fs::rename(&part, &named)
            .with_context(|| format!("cannot place {}", named.display()))?;
        packs.push((pack, named));
    }
    Ok(packs)
}

struct Managed {
    pack: Pack,
    root: String,
    path: PathBuf,
    sha1: String,
    enabled: bool,
}

/// Make `data/` match the index: extract every enabled pack, drop the
/// extractions of packs no longer enabled, and rewrite the world's pack lists.
pub(crate) fn sync(entry_dir: &Path, data_dir: &Path, level: &str) -> Result<()> {
    let mut managed = Vec::new();
    for item in install::load(entry_dir)
        .into_iter()
        .filter(|i| KINDS.contains(&i.kind))
    {
        let path = install::managed_path(entry_dir, &item)?;
        match read_single(&path) {
            Ok((pack, root)) => managed.push(Managed {
                pack,
                root,
                path,
                sha1: item.sha1,
                enabled: item.enabled,
            }),
            Err(e) => tracing::warn!(
                path = %path.display(),
                error = %format!("{e:#}"),
                "cannot read a managed pack; skipping it"
            ),
        }
    }
    let mut removed = HashSet::new();
    for kind in KINDS {
        let dir = data_dir.join(load_dir(kind).expect("pack kinds have a load dir"));
        let enabled: Vec<&Managed> = managed
            .iter()
            .filter(|m| m.pack.kind == kind && m.enabled)
            .collect();
        let wanted: HashSet<&str> = enabled.iter().map(|m| m.pack.uuid.as_str()).collect();
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = entry.path();
            if name.ends_with(STAGING_SUFFIX) {
                let _ = std::fs::remove_dir_all(&path);
            } else if path.join(MARKER).is_file() && !wanted.contains(name.as_str()) {
                std::fs::remove_dir_all(&path)
                    .with_context(|| format!("cannot remove {}", path.display()))?;
                tracing::info!(uuid = %name, "pack extraction removed");
                removed.insert(name);
            }
        }
        for m in &enabled {
            extract(m, &dir)?;
        }
        let all: Vec<&Managed> = managed.iter().filter(|m| m.pack.kind == kind).collect();
        write_world_list(data_dir, level, kind, &all, &removed)?;
    }
    Ok(())
}

fn extract(managed: &Managed, dir: &Path) -> Result<()> {
    let target = dir.join(&managed.pack.uuid);
    if target.exists() {
        match std::fs::read_to_string(target.join(MARKER)) {
            Ok(sha1) if sha1.trim() == managed.sha1 => {
                tracing::debug!(uuid = %managed.pack.uuid, "pack already extracted");
                return Ok(());
            }
            Ok(_) => {}
            Err(_) => {
                tracing::warn!(
                    uuid = %managed.pack.uuid,
                    "a pack folder Hestia did not write holds this uuid; leaving it"
                );
                return Ok(());
            }
        }
    }
    let staging = dir.join(format!("{}{STAGING_SUFFIX}", managed.pack.uuid));
    let _ = std::fs::remove_dir_all(&staging);
    let root = managed.root.trim_end_matches('/');
    let placed =
        crate::extract::extract_zip_under(&managed.path, Path::new(root), &staging, |_, _| Ok(()))
            .and_then(|()| {
                std::fs::write(staging.join(MARKER), &managed.sha1)
                    .context("cannot mark the extraction")
            })
            .and_then(|()| {
                if target.exists() {
                    std::fs::remove_dir_all(&target)?;
                }
                std::fs::rename(&staging, &target).context("cannot place the extracted pack")
            });
    if placed.is_err() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    placed?;
    tracing::info!(
        name = %managed.pack.name,
        uuid = %managed.pack.uuid,
        "pack extracted"
    );
    Ok(())
}

/// Keeps the entries Hestia did not write: only a pack the index names, or one
/// whose extraction was just removed, is replaced.
fn write_world_list(
    data_dir: &Path,
    level: &str,
    kind: ContentKind,
    managed: &[&Managed],
    removed: &HashSet<String>,
) -> Result<()> {
    if level.is_empty() || level == ".." || level.contains(['/', '\\']) {
        tracing::warn!(
            level,
            "the level name is not a folder name; world pack list left alone"
        );
        return Ok(());
    }
    let file = data_dir.join("worlds").join(level).join(match kind {
        ContentKind::BehaviorPack => "world_behavior_packs.json",
        _ => "world_resource_packs.json",
    });
    let existing: Vec<Value> = match std::fs::read(&file) {
        Ok(bytes) => match serde_json::from_slice(&bytes) {
            Ok(list) => list,
            Err(e) => {
                tracing::warn!(path = %file.display(), error = %e, "world pack list is not valid JSON; leaving it");
                return Ok(());
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(e).with_context(|| format!("cannot read {}", file.display())),
    };
    let ours: HashSet<&str> = managed.iter().map(|m| m.pack.uuid.as_str()).collect();
    let mut next: Vec<Value> = existing
        .iter()
        .filter(|entry| {
            let id = entry
                .get("pack_id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_ascii_lowercase();
            !ours.contains(id.as_str()) && !removed.contains(&id)
        })
        .cloned()
        .collect();
    next.extend(
        managed
            .iter()
            .filter(|m| m.enabled)
            .map(|m| json!({ "pack_id": m.pack.uuid, "version": m.pack.version })),
    );
    if next == existing {
        tracing::debug!(path = %file.display(), "world pack list already current");
        return Ok(());
    }
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("cannot create {}", parent.display()))?;
    }
    let body = serde_json::to_vec_pretty(&next).context("the pack list serializes")?;
    common::atomic::write(&file, &body)
        .with_context(|| format!("cannot write {}", file.display()))?;
    tracing::debug!(path = %file.display(), packs = next.len(), "world pack list rewritten");
    Ok(())
}

fn subfolders(dir: &Path) -> Vec<(String, PathBuf)> {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
        .filter(|(name, _)| !name.ends_with(STAGING_SUFFIX))
        .collect()
}

/// Pack folders in the server's load dir that Hestia did not extract and the
/// server did not ship — dropped in by hand.
pub(crate) fn untracked(data_dir: &Path, kind: ContentKind) -> Vec<UntrackedFile> {
    let Some(dir) = load_dir(kind) else {
        return Vec::new();
    };
    let shipped = std::fs::read_to_string(data_dir.join(SHIPPED)).unwrap_or_default();
    let mut found: Vec<UntrackedFile> = subfolders(&data_dir.join(dir))
        .into_iter()
        .filter(|(name, path)| {
            !path.join(MARKER).is_file() && !shipped.lines().any(|l| l == format!("{dir}/{name}"))
        })
        .map(|(name, path)| UntrackedFile {
            name,
            path: path.to_string_lossy().into_owned(),
        })
        .collect();
    found.sort_by(|a, b| a.name.cmp(&b.name));
    found
}

/// Remember which pack folders the server zip put there, so they are never
/// reported as untracked or touched.
pub(crate) fn record_shipped(root: &Path) {
    let lines: Vec<String> = [BEHAVIOR_DIR, RESOURCE_DIR]
        .iter()
        .flat_map(|dir| {
            subfolders(&root.join(dir))
                .into_iter()
                .filter(|(_, path)| !path.join(MARKER).is_file())
                .map(move |(name, _)| format!("{dir}/{name}"))
        })
        .collect();
    if let Err(e) = std::fs::write(root.join(SHIPPED), lines.join("\n")) {
        tracing::warn!(error = %e, "cannot record the packs the server ships");
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    const UUID: &str = "11111111-2222-3333-4444-555555555555";
    const OTHER: &str = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";

    fn manifest(uuid: &str, module: &str) -> String {
        format!(
            r#"{{ // comment
  "format_version": 2,
  "header": {{ "name": "Test {module}", "uuid": "{uuid}", "version": [1, 0, 2] }},
  "modules": [{{ "type": "{module}", "uuid": "{OTHER}", "version": [1, 0, 0] }}]
}}"#
        )
    }

    pub(crate) fn zip_of(path: &Path, files: &[(&str, &[u8])]) {
        let mut zip = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        for (name, body) in files {
            zip.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(body).unwrap();
        }
        zip.finish().unwrap();
    }

    pub(crate) fn mcpack(path: &Path, uuid: &str, module: &str) {
        zip_of(
            path,
            &[
                ("manifest.json", manifest(uuid, module).as_bytes()),
                ("entities/a.json", b"{}"),
            ],
        );
    }

    #[test]
    fn a_behavior_and_a_resource_pack_are_read() {
        let dir = tempfile::tempdir().unwrap();
        let (b, r) = (dir.path().join("b.mcpack"), dir.path().join("r.mcpack"));
        mcpack(&b, UUID, "data");
        mcpack(&r, OTHER, "resources");
        let pack = read(&b).unwrap().remove(0);
        assert_eq!(pack.kind, ContentKind::BehaviorPack);
        assert_eq!(pack.uuid, UUID);
        assert_eq!(pack.version, [1, 0, 2]);
        assert_eq!(read(&r).unwrap()[0].kind, ContentKind::ResourcePack);
    }

    #[test]
    fn a_manifest_in_one_top_folder_is_found() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("p.mcpack");
        zip_of(
            &path,
            &[("Pack/manifest.json", manifest(UUID, "script").as_bytes())],
        );
        let (pack, root) = read_single(&path).unwrap();
        assert_eq!(root, "Pack/");
        assert_eq!(pack.kind, ContentKind::BehaviorPack);
    }

    #[test]
    fn an_addon_yields_one_pack_per_member() {
        let dir = tempfile::tempdir().unwrap();
        let (inner, addon) = (dir.path().join("in.mcpack"), dir.path().join("a.mcaddon"));
        mcpack(&inner, UUID, "data");
        let inner_bytes = std::fs::read(&inner).unwrap();
        zip_of(
            &addon,
            &[
                ("bp.mcpack", &inner_bytes),
                ("RP/manifest.json", manifest(OTHER, "resources").as_bytes()),
                ("RP/textures/x.png", b"x"),
            ],
        );
        let mut kinds: Vec<_> = read(&addon).unwrap().into_iter().map(|p| p.kind).collect();
        kinds.sort_by_key(|k| format!("{k:?}"));
        assert_eq!(
            kinds,
            vec![ContentKind::BehaviorPack, ContentKind::ResourcePack]
        );
        let out = dir.path().join("out");
        let parts = split(&addon, &out).unwrap();
        assert_eq!(parts.len(), 2);
        assert!(parts.iter().all(|(_, p)| read_single(p).is_ok()));
    }

    fn refusal(path: &Path) -> ErrorInfo {
        crate::error_info(read(path).unwrap_err())
    }

    #[test]
    fn skin_packs_encrypted_packs_and_bad_uuids_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("p.mcpack");

        mcpack(&path, UUID, "skin_pack");
        assert!(matches!(
            refusal(&path),
            ErrorInfo::ContentPackUnsupported { module, .. } if module == "skin_pack"
        ));

        zip_of(
            &path,
            &[
                ("manifest.json", manifest(UUID, "data").as_bytes()),
                ("contents.json", br#"{"content":[{"path":"a","key":"k"}]}"#),
            ],
        );
        assert!(matches!(
            refusal(&path),
            ErrorInfo::ContentPackEncrypted { .. }
        ));

        zip_of(
            &path,
            &[
                ("manifest.json", &[0u8, 159, 146, 150]),
                ("contents.json", &[0u8, 1, 2]),
            ],
        );
        assert!(matches!(
            refusal(&path),
            ErrorInfo::ContentPackEncrypted { .. }
        ));

        mcpack(&path, "../../etc", "data");
        assert!(matches!(
            refusal(&path),
            ErrorInfo::ArchiveInvalid {
                reason: ArchiveFault::ManifestMalformed,
                ..
            }
        ));
    }

    #[test]
    fn only_a_direct_https_pack_link_names_a_file() {
        assert_eq!(
            url_filename("https://example.com/dl/Pack.mcpack?x=1").as_deref(),
            Some("Pack.mcpack")
        );
        assert_eq!(url_filename("http://example.com/a.mcpack"), None);
        assert_eq!(url_filename("https://example.com/a.zip"), None);
        assert_eq!(url_filename("https://a.mcpack"), None);
    }

    fn entry(root: &Path, uuid: &str, module: &str, enabled: bool) -> PathBuf {
        let kind = match module {
            "data" => ContentKind::BehaviorPack,
            _ => ContentKind::ResourcePack,
        };
        let item = InstalledContent {
            kind,
            filename: format!("{uuid}.mcpack"),
            sha1: "sha".to_string(),
            enabled,
            ..InstalledContent::default()
        };
        let managed = install::managed_path(root, &item).unwrap();
        std::fs::create_dir_all(managed.parent().unwrap()).unwrap();
        mcpack(&managed, uuid, module);
        let mut items = install::load(root);
        items.push(item);
        install::save(root, items).unwrap();
        managed
    }

    fn pack_ids(file: &Path) -> Vec<String> {
        let list: Vec<Value> = serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
        list.iter()
            .map(|e| e["pack_id"].as_str().unwrap().to_string())
            .collect()
    }

    #[test]
    fn sync_extracts_lists_keeps_foreign_entries_and_removes_what_is_disabled() {
        let dir = tempfile::tempdir().unwrap();
        let (root, data) = (dir.path().join("entry"), dir.path().join("entry/data"));
        std::fs::create_dir_all(data.join("behavior_packs/vanilla")).unwrap();
        std::fs::create_dir_all(data.join("behavior_packs/by_hand")).unwrap();
        std::fs::write(data.join("behavior_packs/vanilla/manifest.json"), "{}").unwrap();
        std::fs::write(data.join(SHIPPED), "behavior_packs/vanilla").unwrap();
        let list = data.join("worlds/Bedrock level/world_behavior_packs.json");
        std::fs::create_dir_all(list.parent().unwrap()).unwrap();
        std::fs::write(&list, r#"[{"pack_id":"foreign","version":[1,0,0]}]"#).unwrap();

        entry(&root, UUID, "data", true);
        sync(&root, &data, "Bedrock level").unwrap();

        let extracted = data.join("behavior_packs").join(UUID);
        assert!(extracted.join("manifest.json").is_file());
        assert!(extracted.join("entities/a.json").is_file());
        assert_eq!(
            pack_ids(&list),
            vec!["foreign".to_string(), UUID.to_string()]
        );
        let names: Vec<_> = untracked(&data, ContentKind::BehaviorPack)
            .into_iter()
            .map(|f| f.name)
            .collect();
        assert_eq!(names, vec!["by_hand".to_string()]);

        let mut items = install::load(&root);
        items[0].enabled = false;
        install::save(&root, items).unwrap();
        sync(&root, &data, "Bedrock level").unwrap();

        assert!(!extracted.exists());
        assert_eq!(pack_ids(&list), vec!["foreign".to_string()]);
        assert!(data.join("behavior_packs/vanilla").is_dir());
        assert!(data.join("behavior_packs/by_hand").is_dir());
    }

    #[test]
    fn a_resource_pack_lands_in_the_resource_dir_and_its_own_list() {
        let dir = tempfile::tempdir().unwrap();
        let (root, data) = (dir.path().join("entry"), dir.path().join("entry/data"));
        entry(&root, OTHER, "resources", true);
        sync(&root, &data, "world").unwrap();
        assert!(data
            .join("resource_packs")
            .join(OTHER)
            .join("manifest.json")
            .is_file());
        assert_eq!(
            pack_ids(&data.join("worlds/world/world_resource_packs.json")),
            vec![OTHER.to_string()]
        );
        assert!(!data.join("worlds/world/world_behavior_packs.json").exists());
    }
}
