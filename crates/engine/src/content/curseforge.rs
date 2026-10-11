//! The CurseForge (`api.curseforge.com/v1`) content provider: search, project
//! detail, a project's files, and modpack manifest resolution. CurseForge's raw
//! JSON is mapped into the normalized `proto::content` types here; the rest of
//! the engine never sees a CurseForge-specific shape.
//!
//! Every request must carry an `x-api-key`, so the source serves only once a
//! key resolves — the `content.curseforge-key` setting, else one baked in at
//! build time — and `Content` leaves it out of the source list until then.

use std::collections::HashMap;
use std::sync::RwLock;

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use proto::content::{
    ContentDependency, ContentFile, ContentKind, ContentProject, ContentVersion, DependencyKind,
    GalleryImage, ModpackFile, ReleaseChannel, ResolvedModpack, SearchQuery, SearchResult,
    SearchSort, SideSupport, VersionQuery,
};
use proto::download::{Checksum, HashAlgorithm};
use proto::error::{ArchiveFault, ErrorInfo, Service};
use proto::minecraft::Artifact;
use serde_json::{json, Value};
use std::io::{Cursor, Read};

use crate::config::ContentSettings;

use super::pack;
use super::provider::{ContentProvider, FileRef, LocalFile, UrlRef};

const API: &str = "https://api.curseforge.com/v1";
const SITE: &str = "curseforge.com";
pub(crate) const ID: &str = "curseforge";
const PACK_MANIFEST: &str = "minecraftModpack";
const NAME: &str = "CurseForge";

/// CurseForge's own id for Minecraft; every catalogue call is scoped to it.
const GAME: u32 = 432;

/// Names its files by id, so only this module resolves them into downloads;
/// the archive's override trees are `pack.rs`'s half.
const MANIFEST: &str = "manifest.json";

/// Baked in by a distributor, empty in a plain `cargo build`; the
/// `content.curseforge-key` setting overrides it without a rebuild.
const BUILD_KEY: &str = match option_env!("HESTIA_CURSEFORGE_API_KEY") {
    Some(key) => key,
    None => "",
};

/// The API pages at 50 and refuses a window past 10 000 results.
const PAGE: u32 = 50;
const MAX_INDEX: u32 = 10_000;

/// How deep a version lookup pages; the query is already filtered, so a pick
/// never needs more.
const MAX_FILES: usize = 200;

/// How many ids one batched `mods`/`mods/files` POST carries.
const BATCH: usize = 50;

/// A *class* is what hestia calls a kind. Bukkit plugins (class 5) are absent:
/// the API no longer serves them.
const CLASSES: [(ContentKind, u64); 5] = [
    (ContentKind::Mod, 6),
    (ContentKind::ResourcePack, 12),
    (ContentKind::Modpack, 4471),
    (ContentKind::Shader, 6552),
    (ContentKind::DataPack, 6945),
];

/// `ModLoaderType` — CurseForge only models modloaders, so a kind it types by
/// class instead (a datapack, a world) has no entry here.
const LOADERS: [(&str, u32); 4] = [("forge", 1), ("fabric", 4), ("quilt", 5), ("neoforge", 6)];

/// A file's `gameVersions` mixes these with Minecraft versions and side
/// markers; anything else carrying a dot is taken for a game version.
const LOADER_NAMES: [&str; 6] = [
    "neoforge",
    "forge",
    "fabric",
    "quilt",
    "cauldron",
    "liteloader",
];

/// The site's project-type path segments (`curseforge.com/minecraft/<type>/<slug>`).
const SITE_TYPES: [(&str, ContentKind); 5] = [
    ("mc-mods", ContentKind::Mod),
    ("texture-packs", ContentKind::ResourcePack),
    ("shaders", ContentKind::Shader),
    ("modpacks", ContentKind::Modpack),
    ("data-packs", ContentKind::DataPack),
];

/// Where a modpack manifest's files land, by the class of the project each one
/// belongs to. A world is the odd one out: it is a save, not loader content.
const CLASS_DIRS: [(u64, &str); 5] = [
    (6, "mods"),
    (12, "resourcepacks"),
    (6552, "shaderpacks"),
    (6945, "datapacks"),
    (17, "saves"),
];

pub struct CurseForge {
    key: RwLock<String>,
}

impl Default for CurseForge {
    fn default() -> Self {
        CurseForge {
            key: RwLock::new(BUILD_KEY.to_string()),
        }
    }
}

