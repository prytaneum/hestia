//! The Modrinth (`api.modrinth.com/v2`) content provider: search with facets,
//! project detail and project versions. Modrinth's raw JSON is mapped into the normalized
//! `proto::content` types here; the rest of the engine never sees a
//! Modrinth-specific shape. No API key is required.

use std::collections::HashMap;

use anyhow::{Context, Result};
use async_trait::async_trait;
use proto::content::{
    ContentDependency, ContentFile, ContentKind, ContentProject, ContentVersion, DependencyKind,
    GalleryImage, ReleaseChannel, SearchQuery, SearchResult, SearchSort, VersionQuery,
};
use proto::download::{Checksum, HashAlgorithm};
use proto::error::Service;
use proto::minecraft::Artifact;
use serde_json::Value;

use super::json::{non_empty, parse_side, str_field, u64_field};
use super::provider::{ContentProvider, FileRef, LocalFile, UrlRef};

const API: &str = "https://api.modrinth.com/v2";
/// Only for an organization's name: v2 has no organization route.
const API_V3: &str = "https://api.modrinth.com/v3";
const SITE: &str = "modrinth.com";
/// Where Modrinth serves project files from. The path carries both ids, so a
/// pack index's bare download URL is enough to make its file a tracked item.
const CDN: &str = "cdn.modrinth.com";
/// Ids per bulk request. The whole query goes in the URL, so a big pack's ids
/// are chunked rather than sent as one over-long line.
const BULK_LIMIT: usize = 100;
const HASH_LIMIT: usize = 800;

/// The site's project-type path segments (`modrinth.com/<type>/<slug>`) and the
/// kind each names.
const SITE_TYPES: [(&str, Option<ContentKind>); 6] = [
    ("mod", Some(ContentKind::Mod)),
    ("modpack", Some(ContentKind::Modpack)),
    ("resourcepack", Some(ContentKind::ResourcePack)),
    ("shader", Some(ContentKind::Shader)),
    ("datapack", Some(ContentKind::DataPack)),
    ("plugin", Some(ContentKind::Plugin)),
];

/// Which kind each loader implies. Modrinth types a datapack project as `mod`,
/// so the loaders are the only signal of what a project actually publishes; the
/// server-platform loaders are what distinguish a plugin from a mod, since both
/// are jars a `mod`-typed project publishes.
const LOADER_KINDS: [(&str, ContentKind); 18] = [
    ("fabric", ContentKind::Mod),
    ("forge", ContentKind::Mod),
    ("neoforge", ContentKind::Mod),
    ("quilt", ContentKind::Mod),
    ("bukkit", ContentKind::Plugin),
    ("spigot", ContentKind::Plugin),
    ("paper", ContentKind::Plugin),
    ("folia", ContentKind::Plugin),
    ("purpur", ContentKind::Plugin),
    ("sponge", ContentKind::Plugin),
    ("bungeecord", ContentKind::Plugin),
    ("waterfall", ContentKind::Plugin),
    ("velocity", ContentKind::Plugin),
    ("datapack", ContentKind::DataPack),
    ("iris", ContentKind::Shader),
    ("optifine", ContentKind::Shader),
    ("vanilla", ContentKind::ResourcePack),
    ("minecraft", ContentKind::ResourcePack),
];

pub(crate) const ID: &str = "modrinth";

pub struct Modrinth;

/// `cdn.modrinth.com/data/<project>/versions/<version>/<filename>`. Any other
/// host is somebody else's file and stays an untracked direct download.
pub(crate) fn cdn_file(url: &str) -> Option<FileRef> {
    let rest = url.strip_prefix("https://")?;
    let path = rest.strip_prefix(CDN)?.strip_prefix("/data/")?;
    let mut segments = path.split('/');
    let project_id = segments.next().filter(|s| !s.is_empty())?.to_string();
    if segments.next()? != "versions" {
        return None;
    }
    let version_id = segments.next().filter(|s| !s.is_empty())?.to_string();
    Some(FileRef {
        project_id,
        version_id,
    })
}

