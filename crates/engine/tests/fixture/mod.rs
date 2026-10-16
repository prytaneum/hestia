//! Stand-in flavors and content sources. The provider traits are the engine's
//! real seams; these are the second adapter behind them, so a flow can be
//! driven without reaching Mojang, Modrinth or a JDK mirror.

use std::collections::HashMap;

use anyhow::{bail, Result};
use async_trait::async_trait;
use engine::{
    AssetIndex, Content, ContentProvider, FileRef, FlavorInfo, FlavorProvider, InstanceProfile,
    LocalFile, Minecraft, ServerJar, ServerProfile, Side, UrlRef,
};
use proto::content::{
    ContentKind, ContentProject, ContentVersion, ResolvedModpack, SearchQuery, SearchResult,
    VersionQuery,
};

use proto::minecraft::{Artifact, GameVersion};

/// A flavor that resolves to a profile naming `artifact`, or fails when it is
/// `None` — the two halves of what a provider can do to a create.
pub struct Flavor {
    pub info: FlavorInfo,
    pub versions: Vec<&'static str>,
    pub artifact: Option<Artifact>,
}

fn info(id: &'static str) -> FlavorInfo {
    FlavorInfo {
        id,
        name: "Fixture",
        summary: "a flavor that exists only in tests",
        loads: Some(ContentKind::Mod),
        requires: &[],
        sides: &[Side::Server, Side::Client],
    }
}

impl Flavor {
    pub fn resolving(id: &'static str) -> Flavor {
        Flavor {
            info: info(id),
            versions: vec!["1.21", "1.20.1"],
            artifact: Some(Artifact {
                url: "http://127.0.0.1:1/server.jar".into(),
                filename: "server.jar".into(),
                size: 0,
                checksum: None,
            }),
        }
    }

    /// A flavor whose artifact is actually downloadable, for a flow that
    /// materializes it.
    pub fn serving(id: &'static str, files: &Files) -> Flavor {
        Flavor {
            info: info(id),
            versions: vec!["1.21", "1.20.1"],
            artifact: Some(Artifact {
                url: files.url("client.jar"),
                filename: "client.jar".into(),
                size: Files::BODY.len() as u64,
                checksum: None,
            }),
        }
    }

    pub fn failing(id: &'static str) -> Flavor {
        Flavor {
            info: info(id),
            versions: vec!["1.21"],
            artifact: None,
        }
    }

    fn catalogue(&self) -> Vec<GameVersion> {
        self.versions
            .iter()
            .map(|v| GameVersion {
                id: (*v).to_string(),
                ..GameVersion::default()
            })
            .collect()
    }

    fn primary(&self) -> Result<Artifact> {
        match &self.artifact {
            Some(artifact) => Ok(artifact.clone()),
            None => bail!("this flavor cannot resolve"),
        }
    }
}

#[async_trait]
impl FlavorProvider for Flavor {
    fn info(&self) -> &FlavorInfo {
        &self.info
    }
    async fn versions(&self) -> Result<Vec<GameVersion>> {
        Ok(self.catalogue())
    }
    async fn resolve_server(&self, request: &engine::ResolveRequest<'_>) -> Result<ServerProfile> {
        Ok(ServerProfile {
            flavor: self.info.id.to_string(),
            game_version: request.version.clone(),
            jar: ServerJar::Download(self.primary()?),
            java_major: 21,
            ..ServerProfile::default()
        })
    }
    async fn resolve_client(
        &self,
        request: &engine::ResolveRequest<'_>,
    ) -> Result<InstanceProfile> {
        let client = self.primary()?;
        Ok(InstanceProfile {
            flavor: self.info.id.to_string(),
            game_version: request.version.clone(),
            asset_index: AssetIndex {
                id: request.version.clone(),
                artifact: Artifact {
                    url: client.url.replace("client.jar", "assets.json"),
                    filename: format!("{}.json", request.version),
                    ..Artifact::default()
                },
                total_size: 0,
            },
            client,
            java_major: 21,
            main_class: "net.minecraft.client.main.Main".into(),
            ..InstanceProfile::default()
        })
    }
}

