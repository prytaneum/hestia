//! A content source that serves a JSON index compiled into the component.
//! Files in the index name a path relative to the plugin's `base-url` data
//! file, so the plugin needs no network of its own: Hestia downloads them.

use std::collections::HashMap;

use hestia::proto::content::{
    ContentKind, ContentProject, ContentVersion, ModpackFile, ResolvedModpack, SearchQuery,
    SearchResult, SourceInfo, VersionQuery,
};
use hestia::proto::download::{Checksum, HashAlgorithm};
use hestia::proto::minecraft::Artifact;
use hestia::proto::provider::content::{
    IdQuery, Identify, IdentifyQuery, Info, PackManifestQuery, Project, ProjectQuery,
    ResolveManifest, Search, Versions, VersionsById,
};
use hestia::serde_json::{self, Value};
use hestia::{Error, Plugin, Provide, Provided};

const INDEX: &str = include_str!("../index.json");
const SOURCE: &str = "index";
const DEFAULT_BASE: &str = "https://index.example.com/files";

struct StaticIndex;

fn index() -> Result<Value, Error> {
    Ok(serde_json::from_str(INDEX)?)
}

fn projects() -> Result<Vec<ContentProject>, Error> {
    Ok(serde_json::from_value(index()?["projects"].clone())?)
}

fn check(source: &str) -> Result<(), Error> {
    match source {
        SOURCE => Ok(()),
        other => Err(Error::NotFound(format!("unknown source `{other}`"))),
    }
}

fn versions() -> Result<Vec<ContentVersion>, Error> {
    let base = hestia::data::load::<String>("base-url")?.unwrap_or_else(|| DEFAULT_BASE.into());
    let mut versions: Vec<ContentVersion> = serde_json::from_value(index()?["versions"].clone())?;
    for file in versions.iter_mut().flat_map(|version| &mut version.files) {
        file.artifact.url = format!("{}/{}", base.trim_end_matches('/'), file.artifact.url);
    }
    Ok(versions)
}

fn info(source: &str) -> Result<SourceInfo, Error> {
    check(source)?;
    Ok(SourceInfo {
        name: "Static index".into(),
        kinds: vec![ContentKind::Mod],
        requires_setting: hestia::data::load::<String>("requires-setting")?,
        ..SourceInfo::default()
    })
}

fn search(source: &str, query: SearchQuery) -> Result<SearchResult, Error> {
    check(source)?;
    let needle = query.query.to_lowercase();
    let hits: Vec<ContentProject> = projects()?
        .into_iter()
        .filter(|project| project.title.to_lowercase().contains(&needle))
        .collect();
    Ok(SearchResult {
        total: hits.len() as u32,
        hits,
        ..SearchResult::default()
    })
}

fn project(source: &str, query: ProjectQuery) -> Result<ContentProject, Error> {
    check(source)?;
    projects()?
        .into_iter()
        .find(|found| found.id == query.project || found.slug == query.project)
        .ok_or_else(|| Error::NotFound(format!("no project `{}`", query.project)))
}

fn project_versions(source: &str, query: VersionQuery) -> Result<Vec<ContentVersion>, Error> {
    check(source)?;
    Ok(versions()?
        .into_iter()
        .filter(|version| version.project_id == query.project)
        .collect())
}

fn versions_by_id(source: &str, query: IdQuery) -> Result<Vec<ContentVersion>, Error> {
    check(source)?;
    Ok(versions()?
        .into_iter()
        .filter(|version| query.ids.contains(&version.id))
        .collect())
}

/// A file is the version whose filename is the file's text. The version's name
/// reports what the guest saw of the mount, so a test can tell it is read-only
/// and holds nothing but the files asked about.
fn identify(source: &str, query: IdentifyQuery) -> Result<HashMap<String, ContentVersion>, Error> {
    check(source)?;
    let known = versions()?;
    let listed = std::fs::read_dir("/files").map(Iterator::count).unwrap_or(0);
    let mut found = HashMap::new();
    for file in query.files {
        let text = std::fs::read_to_string(&file.path)
            .map_err(|error| Error::Failed(format!("cannot read {}: {error}", file.path)))?;
        let writable = std::fs::OpenOptions::new()
            .write(true)
            .open(&file.path)
            .is_ok();
        let escaped = std::fs::read_to_string("/files/../data/base-url.json").is_ok();
        let Some(mut version) = known
            .iter()
            .find(|version| version.files[0].artifact.filename == text.trim())
            .cloned()
        else {
            continue;
        };
        version.name = format!("writable={writable} escaped={escaped} listed={listed}");
        found.insert(file.sha1, version);
    }
    Ok(found)
}

/// `{ name, files: [{ path, url, sha1? }] }` into a pack whose relative URLs sit
/// under the base URL; a file with no `sha1` is listed unverified.
fn resolve_manifest(source: &str, query: PackManifestQuery) -> Result<ResolvedModpack, Error> {
    check(source)?;
    let base = hestia::data::load::<String>("base-url")?.unwrap_or_else(|| DEFAULT_BASE.into());
    let text = |value: &Value, key: &str| value[key].as_str().unwrap_or_default().to_string();
    let files = query.manifest["files"]
        .as_array()
        .map(|entries| {
            entries
                .iter()
                .map(|entry| ModpackFile {
                    path: text(entry, "path"),
                    artifact: Artifact {
                        url: match text(entry, "url").as_str() {
                            "" => String::new(),
                            url if url.starts_with("https://") => url.to_string(),
                            relative => format!("{}/{relative}", base.trim_end_matches('/')),
                        },
                        filename: text(entry, "path"),
                        checksum: entry["sha1"].as_str().map(|hex| Checksum {
                            algorithm: HashAlgorithm::Sha1,
                            hex: hex.to_string(),
                        }),
                        ..Artifact::default()
                    },
                    ..ModpackFile::default()
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(ResolvedModpack {
        name: text(&query.manifest, "name"),
        game_version: "1.21".into(),
        files,
        ..ResolvedModpack::default()
    })
}

impl Plugin for StaticIndex {
    fn provide(call: Provide) -> Provided {
        call.on::<Info>(|source, _| info(source))
            .on::<Search>(search)
            .on::<Project>(project)
            .on::<Versions>(project_versions)
            .on::<VersionsById>(versions_by_id)
            .on::<Identify>(identify)
            .on::<ResolveManifest>(resolve_manifest)
            .finish()
    }
}

hestia::plugin!(StaticIndex);