#[async_trait]
impl ContentProvider for Modrinth {
    fn id(&self) -> &'static str {
        ID
    }

    fn name(&self) -> &'static str {
        "Modrinth"
    }

    fn kinds(&self) -> Vec<ContentKind> {
        SITE_TYPES.iter().filter_map(|(_, kind)| *kind).collect()
    }

    /// `modrinth.com/<type>/<slug>` names a project;
    /// `…/<slug>/version/<number-or-id>` pins one of its versions.
    fn parse_url(&self, url: &str) -> Option<UrlRef> {
        let rest = url
            .strip_prefix("https://")
            .or_else(|| url.strip_prefix("http://"))?;
        let rest = rest.strip_prefix("www.").unwrap_or(rest);
        let path = rest.strip_prefix(SITE)?.strip_prefix('/')?;
        let mut segments = path
            .split(['?', '#'])
            .next()?
            .split('/')
            .filter(|s| !s.is_empty());
        let segment = segments.next()?;
        let (_, kind) = SITE_TYPES.iter().find(|(name, _)| *name == segment)?;
        let project = segments.next()?.to_string();
        let version = match (segments.next(), segments.next()) {
            (Some("version"), Some(v)) => Some(v.to_string()),
            _ => None,
        };
        Some(UrlRef {
            project,
            version,
            kind: *kind,
        })
    }

    async fn search(&self, query: &SearchQuery) -> Result<SearchResult> {
        let limit = if query.limit == 0 {
            10
        } else {
            query.limit.clamp(1, 100)
        };
        let mut params: Vec<(&str, String)> = vec![
            ("facets", build_facets(query)),
            ("index", sort_index(query.sort).to_string()),
            ("offset", query.offset.to_string()),
            ("limit", limit.to_string()),
        ];
        if !query.query.is_empty() {
            params.push(("query", query.query.clone()));
        }
        let root = get_json(&format!("{API}/search"), &params).await?;
        // The facet fixed the result set; a datapack hit still types as `mod`.
        let hits = root
            .get("hits")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .map(|h| parse_hit(self.id(), h, query.kind))
                    .collect()
            })
            .unwrap_or_default();
        Ok(SearchResult {
            hits,
            offset: root
                .get("offset")
                .and_then(Value::as_u64)
                .unwrap_or(query.offset as u64) as u32,
            limit,
            total: root.get("total_hits").and_then(Value::as_u64).unwrap_or(0) as u32,
        })
    }

    async fn project(&self, project: &str, kind: Option<ContentKind>) -> Result<ContentProject> {
        let body = get_json(&format!("{API}/project/{project}"), &[]).await?;
        let mut detail = parse_project(self.id(), &body, kind);
        detail.author = author(&body, project).await;
        Ok(detail)
    }

    /// One `GET /projects?ids=[…]` for the lot. A pack index resolves to a
    /// hundred-odd ids at once and Modrinth rate-limits hard, so the per-project
    /// default would be the slowest part of every pack install.
    async fn projects(&self, ids: &[String]) -> Result<Vec<ContentProject>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut out = Vec::with_capacity(ids.len());
        for chunk in ids.chunks(BULK_LIMIT) {
            let params = [("ids", serde_json::to_string(chunk).unwrap_or_default())];
            let body = get_json(&format!("{API}/projects"), &params).await?;
            if let Some(arr) = body.as_array() {
                out.extend(arr.iter().map(|p| parse_project(self.id(), p, None)));
            }
        }
        Ok(out)
    }

    async fn versions_by_id(&self, ids: &[String]) -> Result<Vec<ContentVersion>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut out = Vec::with_capacity(ids.len());
        for chunk in ids.chunks(BULK_LIMIT) {
            let params = [("ids", serde_json::to_string(chunk).unwrap_or_default())];
            let body = get_json(&format!("{API}/versions"), &params).await?;
            if let Some(arr) = body.as_array() {
                out.extend(arr.iter().map(|v| parse_version(self.id(), v)));
            }
        }
        Ok(out)
    }

    async fn identify(&self, files: &[LocalFile]) -> Result<HashMap<String, ContentVersion>> {
        let mut out = HashMap::with_capacity(files.len());
        for chunk in files.chunks(HASH_LIMIT) {
            let hashes: Vec<&str> = chunk.iter().map(|f| f.sha1.as_str()).collect();
            let body = serde_json::json!({ "hashes": hashes, "algorithm": "sha1" });
            let found = post_json(&format!("{API}/version_files"), &body).await?;
            if let Some(map) = found.as_object() {
                out.extend(
                    map.iter()
                        .map(|(sha1, v)| (sha1.clone(), parse_version(self.id(), v))),
                );
            }
        }
        Ok(out)
    }

    async fn versions(&self, query: &VersionQuery) -> Result<Vec<ContentVersion>> {
        let mut params: Vec<(&str, String)> = Vec::new();
        if let Some(loader) = non_empty(&query.loader) {
            params.push(("loaders", json_array(loader)));
        }
        if let Some(game) = non_empty(&query.game_version) {
            params.push(("game_versions", json_array(game)));
        }
        let arr = get_json(&format!("{API}/project/{}/version", query.project), &params).await?;
        Ok(arr
            .as_array()
            .map(|a| a.iter().map(|v| parse_version(self.id(), v)).collect())
            .unwrap_or_default())
    }
}