/// A content source that serves whatever it was built with.
#[derive(Default)]
pub struct Source {
    pub projects: Vec<ContentProject>,
    pub versions: Vec<ContentVersion>,
}

#[async_trait]
impl ContentProvider for Source {
    fn id(&self) -> &'static str {
        "fixture"
    }
    fn name(&self) -> &'static str {
        "Fixture"
    }
    fn kinds(&self) -> Vec<ContentKind> {
        vec![ContentKind::Mod, ContentKind::DataPack]
    }
    async fn search(&self, _query: &SearchQuery) -> Result<SearchResult> {
        Ok(SearchResult {
            hits: self.projects.clone(),
            total: self.projects.len() as u32,
            ..SearchResult::default()
        })
    }
    async fn project(&self, reference: &str, _kind: Option<ContentKind>) -> Result<ContentProject> {
        self.projects
            .iter()
            .find(|p| p.id == reference || p.slug == reference)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("no such project"))
    }
    async fn versions(&self, query: &VersionQuery) -> Result<Vec<ContentVersion>> {
        Ok(self
            .versions
            .iter()
            .filter(|v| v.project_id == query.project)
            .cloned()
            .collect())
    }
    async fn identify(&self, files: &[LocalFile]) -> Result<HashMap<String, ContentVersion>> {
        Ok(files
            .iter()
            .filter_map(|file| {
                let name = file.path.file_name()?.to_str()?;
                let version = self
                    .versions
                    .iter()
                    .find(|v| v.files.iter().any(|f| f.artifact.filename == name))?;
                Some((file.sha1.clone(), version.clone()))
            })
            .collect())
    }
    fn parse_url(&self, _url: &str) -> Option<UrlRef> {
        None
    }
    fn parse_file_url(&self, url: &str) -> Option<FileRef> {
        let version = self
            .versions
            .iter()
            .find(|v| v.files.iter().any(|f| f.artifact.url == url))?;
        Some(FileRef {
            project_id: version.project_id.clone(),
            version_id: version.id.clone(),
        })
    }
    async fn versions_by_id(&self, ids: &[String]) -> Result<Vec<ContentVersion>> {
        Ok(self
            .versions
            .iter()
            .filter(|v| ids.contains(&v.id))
            .cloned()
            .collect())
    }
    async fn resolve_modpack(&self, _version: &str) -> Result<ResolvedModpack> {
        bail!("the fixture source serves no packs")
    }
    async fn fetch_modpack(&self, _version_id: &str) -> Result<(ResolvedModpack, Vec<u8>)> {
        bail!("the fixture source serves no packs")
    }
}

/// An engine over `flavors` for both sides and `sources` for content.
pub fn engine(
    home: &std::path::Path,
    flavors: Vec<Flavor>,
    sources: Vec<Source>,
) -> engine::Engine {
    let flavors: Vec<Box<dyn FlavorProvider>> = flavors
        .into_iter()
        .map(|f| Box::new(f) as Box<dyn FlavorProvider>)
        .collect();
    let providers: Vec<Box<dyn ContentProvider>> = sources
        .into_iter()
        .map(|s| Box::new(s) as Box<dyn ContentProvider>)
        .collect();
    engine::Engine::over(
        Some(home),
        Minecraft::with_providers(flavors),
        Content::with_providers(providers),
    )
}

/// A project the fixture source catalogues.
pub fn project(id: &str, kind: ContentKind) -> ContentProject {
    ContentProject {
        id: id.to_string(),
        slug: id.to_string(),
        title: id.to_string(),
        source: "fixture".to_string(),
        kind,
        kinds: vec![kind],
        client_side: proto::content::SideSupport::Required,
        server_side: proto::content::SideSupport::Required,
        ..ContentProject::default()
    }
}

