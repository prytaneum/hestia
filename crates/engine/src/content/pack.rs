//! A modpack archive: the zip a pack is distributed as, its manifest, and the
//! trees it writes straight into the game directory. This module owns what the
//! launcher itself reads — the `.mrpack` index and its fixed `overrides/`,
//! `client-overrides/` and `server-overrides/` — and the [`Archive`] every pack
//! is installed from, whichever format read it: a pack format a plugin
//! contributes hands over the trees it names and the policy its files are
//! fetched under, so no caller is told which platform served it.
//!
//! A pack picked off disk has no source; only its files' Modrinth CDN URLs are read for ids.

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use proto::content::{ModpackFile, ResolvedModpack, SideSupport};
use proto::download::{Checksum, HashAlgorithm};
use proto::error::ArchiveFault;
use proto::minecraft::{Artifact, ConfigEntry, ProvisionPhase};
use proto::provider::pack::OverrideTrees;
use proto::provider::Provider;
use proto::transfer::ImportFormat;
use serde_json::{Map, Value};

use super::json::{parse_side, str_field};
use crate::cancel::Job;
use crate::minecraft::Loaders;
use crate::plugins::vouch::Policy;
use crate::transfer::archive::{Extracted, Reader};

pub(crate) const INDEX: &str = "modrinth.index.json";
const MINECRAFT_DEPENDENCY: &str = "minecraft";

/// Which side an install is for — which of the two side-specific override trees
/// applies, and which `env` field decides whether a file is wanted.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Side {
    Client,
    Server,
}