#[async_trait]
impl ContentProvider for CurseForge {
    fn id(&self) -> &'static str {
        ID
    }

    fn name(&self) -> &'static str {
        NAME
    }

    fn kinds(&self) -> Vec<ContentKind> {
        CLASSES.iter().map(|(kind, _)| *kind).collect()
    }

    fn available(&self) -> bool {
        !self.key().is_empty()
    }

    fn configure(&self, settings: &ContentSettings) {
        let key = match settings.curseforge_key.trim() {
            "" => BUILD_KEY,
            configured => configured,
        };
        let mut current = self.key.write().unwrap();
        if *current != key {
            tracing::info!(
                source = ID,
                configured = !key.is_empty(),
                "content source key"
            );
            *current = key.to_string();
        }
    }

    /// `curseforge.com/minecraft/<type>/<slug>` names a project;
    /// `…/<slug>/files/<id>` (or `…/download/<id>`) pins one of its files.
    fn parse_url(&self, url: &str) -> Option<UrlRef> {
        let rest = url
            .strip_prefix("https://")
            .or_else(|| url.strip_prefix("http://"))?;
        let rest = rest.strip_prefix("www.").unwrap_or(rest);
        let rest = rest.strip_prefix("legacy.").unwrap_or(rest);
        let path = rest.strip_prefix(SITE)?.strip_prefix('/')?;
        let mut segments = path
            .split(['?', '#'])
            .next()?
            .split('/')
            .filter(|s| !s.is_empty());
        if segments.next()? != "minecraft" {
            return None;
        }
        let segment = segments.next()?;
        let (_, kind) = SITE_TYPES.iter().find(|(name, _)| *name == segment)?;
        let project = segments.next()?.to_string();
        let version = match (segments.next(), segments.next()) {
            (Some("files" | "download"), Some(id)) => Some(id.to_string()),
            _ => None,
        };
        Some(UrlRef {
            project,
            version,
            kind: Some(*kind),
        })
    }

    /// `…/files/<id / 1000>/<id % 1000>/<name>` — the file, never the project,
    /// which `versions_by_id` answers instead.
    fn parse_file_url(&self, url: &str) -> Option<FileRef> {
        let rest = url.strip_prefix("https://")?;
        let (host, path) = rest.split_once('/')?;
        if !host.ends_with("forgecdn.net") {
            return None;
        }
        let mut segments = path.strip_prefix("files/")?.split('/');
        let high: u64 = segments.next()?.parse().ok()?;
        let low: u64 = segments.next()?.parse().ok()?;
        if low >= 1000 || segments.next().is_none() {
            return None;
        }
        Some(FileRef {
            project_id: String::new(),
            version_id: (high * 1000 + low).to_string(),
        })
    }

    async fn search(&self, query: &SearchQuery) -> Result<SearchResult> {
        let limit = if query.limit == 0 {
            10
        } else {
            query.limit.clamp(1, PAGE)
        };
        let Some(class) = class_id(query.kind) else {
            return Ok(SearchResult {
                hits: Vec::new(),
                offset: query.offset,
                limit,
                total: 0,
            });
        };
        let offset = query.offset.min(MAX_INDEX.saturating_sub(limit));
        let mut params = vec![
            ("gameId", GAME.to_string()),
            ("classId", class.to_string()),
            ("index", offset.to_string()),
            ("pageSize", limit.to_string()),
        ];
        if !query.query.is_empty() {
            params.push(("searchFilter", query.query.clone()));
        }
        // With no sort field CurseForge orders by relevance, which is what
        // hestia's `Relevance` means and no sort field expresses.
        if let Some(field) = sort_field(query.sort) {
            params.push(("sortField", field.to_string()));
            params.push(("sortOrder", "desc".to_string()));
        }
        if let Some(loader) = query.loader.as_deref().and_then(loader_type) {
            params.push(("modLoaderType", loader.to_string()));
        }
        if let Some(game) = non_empty(&query.game_version) {
            params.push(("gameVersion", game.to_string()));
        }
        let categories = self.category_ids(class, &query.categories).await?;
        if !categories.is_empty() {
            params.push(("categoryIds", json!(categories).to_string()));
        }

        let body = self.get("/mods/search", &params).await?;
        let hits = data(&body)
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|m| parse_mod(m, Some(query.kind), String::new()))
                    .collect()
            })
            .unwrap_or_default();
        let pagination = body.get("pagination");
        Ok(SearchResult {
            hits,
            offset,
            limit,
            total: pagination
                .map(|p| u64_field(p, "totalCount"))
                .unwrap_or_default() as u32,
        })
    }

    async fn project(&self, project: &str, kind: Option<ContentKind>) -> Result<ContentProject> {
        let found = self.find_mod(project, kind).await?;
        let id = u64_field(&found, "id");
        // The long description is its own endpoint on CurseForge, and it is
        // presentation only — a failure must not fail the lookup.
        let body = match self.get(&format!("/mods/{id}/description"), &[]).await {
            Ok(body) => data(&body).as_str().unwrap_or_default().to_string(),
            Err(e) => {
                tracing::debug!(project = id, error = %format!("{e:#}"), "no curseforge description");
                String::new()
            }
        };
        Ok(parse_mod(&found, kind, body))
    }

    async fn versions(&self, query: &VersionQuery) -> Result<Vec<ContentVersion>> {
        let id = self.mod_id(&query.project).await?;
        let mut params = vec![("pageSize", PAGE.to_string())];
        if let Some(game) = non_empty(&query.game_version) {
            params.push(("gameVersion", game.to_string()));
        }
        if let Some(loader) = query.loader.as_deref().and_then(loader_type) {
            params.push(("modLoaderType", loader.to_string()));
        }

        let mut versions: Vec<ContentVersion> = Vec::new();
        let mut index = 0u32;
        loop {
            let mut page = params.clone();
            page.push(("index", index.to_string()));
            let body = self.get(&format!("/mods/{id}/files"), &page).await?;
            let files = data(&body).as_array().cloned().unwrap_or_default();
            let fetched = files.len();
            versions.extend(files.iter().map(parse_file));
            let total = body
                .get("pagination")
                .map(|p| u64_field(p, "totalCount"))
                .unwrap_or_default() as usize;
            index += PAGE;
            if fetched == 0 || versions.len() >= total.min(MAX_FILES) {
                break;
            }
        }

        // CurseForge types datapacks by class, so their files name no loader
        // for the `datapack` pseudo-loader a version pick filters on.
        if query.loader.as_deref() == Some("datapack") {
            for version in &mut versions {
                version.loaders.push("datapack".to_string());
            }
        }
        versions.sort_by(|a, b| b.date_published.cmp(&a.date_published));
        Ok(versions)
    }

    /// Bulk by id, so a pack index costs two requests rather than two hundred.
    async fn projects(&self, ids: &[String]) -> Result<Vec<ContentProject>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let mut out = Vec::with_capacity(ids.len());
        for chunk in ids.chunks(BATCH) {
            let body = self.post("/mods", json!({ "modIds": chunk })).await?;
            if let Some(arr) = data(&body).as_array() {
                out.extend(arr.iter().map(|m| parse_mod(m, None, String::new())));
            }
        }
        Ok(out)
    }

    async fn versions_by_id(&self, ids: &[String]) -> Result<Vec<ContentVersion>> {
        Ok(self.files(ids).await?.iter().map(parse_file).collect())
    }

    async fn identify(&self, files: &[LocalFile]) -> Result<HashMap<String, ContentVersion>> {
        let mut sha1_of = HashMap::with_capacity(files.len());
        for file in files {
            let bytes = std::fs::read(&file.path)
                .with_context(|| format!("cannot read {}", file.path.display()))?;
            sha1_of.insert(u64::from(fingerprint(&bytes)), file.sha1.clone());
        }
        let fingerprints: Vec<u64> = sha1_of.keys().copied().collect();
        let mut out = HashMap::with_capacity(files.len());
        for chunk in fingerprints.chunks(BATCH) {
            let body = self
                .post(
                    &format!("/fingerprints/{GAME}"),
                    json!({ "fingerprints": chunk }),
                )
                .await?;
            let matches = data(&body).get("exactMatches").and_then(Value::as_array);
            for file in matches.into_iter().flatten().filter_map(|m| m.get("file")) {
                if let Some(sha1) = sha1_of.get(&u64_field(file, "fileFingerprint")) {
                    out.insert(sha1.clone(), parse_file(file));
                }
            }
        }
        Ok(out)
    }

    async fn resolve_modpack(&self, version_id: &str) -> Result<ResolvedModpack> {
        Ok(self.fetch_modpack(version_id).await?.0)
    }

    async fn fetch_modpack(&self, version_id: &str) -> Result<(ResolvedModpack, Vec<u8>)> {
        let file = self
            .files(&[version_id.to_string()])
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| {
                anyhow::Error::from(ErrorInfo::VersionNotFound {
                    reference: version_id.to_string(),
                })
            })?;
        let display = str_field(&file, "displayName");
        let url = str_field(&file, "downloadUrl");
        if url.is_empty() {
            bail!(ErrorInfo::ContentDownloadBlocked {
                title: display,
                source: ID.to_string(),
            });
        }

        let bytes = self.download(&url).await?;
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes.clone()))
            .context("the modpack archive could not be read")?;
        let manifest: Value = {
            let mut entry = archive.by_name(MANIFEST).map_err(|_| {
                anyhow::Error::from(ErrorInfo::NotAModpack {
                    reference: version_id.to_string(),
                })
            })?;
            let mut text = String::new();
            entry
                .read_to_string(&mut text)
                .context("manifest.json is malformed")?;
            serde_json::from_str(&text).context("manifest.json is malformed")?
        };

        let mut resolved = self.resolve_manifest(&manifest).await?;
        resolved.version_id = u64_field(&file, "id").to_string();
        resolved.project_id = u64_field(&file, "modId").to_string();
        // The manifest's own `version` is free text; an update compares against
        // what the platform published.
        resolved.version_number = display;
        Ok((resolved, bytes))
    }

    async fn resolve_manifest(&self, manifest: &Value) -> Result<ResolvedModpack> {
        let mut resolved = parse_manifest(manifest)?;
        resolved.source = ID.to_string();
        resolved.files = self.resolve_manifest_files(manifest).await?;
        Ok(resolved)
    }
}