/// Who a project detail is "by": the organization that owns it, else the team
/// member holding the `Owner` role. It is presentation only, so a lookup that
/// fails leaves the byline empty rather than failing the project.
async fn author(body: &Value, project: &str) -> String {
    let organization = str_field(body, "organization");
    if !organization.is_empty() {
        match get_json(&format!("{API_V3}/organization/{organization}"), &[]).await {
            Ok(org) => {
                let name = str_field(&org, "name");
                if !name.is_empty() {
                    return name;
                }
            }
            Err(e) => {
                tracing::debug!(organization, error = %format!("{e:#}"), "no modrinth organization")
            }
        }
    }
    match get_json(&format!("{API}/project/{project}/members"), &[]).await {
        Ok(members) => owner(&members),
        Err(e) => {
            tracing::debug!(project, error = %format!("{e:#}"), "no modrinth team members");
            String::new()
        }
    }
}

/// The member holding `Owner`, falling back to the first listed — a team owned
/// by an organization has no owning member at all.
fn owner(members: &Value) -> String {
    let members = members.as_array().map(Vec::as_slice).unwrap_or_default();
    members
        .iter()
        .find(|m| str_field(m, "role") == "Owner")
        .or_else(|| members.first())
        .map(|m| str_field(m.get("user").unwrap_or(&Value::Null), "username"))
        .unwrap_or_default()
}

fn parse_hit(source: &str, hit: &Value, requested: ContentKind) -> ContentProject {
    ContentProject {
        source: source.to_string(),
        id: str_field(hit, "project_id"),
        slug: str_field(hit, "slug"),
        kind: requested,
        kinds: kinds_from_loaders(
            &str_array(hit, "categories"),
            parse_kind(&str_field(hit, "project_type")),
        ),
        title: str_field(hit, "title"),
        description: str_field(hit, "description"),
        body: String::new(),
        author: str_field(hit, "author"),
        downloads: u64_field(hit, "downloads"),
        follows: u64_field(hit, "follows"),
        categories: categories(hit),
        icon_url: str_field(hit, "icon_url"),
        gallery: hit
            .get("gallery")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(|url| GalleryImage {
                        url: url.to_string(),
                        ..GalleryImage::default()
                    })
                    .collect()
            })
            .unwrap_or_default(),
        client_side: parse_side(&str_field(hit, "client_side")),
        server_side: parse_side(&str_field(hit, "server_side")),
    }
}

