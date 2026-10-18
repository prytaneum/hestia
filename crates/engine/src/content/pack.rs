//! A modpack archive: the zip a pack is distributed as, its manifest, and the
//! trees it writes straight into the game directory. This module is the one
//! place that decides whether a zip is a pack, which format, and where it
//! starts. An archive knows its own format — `.mrpack`'s fixed `overrides/`,
//! `client-overrides/` and `server-overrides/`, or the single tree a CurseForge
//! `manifest.json` names — so no caller is told which platform served it. Only
//! the `.mrpack` index is parsed here; a CurseForge manifest names files by id,
//! which its API alone resolves, so that half lives in `curseforge.rs` and the
//! archive only hands the manifest over.
//!
//! A pack picked off disk has no source; only its files' Modrinth CDN URLs are read for ids.

use std::path::{Component, Path, PathBuf};

use anyhow::Result;
use proto::content::{ModpackFile, ResolvedModpack, SideSupport};
use proto::download::{Checksum, HashAlgorithm};
use proto::error::{ArchiveFault, ErrorInfo};
use proto::minecraft::{Artifact, ProvisionPhase};
use proto::transfer::ImportFormat;
use serde_json::{Map, Value};

use super::json::{parse_side, str_field};
use super::modrinth;
use crate::cancel::Job;
use crate::transfer::archive::{Extracted, Reader};

pub(crate) const INDEX: &str = "modrinth.index.json";
const MANIFEST: &str = "manifest.json";
const DEFAULT_OVERRIDES: &str = "overrides";

/// The dependency keys that name a modloader, newest-preferred order. The loader
/// name is the key with any `-loader` suffix stripped.
const LOADER_KEYS: [&str; 4] = ["fabric-loader", "quilt-loader", "neoforge", "forge"];

/// Which side an install is for — which of the two side-specific override trees
/// applies, and which `env` field decides whether a file is wanted.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Side {
    Client,
    Server,
}

impl Side {
    /// The `.mrpack` override tree that belongs to this side alone. The shared
    /// `overrides/` tree applies to both and is written first, so a side tree
    /// wins where the two name the same path.
    fn overrides_dir(self) -> &'static str {
        match self {
            Side::Client => "client-overrides",
            Side::Server => "server-overrides",
        }
    }

    /// Whether the pack wants this file on this side. A file the pack marks
    /// unsupported for the side is not an error — a client-only shader in a
    /// pack installed on a server is simply not that server's file.
    pub fn wants(self, file: &ModpackFile) -> bool {
        let support = match self {
            Side::Client => file.client,
            Side::Server => file.server,
        };
        support != SideSupport::Unsupported
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Format {
    Mrpack,
    CurseForge,
}

impl Format {
    pub(crate) fn marker(self) -> &'static str {
        match self {
            Format::Mrpack => INDEX,
            Format::CurseForge => MANIFEST,
        }
    }

    pub(crate) fn id(self) -> ImportFormat {
        match self {
            Format::Mrpack => ImportFormat::Mrpack,
            Format::CurseForge => ImportFormat::CurseForge,
        }
    }
}

/// `manifest.json` is a name any archive may carry, so a CurseForge candidate counts only when `claims_manifest` vets it.
pub(crate) fn locate(
    names: &[String],
    mut claims_manifest: impl FnMut(&str) -> bool,
) -> Option<(Format, String)> {
    [Format::Mrpack, Format::CurseForge]
        .into_iter()
        .filter_map(|format| shallowest(names, format.marker()).map(|root| (format, root)))
        .filter(|(format, root)| *format == Format::Mrpack || claims_manifest(root))
        .min_by_key(|(_, root)| root.matches('/').count())
}

pub(crate) fn is_curseforge_pack(reader: &mut Reader, root: &str) -> bool {
    manifest_at(reader, root).is_ok_and(|manifest| super::curseforge::is_pack_manifest(&manifest))
}

fn manifest_at(reader: &mut Reader, root: &str) -> Result<Value> {
    let text = reader.read_text(&format!("{root}{MANIFEST}"))?;
    serde_json::from_str(&text).map_err(|e| {
        tracing::warn!(error = %e, "{MANIFEST} is malformed");
        invalid(ArchiveFault::ManifestMalformed)
    })
}