impl CurseForge {
    fn key(&self) -> String {
        self.key.read().unwrap().clone()
    }

    async fn get(&self, path: &str, query: &[(&str, String)]) -> Result<Value> {
        let key = self.key();
        if key.is_empty() {
            bail!(ErrorInfo::ContentSourceUnavailable {
                source: ID.to_string()
            });
        }
        let url = format!("{API}{path}");
        tracing::debug!(url, "curseforge GET");
        let response = crate::net::send(
            Some(Service::CurseForge),
            crate::net::client()
                .get(&url)
                .query(query)
                .header("x-api-key", key),
        )
        .await?;
        json(path, response).await
    }

    async fn post(&self, path: &str, body: Value) -> Result<Value> {
        let key = self.key();
        if key.is_empty() {
            bail!(ErrorInfo::ContentSourceUnavailable {
                source: ID.to_string()
            });
        }
        let url = format!("{API}{path}");
        tracing::debug!(url, "curseforge POST");
        let response = crate::net::send(
            Some(Service::CurseForge),
            crate::net::client()
                .post(&url)
                .header("x-api-key", key)
                .json(&body),
        )
        .await?;
        json(path, response).await
    }

    async fn download(&self, url: &str) -> Result<Vec<u8>> {
        tracing::debug!(url, "curseforge modpack GET");
        let response =
            crate::net::send(Some(Service::CurseForge), crate::net::client().get(url)).await?;
        if !response.status().is_success() {
            return Err(upstream(format!(
                "{url}: HTTP {}",
                response.status().as_u16()
            )));
        }
        Ok(response
            .bytes()
            .await
            .map_err(|e| upstream(format!("{url}: {e}")))?
            .to_vec())
    }