/// A downloadable version of `project`, served from `files`, optionally
/// requiring `deps`.
pub fn version(project: &str, files: &Files, game: &str, deps: &[&str]) -> ContentVersion {
    ContentVersion {
        source: "fixture".to_string(),
        id: format!("{project}-1"),
        project_id: project.to_string(),
        name: project.to_string(),
        version_number: "1.0.0".to_string(),
        game_versions: vec![game.to_string()],
        loaders: vec!["fabric".to_string()],
        files: vec![proto::content::ContentFile {
            artifact: proto::minecraft::Artifact {
                url: files.url(&format!("{project}.jar")),
                filename: format!("{project}.jar"),
                size: Files::BODY.len() as u64,
                checksum: None,
            },
            primary: true,
        }],
        dependencies: deps
            .iter()
            .map(|d| proto::content::ContentDependency {
                project_id: (*d).to_string(),
                kind: proto::content::DependencyKind::Required,
                ..proto::content::ContentDependency::default()
            })
            .collect(),
        ..ContentVersion::default()
    }
}

/// The smallest HTTP server that can hand back a jar, so an install writes real
/// bytes without reaching a platform.
pub struct Files {
    port: u16,
}

impl Files {
    pub const BODY: &'static [u8] = b"PK\x03\x04 not really a jar";
    /// An index naming no objects: the assets pass has nothing to fetch, but
    /// still has to find and parse one.
    pub const ASSET_INDEX: &'static [u8] = br#"{"objects":{}}"#;

    pub async fn serve() -> Files {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let port = listener.local_addr().expect("addr").port();
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                tokio::spawn(async move {
                    use tokio::io::{AsyncReadExt, AsyncWriteExt};
                    let mut scratch = [0u8; 1024];
                    let read = socket.read(&mut scratch).await.unwrap_or(0);
                    let request = String::from_utf8_lossy(&scratch[..read]).to_string();
                    // An asset index has to parse; everything else is opaque
                    // bytes as far as a materialize is concerned.
                    let body: &[u8] = match request.contains(".json") {
                        true => Files::ASSET_INDEX,
                        false => Files::BODY,
                    };
                    let head = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = socket.write_all(head.as_bytes()).await;
                    let _ = socket.write_all(body).await;
                    let _ = socket.flush().await;
                });
            }
        });
        Files { port }
    }

    pub fn url(&self, name: &str) -> String {
        format!("http://127.0.0.1:{}/{name}", self.port)
    }
}

/// Register a Java runtime on disk so `ensure_java` finds it instead of
/// resolving one upstream — the store scans its directory, so the disk is the
/// only seam a test needs.
pub fn java_runtime(home: &std::path::Path, major: i32) {
    let install = home.join("java").join(format!("fixture-{major}"));
    let bin = install.join("bin");
    std::fs::create_dir_all(&bin).expect("java dir");
    let exe = if cfg!(windows) { "java.exe" } else { "java" };
    std::fs::write(bin.join(exe), b"#!/bin/sh\nexit 0\n").expect("java binary");
    std::fs::write(
        install.join("runtime.json"),
        serde_json::json!({
            "vendor": "fixture",
            "major": major,
            "release_name": format!("jdk-{major}"),
            "executable": format!("bin/{exe}"),
        })
        .to_string(),
    )
    .expect("runtime record");
}

/// Sign an account in on disk, with a token far enough from expiry that
/// `access_token` hands it back rather than refreshing it upstream.
pub fn account(home: &std::path::Path, name: &str) -> String {
    let uuid = "00000000000000000000000000000001".to_string();
    let far_future = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_secs() as i64
        + 86_400;
    std::fs::create_dir_all(home).expect("home");
    std::fs::write(
        home.join("accounts.json"),
        serde_json::json!({
            "accounts": [{
                "uuid": uuid,
                "name": name,
                "refreshToken": "fixture-refresh",
                "accessToken": "fixture-access",
                "expiresAt": far_future,
                "needsReauth": false,
            }],
            "defaultUuid": uuid,
        })
        .to_string(),
    )
    .expect("accounts.json");
    uuid
}