/// An opened pack archive, handed to the modpack flow without being reopened.
pub(crate) struct Archive {
    reader: Reader,
    format: Format,
    /// The directory the pack starts at: a zip made from a folder nests it one level down.
    root: String,
}

impl Archive {
    pub(crate) fn new(reader: Reader, format: Format, root: String) -> Self {
        Archive {
            reader,
            format,
            root,
        }
    }

    pub(crate) fn open(path: &Path) -> Result<Self> {
        let mut reader = Reader::open(path)?;
        let (format, root) = locate(&reader.names(), |root| {
            is_curseforge_pack(&mut reader, root)
        })
        .ok_or_else(|| ErrorInfo::ArchiveUnrecognised {
            filename: crate::transfer::archive::filename_of(path),
        })?;
        Ok(Archive::new(reader, format, root))
    }

    /// `None` for a `.mrpack`, which [`Self::index`] reads whole.
    pub(crate) fn platform_manifest(&mut self) -> Result<Option<(&'static str, Value)>> {
        match self.format {
            Format::Mrpack => Ok(None),
            Format::CurseForge => Ok(Some((
                super::curseforge::ID,
                manifest_at(&mut self.reader, &self.root)?,
            ))),
        }
    }

    /// The `.mrpack` manifest. Ids are left empty — only the platform that
    /// served the archive knows them, and a pack read off disk has none.
    pub(crate) fn index(&mut self) -> Result<ResolvedModpack> {
        let text = self
            .reader
            .read_text(&format!("{}{INDEX}", self.root))
            .map_err(|_| invalid(ArchiveFault::ManifestMissing))?;
        let index: Value = serde_json::from_str(&text).map_err(|e| {
            tracing::warn!(error = %e, "{INDEX} is malformed");
            invalid(ArchiveFault::ManifestMalformed)
        })?;
        parse_index(&index)
    }

    pub(crate) fn summary(&mut self) -> Result<ResolvedModpack> {
        match self.platform_manifest()? {
            Some((_, manifest)) => super::curseforge::parse_manifest(&manifest),
            None => self.index(),
        }
    }

    /// In write order, so a later tree wins a path the earlier one also names.
    /// CurseForge says nothing about sides, so it has the one tree.
    fn overrides_trees(&mut self, side: Side) -> Result<Vec<String>> {
        let dirs = match self.format {
            Format::Mrpack => vec![
                DEFAULT_OVERRIDES.to_string(),
                side.overrides_dir().to_string(),
            ],
            Format::CurseForge => {
                let manifest = manifest_at(&mut self.reader, &self.root)?;
                vec![match manifest.get("overrides").and_then(Value::as_str) {
                    Some(dir) if !dir.trim().is_empty() => dir.trim().to_string(),
                    _ => DEFAULT_OVERRIDES.to_string(),
                }]
            }
        };
        Ok(dirs
            .into_iter()
            .map(|dir| format!("{}{dir}/", self.root))
            .collect())
    }

    /// `place` maps a path in the override trees to where it goes; `None` leaves what is on disk untouched.
    pub(crate) fn extract_overrides(
        &mut self,
        side: Side,
        job: &Job,
        place: impl FnMut(&str) -> Option<PathBuf>,
    ) -> Result<Vec<Extracted>> {
        let trees = self.overrides_trees(side)?;
        self.reader
            .extract(&trees, ProvisionPhase::Overrides, true, job, place)
    }
}

/// The directory prefix of the shallowest member named `marker`, so a nested
/// export is found but a marker buried deeper never outranks one at the root.
pub(crate) fn shallowest(names: &[String], marker: &str) -> Option<String> {
    names
        .iter()
        .filter_map(|name| match name.as_str() == marker {
            true => Some(String::new()),
            false => name
                .strip_suffix(marker)
                .filter(|root| root.ends_with('/'))
                .map(str::to_string),
        })
        .min_by_key(|root| root.matches('/').count())
}