    /// The numeric mod id a reference names — itself when it is already one,
    /// else the id of the project with that slug.
    async fn mod_id(&self, reference: &str) -> Result<u64> {
        if is_numeric(reference) {
            return reference.parse().context("invalid project id");
        }
        Ok(u64_field(&self.find_mod(reference, None).await?, "id"))
    }

    /// A project by id or slug. There is no by-slug endpoint, and a slug is
    /// unique only within a class, so a slug searches with one where known.
    async fn find_mod(&self, reference: &str, kind: Option<ContentKind>) -> Result<Value> {
        if is_numeric(reference) {
            let body = self.get(&format!("/mods/{reference}"), &[]).await?;
            return Ok(data(&body).clone());
        }
        let mut params = vec![
            ("gameId", GAME.to_string()),
            ("slug", reference.to_string()),
            ("pageSize", PAGE.to_string()),
        ];
        if let Some(class) = kind.and_then(class_id) {
            params.push(("classId", class.to_string()));
        }
        let body = self.get("/mods/search", &params).await?;
        let hits = data(&body).as_array().cloned().unwrap_or_default();
        hits.iter()
            .find(|m| str_field(m, "slug").eq_ignore_ascii_case(reference))
            .or_else(|| hits.first())
            .cloned()
            .ok_or_else(|| {
                anyhow::Error::from(ErrorInfo::ContentNotFound {
                    reference: reference.to_string(),
                })
            })
    }

    /// CurseForge filters by numeric category id, so a named category is
    /// resolved against the class's own category list first.
    async fn category_ids(&self, class: u64, categories: &[String]) -> Result<Vec<u64>> {
        let wanted: Vec<&String> = categories.iter().filter(|c| !c.is_empty()).collect();
        if wanted.is_empty() {
            return Ok(Vec::new());
        }
        let mut ids = Vec::new();
        let mut named = Vec::new();
        for category in wanted {
            match category.parse::<u64>() {
                Ok(id) => ids.push(id),
                Err(_) => named.push(category),
            }
        }
        if named.is_empty() {
            return Ok(ids);
        }
        let body = self
            .get(
                "/categories",
                &[("gameId", GAME.to_string()), ("classId", class.to_string())],
            )
            .await?;
        let known = data(&body).as_array().cloned().unwrap_or_default();
        for category in named {
            let found = known.iter().find(|c| {
                str_field(c, "slug").eq_ignore_ascii_case(category)
                    || str_field(c, "name").eq_ignore_ascii_case(category)
            });
            match found {
                Some(c) => ids.push(u64_field(c, "id")),
                None => bail!("unknown curseforge category: {category}"),
            }
        }
        Ok(ids)
    }

    /// File records for a batch of file ids, in as many requests as the API's
    /// batch limit needs.
    async fn files(&self, ids: &[String]) -> Result<Vec<Value>> {
        let mut out = Vec::new();
        for chunk in ids.chunks(BATCH) {
            let body = self
                .post("/mods/files", json!({ "fileIds": chunk }))
                .await?;
            out.extend(data(&body).as_array().cloned().unwrap_or_default());
        }
        Ok(out)
    }