impl Side {
    /// The directories whose files this side takes, in write order: the shared
    /// trees first, so a tree of the side's own wins a path both name.
    fn trees(self, trees: &OverrideTrees) -> impl Iterator<Item = &String> {
        let own = match self {
            Side::Client => &trees.client,
            Side::Server => &trees.server,
        };
        trees.shared.iter().chain(own)
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

pub(crate) const MRPACK: &str = "mrpack";

pub(crate) fn mrpack_format() -> ImportFormat {
    ImportFormat::Provider(Provider::new(MRPACK, "Modrinth pack"))
}

/// The directory a `.mrpack` starts at, when the archive is one: a zip made
/// from a folder nests it one level down.
pub(crate) fn locate(names: &[String]) -> Option<String> {
    shallowest(names, INDEX)
}

/// An opened pack archive, handed to the modpack flow without being reopened.
pub(crate) struct Archive {
    reader: Reader,
    /// The directory the pack starts at.
    root: String,
    trees: OverrideTrees,
    /// What the files the pack names are fetched under: a plugin's grant, or
    /// none for the launcher's own `.mrpack`.
    policy: Option<Arc<Policy>>,
    /// The vetted JVM settings the pack asks for, as `config` entries, for the
    /// instance an install creates.
    config: Vec<ConfigEntry>,
}

impl Archive {
    pub(crate) fn mrpack(reader: Reader, root: String) -> Self {
        Archive {
            reader,
            root,
            trees: OverrideTrees {
                shared: vec!["overrides".to_string()],
                client: vec!["client-overrides".to_string()],
                server: vec!["server-overrides".to_string()],
            },
            policy: None,
            config: Vec::new(),
        }
    }

    pub(crate) fn contributed(
        reader: Reader,
        root: String,
        trees: OverrideTrees,
        policy: Option<Arc<Policy>>,
        config: Vec<ConfigEntry>,
    ) -> Self {
        Archive {
            reader,
            root,
            trees,
            policy,
            config,
        }
    }

    pub(crate) fn config(&self) -> &[ConfigEntry] {
        &self.config
    }

    pub(crate) fn policy(&self) -> Option<Arc<Policy>> {
        self.policy.clone()
    }

    /// The `.mrpack` manifest. Ids are left empty; see `Content::recognise_files`.
    pub(crate) fn index(&mut self, loaders: &Loaders) -> Result<ResolvedModpack> {
        let text = self
            .reader
            .read_text(&format!("{}{INDEX}", self.root))
            .map_err(|_| invalid(ArchiveFault::ManifestMissing))?;
        let index: Value = serde_json::from_str(&text).map_err(|e| {
            tracing::warn!(error = %e, "{INDEX} is malformed");
            invalid(ArchiveFault::ManifestMalformed)
        })?;
        parse_index(loaders, &index)
    }

    /// `place` maps a path in the override trees to where it goes; `None` leaves what is on disk untouched.
    pub(crate) fn extract_overrides(
        &mut self,
        side: Side,
        job: &Job,
        place: impl FnMut(&str) -> Option<PathBuf>,
    ) -> Result<Vec<Extracted>> {
        let trees: Vec<String> = side
            .trees(&self.trees)
            .map(|dir| format!("{}{dir}/", self.root))
            .collect();
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
/// project/version ids come from whoever recognises the file URLs, not the index.
/// Rejects an unsupported format version, a file with an unsafe (absolute or
/// parent-escaping) path, and a manifest that pins no Minecraft version.
pub fn parse_index(loaders: &Loaders, index: &Value) -> Result<ResolvedModpack> {
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
                    unhashed: false,
                },
                client,
                server,
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
        .and_then(|deps| find_loader(deps, loaders))
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

/// The loader a pack's `dependencies` pin. The index lists nothing but
/// `minecraft` and the loader, so a key no registered flavor claims is a loader
/// this build has no flavor for: it is named as the pack names it, and
/// whatever creates the entry reports it unsupported.
fn find_loader(deps: &Map<String, Value>, loaders: &Loaders) -> Option<(String, String)> {
    deps.iter()
        .filter(|(key, _)| key.as_str() != MINECRAFT_DEPENDENCY)
        .find_map(|(key, version)| {
            let flavor = loaders
                .flavor_of_mrpack_key(key)
                .unwrap_or_else(|| key.strip_suffix("-loader").unwrap_or(key));
            Some((flavor.to_string(), version.as_str()?.to_string()))
        })
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

    fn loaders() -> Loaders {
        crate::minecraft::Minecraft::default().loaders()
    }

    #[test]
    fn index_maps_files_loader_and_game_version() {
        let pack = parse_index(
            &loaders(),
            &json!({
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
            }),
        )
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
        let pack = parse_index(
            &loaders(),
            &json!({
                "formatVersion": 1,
                "files": [
                    {"path": "mods/shared.jar", "downloads": ["https://x/y.jar"]},
                    {"path": "mods/client.jar", "downloads": ["https://x/z.jar"],
                     "env": {"client": "required", "server": "unsupported"}},
                ],
                "dependencies": {"minecraft": "1.21.1"},
            }),
        )
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
    fn unsupported_format_and_unsafe_paths_are_rejected() {
        assert!(parse_index(&loaders(), &json!({"formatVersion": 2})).is_err());
        assert!(parse_index(
            &loaders(),
            &json!({
                "formatVersion": 1,
                "files": [{"path": "../escape.jar", "downloads": ["https://x/y"]}],
                "dependencies": {"minecraft": "1.21.1"},
            })
        )
        .is_err());
        assert!(parse_index(&loaders(), &json!({"formatVersion": 1, "dependencies": {}})).is_err());
    }

    fn archive(entries: &[(&str, &str)]) -> Archive {
        let dir = temp("zip");
        let path = dir.path().join("pack.zip");
        let mut writer = crate::transfer::archive::Writer::create(&path).unwrap();
        for (name, body) in entries {
            writer.add_bytes(name, body.as_bytes()).unwrap();
        }
        writer.finish().unwrap();
        let reader = Reader::open(&path).unwrap();
        let root = locate(&reader.names()).expect("an mrpack");
        Archive::mrpack(reader, root)
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
    fn a_pack_zipped_inside_a_folder_is_read_from_that_folder() {
        let mut mrpack = archive(&[
            (
                "Cozy/modrinth.index.json",
                r#"{"formatVersion":1,"name":"Cozy","dependencies":{"minecraft":"1.21.1"}}"#,
            ),
            ("Cozy/overrides/config/a.toml", "a"),
        ]);
        let dest_dir = temp("nested");
        assert_eq!(mrpack.index(&loaders()).unwrap().name, "Cozy");
        let written = extract(&mut mrpack, Side::Client, dest_dir.path());
        assert_eq!(written.len(), 1);
        assert_eq!(written[0].path, "config/a.toml");
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
}