/// Parse a `modrinth.index.json` into a resolved pack. Pure — the source and the
/// project/version ids come from whoever served the archive, not the index.
/// Rejects an unsupported format version, a file with an unsafe (absolute or
/// parent-escaping) path, and a manifest that pins no Minecraft version.
pub fn parse_index(index: &Value) -> Result<ResolvedModpack> {
    let format = index
        .get("formatVersion")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if format != 1 {
        tracing::warn!(format, "unsupported modpack format version");
        return Err(invalid(ArchiveFault::FormatVersion));
    }

    let mut files = Vec::new();
    if let Some(arr) = index.get("files").and_then(Value::as_array) {
        for f in arr {
            let path = f.get("path").and_then(Value::as_str).unwrap_or_default();
            if !is_safe_path(path) {
                tracing::warn!(path, "a modpack file escapes its directory");
                return Err(invalid(ArchiveFault::UnsafePath));
            }
            let url = f
                .get("downloads")
                .and_then(Value::as_array)
                .and_then(|d| d.first())
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let size = f.get("fileSize").and_then(Value::as_u64).unwrap_or(0);
            let sha1 = f
                .get("hashes")
                .and_then(|h| h.get("sha1"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let env = f.get("env");
            // A missing `env` means the file applies to both sides (the spec).
            let client = env
                .and_then(|e| e.get("client"))
                .and_then(Value::as_str)
                .map(parse_side)
                .unwrap_or(SideSupport::Required);
            let server = env
                .and_then(|e| e.get("server"))
                .and_then(Value::as_str)
                .map(parse_side)
                .unwrap_or(SideSupport::Required);
            let (source, project_id, version_id) = match modrinth::cdn_file(&url) {
                Some(file) => (modrinth::ID.to_string(), file.project_id, file.version_id),
                None => Default::default(),
            };
            files.push(ModpackFile {
                path: path.to_string(),
                artifact: Artifact {
                    url,
                    filename: filename_of(path),
                    size,
                    checksum: (!sha1.is_empty()).then_some(Checksum {
                        algorithm: HashAlgorithm::Sha1,
                        hex: sha1,
                    }),
                },
                client,
                server,
                source,
                project_id,
                version_id,
                ..ModpackFile::default()
            });
        }
    }

    let deps = index.get("dependencies").and_then(Value::as_object);
    let game_version = deps
        .and_then(|d| d.get("minecraft"))
        .and_then(Value::as_str)
        .ok_or_else(|| invalid(ArchiveFault::NoGameVersion))?
        .to_string();
    let (loader, loader_version) = deps
        .and_then(find_loader)
        .map(|(l, v)| (Some(l), Some(v)))
        .unwrap_or((None, None));

    Ok(ResolvedModpack {
        source: String::new(),
        project_id: String::new(),
        version_id: String::new(),
        version_number: str_field(index, "versionId"),
        name: str_field(index, "name"),
        summary: str_field(index, "summary"),
        game_version,
        loader,
        loader_version,
        files,
    })
}

fn find_loader(deps: &Map<String, Value>) -> Option<(String, String)> {
    for key in LOADER_KEYS {
        if let Some(version) = deps.get(key).and_then(Value::as_str) {
            let name = key.strip_suffix("-loader").unwrap_or(key).to_string();
            return Some((name, version.to_string()));
        }
    }
    None
}

pub(crate) fn invalid(reason: ArchiveFault) -> anyhow::Error {
    proto::error::ErrorInfo::ModpackInvalid { reason }.into()
}

fn filename_of(path: &str) -> String {
    path.rsplit(['/', '\\']).next().unwrap_or(path).to_string()
}

/// A relative path that stays inside the game directory: not empty, not
/// absolute, and with no parent (`..`), root, or drive-prefix components.
pub fn is_safe_path(path: &str) -> bool {
    if path.is_empty() {
        return false;
    }
    let p = Path::new(path);
    if p.is_absolute() {
        return false;
    }
    p.components()
        .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn index_maps_files_loader_and_game_version() {
        let pack = parse_index(&json!({
            "formatVersion": 1,
            "name": "Cozy Pack",
            "versionId": "1.4.0",
            "summary": "warm",
            "files": [{
                "path": "mods/sodium.jar",
                "downloads": ["https://cdn.modrinth.com/data/AAA/versions/BBB/sodium.jar"],
                "fileSize": 12,
                "hashes": {"sha1": "abc"},
            }],
            "dependencies": {"minecraft": "1.21.1", "fabric-loader": "0.16.9"},
        }))
        .unwrap();

        assert_eq!(pack.name, "Cozy Pack");
        assert_eq!(pack.version_number, "1.4.0");
        assert_eq!(pack.game_version, "1.21.1");
        assert_eq!(pack.loader.as_deref(), Some("fabric"));
        assert_eq!(pack.loader_version.as_deref(), Some("0.16.9"));
        assert_eq!(pack.files[0].artifact.filename, "sodium.jar");
        assert_eq!(pack.files[0].client, SideSupport::Required);
    }

    #[test]
    fn a_missing_env_means_both_sides_want_the_file() {
        let pack = parse_index(&json!({
            "formatVersion": 1,
            "files": [
                {"path": "mods/shared.jar", "downloads": ["https://x/y.jar"]},
                {"path": "mods/client.jar", "downloads": ["https://x/z.jar"],
                 "env": {"client": "required", "server": "unsupported"}},
            ],
            "dependencies": {"minecraft": "1.21.1"},
        }))
        .unwrap();

        assert!(Side::Server.wants(&pack.files[0]));
        assert!(Side::Client.wants(&pack.files[0]));
        assert!(Side::Client.wants(&pack.files[1]));
        assert!(
            !Side::Server.wants(&pack.files[1]),
            "a client-unsupported file is not the server's"
        );
    }

    #[test]
    fn a_pack_with_no_loader_is_vanilla() {
        let pack = parse_index(&json!({
            "formatVersion": 1,
            "dependencies": {"minecraft": "1.21.1"},
        }))
        .unwrap();
        assert_eq!(pack.loader, None);
    }

    #[test]
    fn unsupported_format_and_unsafe_paths_are_rejected() {
        assert!(parse_index(&json!({"formatVersion": 2})).is_err());
        assert!(parse_index(&json!({
            "formatVersion": 1,
            "files": [{"path": "../escape.jar", "downloads": ["https://x/y"]}],
            "dependencies": {"minecraft": "1.21.1"},
        }))
        .is_err());
        assert!(parse_index(&json!({"formatVersion": 1, "dependencies": {}})).is_err());
    }

    fn archive(entries: &[(&str, &str)]) -> Archive {
        let dir = temp("zip");
        let path = dir.path().join("pack.zip");
        let mut writer = crate::transfer::archive::Writer::create(&path).unwrap();
        for (name, body) in entries {
            writer.add_bytes(name, body.as_bytes()).unwrap();
        }
        writer.finish().unwrap();
        Archive::open(&path).unwrap()
    }

    fn temp(tag: &str) -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix(&format!("hestia-mrpack-{tag}-"))
            .tempdir()
            .expect("temp dir")
    }

    fn extract(archive: &mut Archive, side: Side, dest: &Path) -> Vec<Extracted> {
        let cancel = crate::cancel::Cancel::new();
        let noop = |_: &proto::minecraft::ProvisionProgress| {};
        let job = Job::new(&noop, &cancel);
        archive
            .extract_overrides(side, &job, |path| Some(dest.join(path)))
            .unwrap()
    }

    #[test]
    fn the_last_tick_reports_the_whole_batch_done() {
        let mut pack = archive(&[
            ("modrinth.index.json", "{}"),
            ("overrides/config/a.toml", "a"),
            ("overrides/config/b.toml", "b"),
        ]);
        let dest_dir = temp("ticks");
        let cancel = crate::cancel::Cancel::new();
        let seen = std::sync::Mutex::new(Vec::new());
        let record = |p: &proto::minecraft::ProvisionProgress| {
            seen.lock().unwrap().push((p.current, p.total));
        };
        let job = Job::new(&record, &cancel);

        pack.extract_overrides(Side::Client, &job, |path| Some(dest_dir.path().join(path)))
            .unwrap();

        let seen = seen.into_inner().unwrap();
        assert_eq!(seen.first(), Some(&(0, 2)));
        assert_eq!(seen.last(), Some(&(2, 2)));
    }

    #[test]
    fn a_side_takes_the_shared_tree_and_its_own() {
        let mut pack = archive(&[
            ("modrinth.index.json", "{}"),
            ("overrides/config/shared.toml", "shared"),
            ("client-overrides/options.txt", "client"),
            ("server-overrides/server-icon.png", "server"),
        ]);
        let dest_dir = temp("sides");
        let dest = dest_dir.path();

        let written = extract(&mut pack, Side::Client, dest);

        assert!(dest.join("config/shared.toml").is_file(), "shared tree");
        assert!(dest.join("options.txt").is_file(), "the client's own tree");
        assert!(
            !dest.join("server-icon.png").exists(),
            "the other side's tree is not this side's"
        );
        assert_eq!(written.len(), 2);
    }

    #[test]
    fn the_server_tree_is_the_servers_own() {
        let mut pack = archive(&[
            ("modrinth.index.json", "{}"),
            ("overrides/config/shared.toml", "shared"),
            ("client-overrides/options.txt", "client"),
            ("server-overrides/server-icon.png", "server"),
        ]);
        let dest_dir = temp("server-side");
        let dest = dest_dir.path();

        extract(&mut pack, Side::Server, dest);

        assert!(dest.join("config/shared.toml").is_file());
        assert!(dest.join("server-icon.png").is_file());
        assert!(!dest.join("options.txt").exists());
    }

    #[test]
    fn a_side_tree_wins_where_both_name_the_same_path() {
        let mut pack = archive(&[
            ("modrinth.index.json", "{}"),
            ("overrides/config/cozy.toml", "shared value"),
            ("client-overrides/config/cozy.toml", "client value"),
        ]);
        let dest_dir = temp("precedence");
        let dest = dest_dir.path();

        extract(&mut pack, Side::Client, dest);

        assert_eq!(
            std::fs::read_to_string(dest.join("config/cozy.toml")).unwrap(),
            "client value",
            "the side tree is written after the shared one, so it wins"
        );
    }

    #[test]
    fn a_curseforge_pack_uses_the_tree_its_manifest_names() {
        let manifest = r#"{"manifestType":"minecraftModpack","manifestVersion":1,
                           "overrides":"pack-files"}"#;
        let mut pack = archive(&[
            ("manifest.json", manifest),
            ("pack-files/config/cozy.toml", "cf"),
            ("overrides/config/ignored.toml", "not this pack's tree"),
        ]);
        let dest_dir = temp("cf-overrides");
        let dest = dest_dir.path();

        let written = extract(&mut pack, Side::Client, dest);

        assert!(dest.join("config/cozy.toml").is_file());
        assert!(
            !dest.join("config/ignored.toml").exists(),
            "only the tree the manifest names belongs to the pack"
        );
        assert_eq!(written.len(), 1);
    }

    #[test]
    fn a_curseforge_pack_defaults_to_overrides_and_has_no_side_tree() {
        let manifest = r#"{"manifestType":"minecraftModpack","manifestVersion":1}"#;
        let mut pack = archive(&[
            ("manifest.json", manifest),
            ("overrides/config/cozy.toml", "cf"),
            ("client-overrides/options.txt", "mrpack only"),
        ]);
        let dest_dir = temp("cf-default");
        let dest = dest_dir.path();

        extract(&mut pack, Side::Client, dest);

        assert!(dest.join("config/cozy.toml").is_file());
        assert!(
            !dest.join("options.txt").exists(),
            "CurseForge has no side-specific tree, so nothing claims one"
        );
    }

    #[test]
    fn a_curseforge_pack_hands_its_manifest_to_its_platform() {
        let mut pack = archive(&[(
            "manifest.json",
            r#"{"manifestType":"minecraftModpack","manifestVersion":1}"#,
        )]);
        // Its files are project/file ids, so the archive alone resolves nothing.
        assert!(pack.index().is_err());
        let (source, manifest) = pack.platform_manifest().unwrap().expect("a manifest");
        assert_eq!(source, "curseforge");
        assert_eq!(manifest["manifestVersion"], 1);
    }

    #[test]
    fn an_mrpack_has_no_manifest_to_hand_over() {
        let mut pack = archive(&[("modrinth.index.json", "{}")]);
        assert!(pack.platform_manifest().unwrap().is_none());
    }

    #[test]
    fn a_pack_zipped_inside_a_folder_is_read_from_that_folder() {
        let mut mrpack = archive(&[
            (
                "Cozy/modrinth.index.json",
                r#"{"formatVersion":1,"name":"Cozy","dependencies":{"minecraft":"1.21.1"}}"#,
            ),
            ("Cozy/overrides/config/a.toml", "a"),
        ]);
        let dest_dir = temp("nested");
        assert_eq!(mrpack.index().unwrap().name, "Cozy");
        let written = extract(&mut mrpack, Side::Client, dest_dir.path());
        assert_eq!(written.len(), 1);
        assert_eq!(written[0].path, "config/a.toml");

        let mut curseforge = archive(&[(
            "Cozy/manifest.json",
            r#"{"manifestType":"minecraftModpack","manifestVersion":1}"#,
        )]);
        assert!(curseforge.platform_manifest().unwrap().is_some());
    }

    #[test]
    fn an_mrpack_index_wins_over_anything_else_in_the_zip() {
        let mut pack = archive(&[
            (
                "modrinth.index.json",
                r#"{"formatVersion":1,"name":"Cozy",
              "dependencies":{"minecraft":"1.21.1"}}"#,
            ),
            (
                "manifest.json",
                r#"{"manifestType":"minecraftModpack","manifestVersion":1}"#,
            ),
        ]);
        assert_eq!(pack.index().unwrap().name, "Cozy");
    }

    #[test]
    fn nested_directories_are_created_and_hashed() {
        let mut pack = archive(&[
            ("modrinth.index.json", "{}"),
            ("overrides/config/deep/nested/file.json", "{}"),
        ]);
        let dest_dir = temp("nested");
        let dest = dest_dir.path();

        let written = extract(&mut pack, Side::Client, dest);

        assert!(dest.join("config/deep/nested/file.json").is_file());
        assert_eq!(written[0].path, "config/deep/nested/file.json");
        assert_eq!(
            written[0].sha1,
            crate::content::install::sha1_file(&dest.join("config/deep/nested/file.json")).unwrap(),
            "the recorded hash is what was written"
        );
    }

    #[test]
    fn refusing_a_file_leaves_what_is_there() {
        let mut pack = archive(&[
            ("modrinth.index.json", "{}"),
            ("overrides/config/mine.toml", "the pack's"),
            ("overrides/config/theirs.toml", "the pack's"),
        ]);
        let dest_dir = temp("keep");
        let dest = dest_dir.path();
        std::fs::create_dir_all(dest.join("config")).unwrap();
        std::fs::write(dest.join("config/mine.toml"), "my edit").unwrap();

        let cancel = crate::cancel::Cancel::new();
        let noop = |_: &proto::minecraft::ProvisionProgress| {};
        let job = Job::new(&noop, &cancel);
        let written = pack
            .extract_overrides(Side::Client, &job, |path| {
                (path != "config/mine.toml").then(|| dest.join(path))
            })
            .unwrap();

        assert_eq!(
            std::fs::read_to_string(dest.join("config/mine.toml")).unwrap(),
            "my edit",
            "a refused file is untouched"
        );
        assert_eq!(written.len(), 1, "and is not reported as written");
        assert_eq!(written[0].path, "config/theirs.toml");
    }

    #[test]
    fn an_override_escaping_the_game_directory_is_refused() {
        let mut pack = archive(&[
            ("modrinth.index.json", "{}"),
            ("overrides/../../escape.txt", "no"),
        ]);
        let dest_dir = temp("escape");
        let dest = dest_dir.path();
        let cancel = crate::cancel::Cancel::new();
        let noop = |_: &proto::minecraft::ProvisionProgress| {};
        let job = Job::new(&noop, &cancel);

        assert!(pack
            .extract_overrides(Side::Client, &job, |path| Some(dest.join(path)))
            .is_err());
    }

    #[test]
    fn a_pack_with_no_override_trees_writes_nothing() {
        let mut pack = archive(&[("modrinth.index.json", "{}")]);
        let dest_dir = temp("empty");
        let dest = dest_dir.path();
        assert!(extract(&mut pack, Side::Client, dest).is_empty());
    }

    #[test]
    fn safe_paths_stay_inside_the_game_directory() {
        assert!(is_safe_path("mods/sodium.jar"));
        assert!(is_safe_path("config/nested/file.toml"));
        assert!(!is_safe_path(""));
        assert!(!is_safe_path("/etc/passwd"));
        assert!(!is_safe_path("../../escape"));
        assert!(!is_safe_path("mods/../../escape"));
    }
}