fn parse_project(source: &str, body: &Value, requested: Option<ContentKind>) -> ContentProject {
    let parsed = parse_kind(&str_field(body, "project_type"));
    let kinds = kinds_from_loaders(&str_array(body, "loaders"), parsed);
    ContentProject {
        source: source.to_string(),
        id: str_field(body, "id"),
        slug: str_field(body, "slug"),
        kind: requested.filter(|k| kinds.contains(k)).unwrap_or(parsed),
        kinds,
        title: str_field(body, "title"),
        description: str_field(body, "description"),
        body: str_field(body, "body"),
        // A project payload names no author — only the search index does.
        author: String::new(),
        categories: categories(body),
        downloads: u64_field(body, "downloads"),
        // The search index calls this `follows`; a project payload does not.
        follows: u64_field(body, "followers"),
        icon_url: str_field(body, "icon_url"),
        gallery: body
            .get("gallery")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .map(|g| GalleryImage {
                        url: str_field(g, "url"),
                        featured: g.get("featured").and_then(Value::as_bool).unwrap_or(false),
                        title: str_field(g, "title"),
                        description: str_field(g, "description"),
                    })
                    .collect()
            })
            .unwrap_or_default(),
        client_side: parse_side(&str_field(body, "client_side")),
        server_side: parse_side(&str_field(body, "server_side")),
    }
}

fn parse_version(source: &str, v: &Value) -> ContentVersion {
    let files = v
        .get("files")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .map(|f| ContentFile {
                    artifact: Artifact {
                        url: str_field(f, "url"),
                        filename: str_field(f, "filename"),
                        size: u64_field(f, "size"),
                        checksum: f
                            .get("hashes")
                            .and_then(|h| h.get("sha1"))
                            .and_then(Value::as_str)
                            .filter(|s| !s.is_empty())
                            .map(|hex| Checksum {
                                algorithm: HashAlgorithm::Sha1,
                                hex: hex.to_string(),
                            }),
                    },
                    primary: f.get("primary").and_then(Value::as_bool).unwrap_or(false),
                })
                .collect()
        })
        .unwrap_or_default();
    let dependencies = v
        .get("dependencies")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .map(|d| ContentDependency {
                    project_id: str_field(d, "project_id"),
                    version_id: str_field(d, "version_id"),
                    kind: parse_dependency_kind(&str_field(d, "dependency_type")),
                })
                .collect()
        })
        .unwrap_or_default();
    ContentVersion {
        source: source.to_string(),
        id: str_field(v, "id"),
        project_id: str_field(v, "project_id"),
        name: str_field(v, "name"),
        version_number: str_field(v, "version_number"),
        channel: parse_channel(&str_field(v, "version_type")),
        game_versions: str_array(v, "game_versions"),
        loaders: str_array(v, "loaders"),
        featured: v.get("featured").and_then(Value::as_bool).unwrap_or(false),
        date_published: str_field(v, "date_published"),
        downloads: u64_field(v, "downloads"),
        files,
        dependencies,
    }
}

/// Modrinth facets are a JSON array of single-element arrays (each AND'd). The
/// loader is expressed as a `categories:` facet, as Modrinth does. Only set
/// filters are included.
fn build_facets(query: &SearchQuery) -> String {
    let mut facets: Vec<Vec<String>> =
        vec![vec![format!("project_type:{}", project_type(query.kind))]];
    if let Some(loader) = non_empty(&query.loader) {
        facets.push(vec![format!("categories:{loader}")]);
    }
    if let Some(game) = non_empty(&query.game_version) {
        facets.push(vec![format!("versions:{game}")]);
    }
    for category in &query.categories {
        if !category.is_empty() {
            facets.push(vec![format!("categories:{category}")]);
        }
    }
    serde_json::to_string(&facets).unwrap_or_default()
}