    /// A manifest names its files by project and file id alone — where each one
    /// goes is a property of the *project's* class, so both are looked up.
    async fn resolve_manifest_files(&self, manifest: &Value) -> Result<Vec<ModpackFile>> {
        let entries = manifest
            .get("files")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if entries.is_empty() {
            return Ok(Vec::new());
        }
        let file_ids: Vec<String> = entries
            .iter()
            .map(|f| u64_field(f, "fileID").to_string())
            .collect();
        let files = self.files(&file_ids).await?;

        let mut mod_ids: Vec<String> = Vec::new();
        for file in &files {
            let id = u64_field(file, "modId").to_string();
            if !mod_ids.contains(&id) {
                mod_ids.push(id);
            }
        }
        let mut dirs: Vec<(u64, &'static str)> = Vec::new();
        for chunk in mod_ids.chunks(BATCH) {
            let body = self.post("/mods", json!({ "modIds": chunk })).await?;
            for project in data(&body).as_array().cloned().unwrap_or_default() {
                dirs.push((
                    u64_field(&project, "id"),
                    class_dir(u64_field(&project, "classId")),
                ));
            }
        }

        let mut out = Vec::new();
        for entry in &entries {
            let file_id = u64_field(entry, "fileID");
            let Some(file) = files.iter().find(|f| u64_field(f, "id") == file_id) else {
                tracing::warn!(file_id, "a modpack file is not published any more");
                return Err(pack::invalid(ArchiveFault::FileUnpublished));
            };
            let mod_id = u64_field(file, "modId");
            let dir = dirs
                .iter()
                .find(|(id, _)| *id == mod_id)
                .map(|(_, dir)| *dir)
                .unwrap_or("mods");
            let filename = str_field(file, "fileName");
            // A pack lists an optional file so a user may add it; the launcher
            // installs what the pack requires and offers the rest as optional.
            let side = match entry.get("required").and_then(Value::as_bool) {
                Some(false) => SideSupport::Optional,
                _ => SideSupport::Required,
            };
            out.push(ModpackFile {
                path: format!("{dir}/{filename}"),
                artifact: artifact_of(file),
                client: side,
                server: side,
            });
        }
        Ok(out)
    }
}

pub(crate) fn is_pack_manifest(manifest: &Value) -> bool {
    str_field(manifest, "manifestType") == PACK_MANIFEST
}

/// Parse a CurseForge `manifest.json` (everything but its file list, which
/// needs the API to resolve). Rejects a manifest that is not a version-1
/// Minecraft modpack.
pub(crate) fn parse_manifest(manifest: &Value) -> Result<ResolvedModpack> {
    let kind = str_field(manifest, "manifestType");
    let version = u64_field(manifest, "manifestVersion");
    if !is_pack_manifest(manifest) {
        tracing::warn!(kind, "unsupported modpack manifest type");
        return Err(pack::invalid(ArchiveFault::ManifestType));
    }
    if version != 1 {
        tracing::warn!(version, "unsupported modpack manifest version");
        return Err(pack::invalid(ArchiveFault::FormatVersion));
    }
    let minecraft = manifest
        .get("minecraft")
        .ok_or_else(|| pack::invalid(ArchiveFault::NoGameVersion))?;
    let game_version = str_field(minecraft, "version");
    if game_version.is_empty() {
        return Err(pack::invalid(ArchiveFault::NoGameVersion));
    }
    // `id` is `<loader>-<version>` ("neoforge-21.1.65"); the primary one is the
    // pack's loader, falling back to the first when none is marked.
    let loaders = minecraft
        .get("modLoaders")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let primary = loaders
        .iter()
        .find(|l| l.get("primary").and_then(Value::as_bool).unwrap_or(false))
        .or_else(|| loaders.first());
    let (loader, loader_version) = primary
        .map(|l| str_field(l, "id"))
        .and_then(|id| {
            id.split_once('-')
                .map(|(name, version)| (name.to_lowercase(), version.to_string()))
        })
        .map(|(name, version)| (Some(name), Some(version)))
        .unwrap_or((None, None));

    Ok(ResolvedModpack {
        source: String::new(),
        project_id: String::new(),
        version_id: String::new(),
        version_number: str_field(manifest, "version"),
        name: str_field(manifest, "name"),
        // A CurseForge manifest carries no description; the project detail is
        // where a pack's summary comes from.
        summary: String::new(),
        game_version,
        loader,
        loader_version,
        files: Vec::new(),
    })
}

fn parse_mod(m: &Value, requested: Option<ContentKind>, body: String) -> ContentProject {
    let kind = kind_of_class(u64_field(m, "classId")).unwrap_or(ContentKind::Mod);
    let kinds = vec![kind];
    let logo = m.get("logo");
    ContentProject {
        source: ID.to_string(),
        id: u64_field(m, "id").to_string(),
        slug: str_field(m, "slug"),
        kind: requested.filter(|k| kinds.contains(k)).unwrap_or(kind),
        kinds,
        title: str_field(m, "name"),
        description: str_field(m, "summary"),
        body,
        author: m
            .get("authors")
            .and_then(Value::as_array)
            .and_then(|a| a.first())
            .map(|a| str_field(a, "name"))
            .unwrap_or_default(),
        categories: m
            .get("categories")
            .and_then(Value::as_array)
            .map(|a| a.iter().map(|c| str_field(c, "name")).collect())
            .unwrap_or_default(),
        downloads: u64_field(m, "downloadCount"),
        follows: u64_field(m, "thumbsUpCount"),
        icon_url: logo
            .map(|l| {
                let thumbnail = str_field(l, "thumbnailUrl");
                match thumbnail.is_empty() {
                    true => str_field(l, "url"),
                    false => thumbnail,
                }
            })
            .unwrap_or_default(),
        gallery: m
            .get("screenshots")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .map(|s| GalleryImage {
                        url: str_field(s, "url"),
                        featured: false,
                        title: str_field(s, "title"),
                        description: str_field(s, "description"),
                    })
                    .collect()
            })
            .unwrap_or_default(),
        // CurseForge publishes no per-project side support; a file may name
        // `Client`/`Server`, which is a version-level fact, not a project one.
        client_side: SideSupport::Unknown,
        server_side: SideSupport::Unknown,
    }
}

