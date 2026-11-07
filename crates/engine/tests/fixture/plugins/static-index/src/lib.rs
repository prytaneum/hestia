//! A content source that serves a JSON index compiled into the component.
//! Files in the index name a path relative to the plugin's `base-url` data
//! file, so the plugin needs no network of its own: Hestia downloads them.

use hestia::proto::content::{
    ContentKind, ContentProject, ContentVersion, SearchQuery, SearchResult, SourceInfo,
    VersionQuery,
};
use hestia::serde_json::{self, Value};
use hestia::{Error, Plugin};

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

impl Plugin for StaticIndex {
    fn source_info(source: &str) -> Result<SourceInfo, Error> {
        check(source)?;
        Ok(SourceInfo {
            name: "Static index".into(),
            kinds: vec![ContentKind::Mod],
            ..SourceInfo::default()
        })
    }

    fn source_search(source: &str, query: SearchQuery) -> Result<SearchResult, Error> {
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

    fn source_project(
        source: &str,
        project: &str,
        _kind: Option<ContentKind>,
    ) -> Result<ContentProject, Error> {
        check(source)?;
        projects()?
            .into_iter()
            .find(|found| found.id == project || found.slug == project)
            .ok_or_else(|| Error::NotFound(format!("no project `{project}`")))
    }

    fn source_versions(source: &str, query: VersionQuery) -> Result<Vec<ContentVersion>, Error> {
        check(source)?;
        Ok(versions()?
            .into_iter()
            .filter(|version| version.project_id == query.project)
            .collect())
    }

    fn source_versions_by_id(source: &str, ids: Vec<String>) -> Result<Vec<ContentVersion>, Error> {
        check(source)?;
        Ok(versions()?
            .into_iter()
            .filter(|version| ids.contains(&version.id))
            .collect())
    }
}

hestia::plugin!(StaticIndex);