fn project_type(kind: ContentKind) -> &'static str {
    match kind {
        ContentKind::Mod => "mod",
        ContentKind::Modpack => "modpack",
        ContentKind::ResourcePack => "resourcepack",
        ContentKind::Shader => "shader",
        ContentKind::DataPack => "datapack",
        ContentKind::Plugin => "plugin",
    }
}

fn parse_kind(s: &str) -> ContentKind {
    match s {
        "modpack" => ContentKind::Modpack,
        "resourcepack" => ContentKind::ResourcePack,
        "shader" => ContentKind::Shader,
        "datapack" => ContentKind::DataPack,
        "plugin" => ContentKind::Plugin,
        _ => ContentKind::Mod,
    }
}

/// Every kind a project's loaders imply, `fallback` when they imply none.
fn kinds_from_loaders(loaders: &[String], fallback: ContentKind) -> Vec<ContentKind> {
    let mut kinds: Vec<ContentKind> = Vec::new();
    for loader in loaders {
        if let Some((_, kind)) = LOADER_KINDS
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(loader))
        {
            if !kinds.contains(kind) {
                kinds.push(*kind);
            }
        }
    }
    if kinds.is_empty() {
        kinds.push(fallback);
    }
    kinds
}

fn sort_index(sort: SearchSort) -> &'static str {
    match sort {
        SearchSort::Relevance => "relevance",
        SearchSort::Downloads => "downloads",
        SearchSort::Follows => "follows",
        SearchSort::Newest => "newest",
        SearchSort::Updated => "updated",
    }
}

fn parse_channel(s: &str) -> ReleaseChannel {
    match s {
        "beta" => ReleaseChannel::Beta,
        "alpha" => ReleaseChannel::Alpha,
        _ => ReleaseChannel::Release,
    }
}

fn parse_dependency_kind(s: &str) -> DependencyKind {
    match s {
        "optional" => DependencyKind::Optional,
        "incompatible" => DependencyKind::Incompatible,
        "embedded" => DependencyKind::Embedded,
        _ => DependencyKind::Required,
    }
}

fn categories(v: &Value) -> Vec<String> {
    let display = str_array(v, "display_categories");
    if display.is_empty() {
        str_array(v, "categories")
    } else {
        display
    }
}

fn str_array(v: &Value, key: &str) -> Vec<String> {
    v.get(key)
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(String::from)
                .collect()
        })
        .unwrap_or_default()
}

fn json_array(value: &str) -> String {
    serde_json::to_string(&[value]).unwrap_or_default()
}

async fn get_json(url: &str, query: &[(&str, String)]) -> Result<Value> {
    tracing::debug!(url, "modrinth GET");
    let response = crate::net::send(
        Some(Service::Modrinth),
        crate::net::client().get(url).query(query),
    )
    .await?;
    let response = crate::net::require_success(Service::Modrinth, response)?;
    response
        .json()
        .await
        .with_context(|| format!("{url} returned malformed JSON"))
}