fn parse_file(f: &Value) -> ContentVersion {
    let mut game_versions = Vec::new();
    let mut loaders = Vec::new();
    for value in f
        .get("gameVersions")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>())
        .unwrap_or_default()
    {
        match LOADER_NAMES
            .iter()
            .find(|name| name.eq_ignore_ascii_case(value))
        {
            Some(loader) => loaders.push(loader.to_string()),
            None if value.contains('.') => game_versions.push(value.to_string()),
            None => {}
        }
    }
    let display = str_field(f, "displayName");
    ContentVersion {
        source: ID.to_string(),
        id: u64_field(f, "id").to_string(),
        project_id: u64_field(f, "modId").to_string(),
        name: display.clone(),
        version_number: display,
        channel: parse_channel(u64_field(f, "releaseType")),
        game_versions,
        loaders,
        featured: false,
        date_published: str_field(f, "fileDate"),
        downloads: u64_field(f, "downloadCount"),
        files: vec![ContentFile {
            artifact: artifact_of(f),
            primary: true,
        }],
        dependencies: f
            .get("dependencies")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .map(|d| ContentDependency {
                        project_id: u64_field(d, "modId").to_string(),
                        version_id: String::new(),
                        kind: parse_dependency_kind(u64_field(d, "relationType")),
                    })
                    .collect()
            })
            .unwrap_or_default(),
    }
}

/// A file's downloadable artifact. `url` is empty when the author opted out of
/// third-party distribution — the file is listed, but no download is published.
fn artifact_of(f: &Value) -> Artifact {
    Artifact {
        url: str_field(f, "downloadUrl"),
        filename: str_field(f, "fileName"),
        size: u64_field(f, "fileLength"),
        checksum: f
            .get("hashes")
            .and_then(Value::as_array)
            .and_then(|a| a.iter().find(|h| u64_field(h, "algo") == 1))
            .map(|h| str_field(h, "value"))
            .filter(|hex| !hex.is_empty())
            .map(|hex| Checksum {
                algorithm: HashAlgorithm::Sha1,
                hex,
            }),
    }
}

/// CurseForge's index key: MurmurHash2, seed 1, over the bytes minus whitespace.
fn fingerprint(bytes: &[u8]) -> u32 {
    const M: u32 = 0x5bd1_e995;
    let data: Vec<u8> = bytes
        .iter()
        .copied()
        .filter(|b| !matches!(b, 9 | 10 | 13 | 32))
        .collect();
    let mut hash = 1 ^ data.len() as u32;
    let mut words = data.chunks_exact(4);
    for word in &mut words {
        let mut k = u32::from_le_bytes([word[0], word[1], word[2], word[3]]).wrapping_mul(M);
        k ^= k >> 24;
        hash = hash.wrapping_mul(M) ^ k.wrapping_mul(M);
    }
    let tail = words.remainder();
    if !tail.is_empty() {
        for (i, byte) in tail.iter().enumerate() {
            hash ^= u32::from(*byte) << (8 * i);
        }
        hash = hash.wrapping_mul(M);
    }
    hash ^= hash >> 13;
    hash = hash.wrapping_mul(M);
    hash ^ (hash >> 15)
}

fn class_id(kind: ContentKind) -> Option<u64> {
    CLASSES
        .iter()
        .find(|(k, _)| *k == kind)
        .map(|(_, class)| *class)
}

fn kind_of_class(class: u64) -> Option<ContentKind> {
    CLASSES
        .iter()
        .find(|(_, c)| *c == class)
        .map(|(kind, _)| *kind)
}

fn class_dir(class: u64) -> &'static str {
    CLASS_DIRS
        .iter()
        .find(|(c, _)| *c == class)
        .map(|(_, dir)| *dir)
        .unwrap_or("mods")
}

fn loader_type(loader: &str) -> Option<u32> {
    LOADERS
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(loader))
        .map(|(_, id)| *id)
}

/// `ModsSearchSortField`: no field means relevance, and there is no created-at
/// ordering, so newest and updated share `LastUpdated`.
fn sort_field(sort: SearchSort) -> Option<u32> {
    match sort {
        SearchSort::Relevance => None,
        SearchSort::Downloads => Some(6),
        SearchSort::Follows => Some(2),
        SearchSort::Newest | SearchSort::Updated => Some(3),
    }
}

fn parse_channel(release_type: u64) -> ReleaseChannel {
    match release_type {
        2 => ReleaseChannel::Beta,
        3 => ReleaseChannel::Alpha,
        _ => ReleaseChannel::Release,
    }
}

/// `FileRelationType`. Only a required dependency is installed with its
/// dependant, so everything hestia does not model maps to optional.
fn parse_dependency_kind(relation: u64) -> DependencyKind {
    match relation {
        1 => DependencyKind::Embedded,
        3 => DependencyKind::Required,
        5 => DependencyKind::Incompatible,
        _ => DependencyKind::Optional,
    }
}

async fn json(path: &str, response: reqwest::Response) -> Result<Value> {
    let status = response.status();
    if !status.is_success() {
        let detail = match status.as_u16() {
            401 | 403 => format!(
                "{path}: the API key was rejected (HTTP {})",
                status.as_u16()
            ),
            code => format!("{path}: HTTP {code}"),
        };
        return Err(upstream(detail));
    }
    response
        .json()
        .await
        .map_err(|e| upstream(format!("{path} returned malformed JSON: {e}")))
}

fn upstream(detail: String) -> anyhow::Error {
    ErrorInfo::Upstream {
        service: Service::CurseForge,
        detail,
    }
    .into()
}

/// Every CurseForge response wraps its payload in a `data` member.
fn data(body: &Value) -> &Value {
    body.get("data").unwrap_or(&Value::Null)
}

fn is_numeric(value: &str) -> bool {
    !value.is_empty() && value.chars().all(|c| c.is_ascii_digit())
}

fn str_field(v: &Value, key: &str) -> String {
    v.get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn u64_field(v: &Value, key: &str) -> u64 {
    let Some(value) = v.get(key) else {
        return 0;
    };
    value
        .as_u64()
        .or_else(|| value.as_f64().map(|n| n as u64))
        .unwrap_or_default()
}

fn non_empty(opt: &Option<String>) -> Option<&str> {
    opt.as_deref().filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn site_urls_parse_to_project_refs() {
        let cf = CurseForge::default();
        for url in [
            "https://www.curseforge.com/minecraft/mc-mods/jei",
            "https://curseforge.com/minecraft/mc-mods/jei",
            "https://legacy.curseforge.com/minecraft/mc-mods/jei/",
            "https://www.curseforge.com/minecraft/mc-mods/jei?utm=1#files",
        ] {
            let parsed = cf
                .parse_url(url)
                .unwrap_or_else(|| panic!("should parse {url}"));
            assert_eq!(parsed.project, "jei");
            assert_eq!(parsed.version, None, "{url}");
            assert_eq!(parsed.kind, Some(ContentKind::Mod));
        }

        assert_eq!(
            cf.parse_url("https://www.curseforge.com/minecraft/mc-mods/jei/files/5101466")
                .unwrap()
                .version
                .as_deref(),
            Some("5101466")
        );
        assert_eq!(
            cf.parse_url("https://www.curseforge.com/minecraft/modpacks/atm10/download/123")
                .unwrap()
                .version
                .as_deref(),
            Some("123")
        );
        assert_eq!(
            cf.parse_url("https://www.curseforge.com/minecraft/texture-packs/faithful")
                .unwrap()
                .kind,
            Some(ContentKind::ResourcePack)
        );

        assert!(cf.parse_url("https://modrinth.com/mod/sodium").is_none());
        assert!(cf
            .parse_url("https://www.curseforge.com/wow/addons/deadly-boss-mods")
            .is_none());
        assert!(cf
            .parse_url("https://www.curseforge.com/minecraft/mc-mods")
            .is_none());
    }

    #[test]
    fn fingerprints_ignore_whitespace_and_cover_every_tail_length() {
        assert_eq!(fingerprint(b"hello world\n\tfingerprint me"), 1_693_071_057);
        assert_eq!(fingerprint(b"a b\rc"), 1_621_425_345);
        assert_eq!(fingerprint(b"abcd"), 3_376_380_438);
        assert_eq!(fingerprint(b"abcdefg"), 184_182_053);
        assert_eq!(fingerprint(b""), 1_540_447_798);
    }

    #[test]
    fn a_configured_key_overrides_the_build_one() {
        let cf = CurseForge::default();
        cf.configure(&ContentSettings {
            curseforge_key: "  from-config  ".to_string(),
        });
        assert_eq!(cf.key(), "from-config");
        assert!(cf.available());

        cf.configure(&ContentSettings::default());
        assert_eq!(
            cf.key(),
            BUILD_KEY,
            "an unset key falls back to the build's"
        );
        assert_eq!(cf.available(), !BUILD_KEY.is_empty());
    }

    #[test]
    fn files_map_loaders_game_versions_and_hashes() {
        let file = json!({
            "id": 5101466,
            "modId": 238222,
            "displayName": "jei-1.21.1-19.21.0.247.jar",
            "fileName": "jei-1.21.1-19.21.0.247.jar",
            "releaseType": 2,
            "fileDate": "2024-08-15T12:00:00Z",
            "fileLength": 1234,
            "downloadCount": 99,
            "downloadUrl": "https://edge.forgecdn.net/files/5101/466/jei.jar",
            "gameVersions": ["1.21.1", "NeoForge", "Client", "Java 21"],
            "hashes": [{"value": "md5hash", "algo": 2}, {"value": "sha1hash", "algo": 1}],
            "dependencies": [
                {"modId": 111, "relationType": 3},
                {"modId": 222, "relationType": 2}
            ]
        });
        let version = parse_file(&file);
        assert_eq!(version.id, "5101466");
        assert_eq!(version.project_id, "238222");
        assert_eq!(version.channel, ReleaseChannel::Beta);
        assert_eq!(version.game_versions, vec!["1.21.1".to_string()]);
        assert_eq!(version.loaders, vec!["neoforge".to_string()]);
        let artifact = &version.files[0].artifact;
        assert_eq!(artifact.size, 1234);
        assert_eq!(artifact.checksum.as_ref().unwrap().hex, "sha1hash");
        assert_eq!(version.dependencies[0].kind, DependencyKind::Required);
        assert_eq!(version.dependencies[1].kind, DependencyKind::Optional);
    }

    #[test]
    fn a_file_with_no_download_url_carries_an_empty_artifact_url() {
        let file = json!({
            "id": 1,
            "modId": 2,
            "fileName": "blocked.jar",
            "downloadUrl": Value::Null,
            "gameVersions": ["1.21.1"]
        });
        assert!(parse_file(&file).files[0].artifact.url.is_empty());
    }

    #[test]
    fn projects_map_class_ids_to_kinds() {
        let project = json!({
            "id": 238222,
            "slug": "jei",
            "classId": 6,
            "name": "Just Enough Items",
            "summary": "View items and recipes",
            "downloadCount": 400000000.0,
            "thumbsUpCount": 120,
            "authors": [{"name": "mezz", "url": "https://example.invalid"}],
            "categories": [{"name": "Map and Information"}],
            "logo": {"thumbnailUrl": "https://media.forgecdn.net/thumb.png"}
        });
        let parsed = parse_mod(&project, Some(ContentKind::Mod), "body".to_string());
        assert_eq!(parsed.source, "curseforge");
        assert_eq!(parsed.id, "238222");
        assert_eq!(parsed.kind, ContentKind::Mod);
        assert_eq!(parsed.downloads, 400_000_000);
        assert_eq!(parsed.author, "mezz");
        assert_eq!(parsed.categories, vec!["Map and Information".to_string()]);
        assert_eq!(parsed.body, "body");

        let datapack = json!({ "id": 1, "classId": 6945, "slug": "terralith" });
        assert_eq!(
            parse_mod(&datapack, None, String::new()).kind,
            ContentKind::DataPack
        );
        // A requested kind the project does not publish never overrides it.
        assert_eq!(
            parse_mod(&datapack, Some(ContentKind::Shader), String::new()).kind,
            ContentKind::DataPack
        );
    }

    #[test]
    fn download_urls_name_the_file_but_never_the_project() {
        let cf = CurseForge::default();
        let parsed = cf
            .parse_file_url("https://edge.forgecdn.net/files/5101/466/jei-1.21.1.jar")
            .unwrap();
        assert_eq!(parsed.version_id, "5101466");
        assert!(
            parsed.project_id.is_empty(),
            "the URL carries no project — the file lookup answers that"
        );
        assert_eq!(
            cf.parse_file_url("https://mediafilez.forgecdn.net/files/6/12/small.jar")
                .unwrap()
                .version_id,
            "6012",
            "the low segment is the remainder, so it keeps its place value"
        );

        for url in [
            "https://cdn.modrinth.com/data/AAA/versions/BBB/sodium.jar",
            "https://edge.forgecdn.net/files/5101/466",
            "https://edge.forgecdn.net/files/5101/notanumber/x.jar",
        ] {
            assert!(cf.parse_file_url(url).is_none(), "should not parse {url}");
        }
    }

    #[test]
    fn manifests_name_their_loader_and_game_version() {
        let manifest = json!({
            "manifestType": "minecraftModpack",
            "manifestVersion": 1,
            "name": "All the Mods 10",
            "minecraft": {
                "version": "1.21.1",
                "modLoaders": [
                    {"id": "neoforge-21.1.65", "primary": true},
                    {"id": "forge-1.0.0", "primary": false}
                ]
            }
        });
        let resolved = parse_manifest(&manifest).unwrap();
        assert_eq!(resolved.name, "All the Mods 10");
        assert_eq!(resolved.game_version, "1.21.1");
        assert_eq!(resolved.loader.as_deref(), Some("neoforge"));
        assert_eq!(resolved.loader_version.as_deref(), Some("21.1.65"));
    }

    #[test]
    fn manifests_of_another_shape_are_refused() {
        for manifest in [
            json!({ "manifestType": "minecraftModpack", "manifestVersion": 2,
                    "minecraft": {"version": "1.21.1"} }),
            json!({ "manifestType": "somethingElse", "manifestVersion": 1,
                    "minecraft": {"version": "1.21.1"} }),
            json!({ "manifestType": "minecraftModpack", "manifestVersion": 1,
                    "minecraft": {"modLoaders": []} }),
        ] {
            assert!(parse_manifest(&manifest).is_err());
        }
    }

    #[test]
    fn manifest_files_land_in_their_projects_class_dir() {
        assert_eq!(class_dir(6), "mods");
        assert_eq!(class_dir(12), "resourcepacks");
        assert_eq!(class_dir(6552), "shaderpacks");
        assert_eq!(class_dir(17), "saves");
        assert_eq!(class_dir(999), "mods", "an unknown class is loader content");
    }

    #[test]
    fn sort_and_loader_vocabularies_map() {
        assert_eq!(sort_field(SearchSort::Relevance), None);
        assert_eq!(sort_field(SearchSort::Downloads), Some(6));
        assert_eq!(sort_field(SearchSort::Newest), Some(3));

        assert_eq!(loader_type("fabric"), Some(4));
        assert_eq!(loader_type("NeoForge"), Some(6));
        assert_eq!(
            loader_type("datapack"),
            None,
            "a class-typed kind is not a curseforge loader"
        );
        assert_eq!(loader_type("paper"), None);
    }
}