async fn post_json(url: &str, body: &Value) -> Result<Value> {
    tracing::debug!(url, "modrinth POST");
    let response = crate::net::send(
        Some(Service::Modrinth),
        crate::net::client().post(url).json(body),
    )
    .await?;
    let response = crate::net::require_success(Service::Modrinth, response)?;
    response
        .json()
        .await
        .with_context(|| format!("{url} returned malformed JSON"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_cdn_file_url_carries_both_ids() {
        let parsed = cdn_file(
            "https://cdn.modrinth.com/data/AANobbMI/versions/HFxNoSNH/sodium-fabric-0.6.0.jar",
        )
        .unwrap();
        assert_eq!(parsed.project_id, "AANobbMI");
        assert_eq!(parsed.version_id, "HFxNoSNH");
    }

    #[test]
    fn a_foreign_download_url_is_not_guessed_at() {
        for url in [
            "https://example.com/data/AAA/versions/BBB/x.jar",
            "https://cdn.modrinth.com/data/AAA/files/BBB/x.jar",
            "https://cdn.modrinth.com/data/AAA",
        ] {
            assert!(cdn_file(url).is_none(), "should skip {url}");
        }
    }

    #[test]
    fn facets_include_only_set_filters() {
        let query = SearchQuery {
            kind: ContentKind::Mod,
            loader: Some("fabric".into()),
            game_version: Some("1.21.1".into()),
            categories: vec!["optimization".into()],
            ..SearchQuery::default()
        };
        assert_eq!(
            build_facets(&query),
            r#"[["project_type:mod"],["categories:fabric"],["versions:1.21.1"],["categories:optimization"]]"#
        );

        let bare = SearchQuery {
            kind: ContentKind::Modpack,
            ..SearchQuery::default()
        };
        assert_eq!(build_facets(&bare), r#"[["project_type:modpack"]]"#);
    }

    #[test]
    fn sort_maps_to_modrinth_index() {
        assert_eq!(sort_index(SearchSort::Relevance), "relevance");
        assert_eq!(sort_index(SearchSort::Downloads), "downloads");
        assert_eq!(sort_index(SearchSort::Follows), "follows");
        assert_eq!(sort_index(SearchSort::Newest), "newest");
        assert_eq!(sort_index(SearchSort::Updated), "updated");
    }

    #[test]
    fn site_urls_parse_to_project_refs() {
        let m = Modrinth;
        for url in [
            "https://modrinth.com/mod/sodium",
            "http://modrinth.com/mod/sodium",
            "https://www.modrinth.com/mod/sodium/",
            "https://modrinth.com/mod/sodium?query=x#gallery",
            "https://modrinth.com/mod/sodium/versions",
        ] {
            let parsed = m
                .parse_url(url)
                .unwrap_or_else(|| panic!("should parse {url}"));
            assert_eq!(parsed.project, "sodium");
            assert_eq!(parsed.version, None, "{url}");
        }
        let pinned = m
            .parse_url("https://modrinth.com/mod/sodium/version/mc1.21.1-0.8.12-fabric")
            .unwrap();
        assert_eq!(pinned.version.as_deref(), Some("mc1.21.1-0.8.12-fabric"));

        assert!(m.parse_url("https://example.com/mod/sodium").is_none());
        assert!(m
            .parse_url("https://modrinth.com/user/jellysquid3")
            .is_none());
        assert!(m.parse_url("modrinth.com/mod/sodium").is_none());
        assert!(m.parse_url("https://modrinth.com/mod").is_none());
    }

    #[test]
    fn site_url_carries_the_kind_its_path_names() {
        let m = Modrinth;
        assert_eq!(
            m.parse_url("https://modrinth.com/datapack/terralith")
                .unwrap()
                .kind,
            Some(ContentKind::DataPack)
        );
        assert_eq!(
            m.parse_url("https://modrinth.com/shader/complementary")
                .unwrap()
                .kind,
            Some(ContentKind::Shader)
        );
        assert_eq!(
            m.parse_url("https://modrinth.com/plugin/luckperms")
                .unwrap()
                .kind,
            Some(ContentKind::Plugin)
        );
    }

    #[test]
    fn loaders_map_to_every_kind_a_project_publishes() {
        let of = |loaders: &[&str]| {
            kinds_from_loaders(
                &loaders.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
                ContentKind::Mod,
            )
        };
        assert_eq!(of(&["datapack"]), vec![ContentKind::DataPack]);
        assert_eq!(
            of(&["datapack", "fabric", "neoforge"]),
            vec![ContentKind::DataPack, ContentKind::Mod]
        );
        assert_eq!(of(&["iris", "optifine"]), vec![ContentKind::Shader]);
        assert_eq!(of(&["minecraft"]), vec![ContentKind::ResourcePack]);
        assert_eq!(
            of(&["paper", "folia", "bukkit"]),
            vec![ContentKind::Plugin],
            "the server platforms all name one kind"
        );
        assert_eq!(
            of(&["datapack", "paper"]),
            vec![ContentKind::DataPack, ContentKind::Plugin],
            "a project may publish both"
        );
        assert_eq!(of(&["nilloader"]), vec![ContentKind::Mod], "falls back");
    }

    #[test]
    fn a_datapack_hit_is_typed_by_the_query_not_modrinths_project_type() {
        // Modrinth answers `project_type: "mod"` for every datapack project.
        let hit = json!({
            "project_id": "8oi3bsk5",
            "slug": "terralith",
            "project_type": "mod",
            "title": "Terralith",
            "categories": ["datapack", "fabric", "worldgen"],
        });
        let parsed = parse_hit("modrinth", &hit, ContentKind::DataPack);
        assert_eq!(parsed.kind, ContentKind::DataPack);
        assert!(parsed.kinds.contains(&ContentKind::Mod));

        let as_mod = parse_hit("modrinth", &hit, ContentKind::Mod);
        assert_eq!(as_mod.kind, ContentKind::Mod);
    }

    #[test]
    fn project_detail_takes_the_requested_kind_only_when_published() {
        let body = json!({
            "id": "8oi3bsk5",
            "slug": "terralith",
            "project_type": "mod",
            "title": "Terralith",
            "loaders": ["datapack", "fabric"],
        });
        assert_eq!(
            parse_project("modrinth", &body, Some(ContentKind::DataPack)).kind,
            ContentKind::DataPack
        );
        assert_eq!(
            parse_project("modrinth", &body, Some(ContentKind::Shader)).kind,
            ContentKind::Mod
        );
        assert_eq!(
            parse_project("modrinth", &body, None).kind,
            ContentKind::Mod
        );
    }

    #[test]
    fn project_detail_counts_followers() {
        let body = json!({
            "id": "AANobbMI",
            "project_type": "mod",
            "downloads": 71_000_000,
            "followers": 39_396,
        });
        let parsed = parse_project("modrinth", &body, None);
        assert_eq!(parsed.downloads, 71_000_000);
        assert_eq!(parsed.follows, 39_396);
    }

    #[test]
    fn the_byline_is_the_owning_member() {
        let members = json!([
            { "role": "Lead Developer", "user": { "username": "IMS" } },
            { "role": "Owner", "user": { "username": "coderbot" } },
        ]);
        assert_eq!(owner(&members), "coderbot");

        let organization_owned = json!([
            { "role": "Maintainer", "user": { "username": "IMS" } },
            { "role": "Project Lead", "user": { "username": "jellysquid3" } },
        ]);
        assert_eq!(owner(&organization_owned), "IMS");

        assert_eq!(owner(&json!([])), "");
        assert_eq!(owner(&Value::Null), "");
    }

    #[test]
    fn channel_and_dependency_strings_map() {
        assert_eq!(parse_channel("beta"), ReleaseChannel::Beta);
        assert_eq!(parse_channel("alpha"), ReleaseChannel::Alpha);
        assert_eq!(parse_channel("release"), ReleaseChannel::Release);
        assert_eq!(parse_channel("weird"), ReleaseChannel::Release);

        assert_eq!(parse_dependency_kind("optional"), DependencyKind::Optional);
        assert_eq!(
            parse_dependency_kind("incompatible"),
            DependencyKind::Incompatible
        );
        assert_eq!(parse_dependency_kind("embedded"), DependencyKind::Embedded);
        assert_eq!(parse_dependency_kind("required"), DependencyKind::Required);
        assert_eq!(parse_dependency_kind("unknown"), DependencyKind::Required);
    }
}
