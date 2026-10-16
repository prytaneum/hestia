//! Fabric — the loader profile layered over a base game version. The server is a
//! self-contained launcher jar; the client merges Fabric's libraries and main
//! class over the vanilla profile.

use std::collections::HashMap;

use anyhow::Result;
use async_trait::async_trait;
use proto::content::ContentKind;
use proto::minecraft::{Artifact, GameVersion, VersionKind};

use super::meta::{fabric, mojang};
use super::profile::{InstanceProfile, Library, ServerProfile};
use super::provider::{InstanceProvider, Loads, ResolveRequest, ServerProvider};

const ID: &str = "fabric";
const NAME: &str = "Fabric";
const SUMMARY: &str =
    "A light, fast-moving mod loader. Usually the first to support a new Minecraft version.";

async fn game_versions() -> Result<Vec<GameVersion>> {
    Ok(fabric::game_versions()
        .await?
        .into_iter()
        .map(|(id, stable)| GameVersion {
            id,
            kind: if stable {
                VersionKind::Release
            } else {
                VersionKind::Snapshot
            },
            stable,
        })
        .collect())
}

async fn resolve_loader(request: &ResolveRequest) -> Result<String> {
    match &request.loader_version {
        Some(loader) => Ok(loader.clone()),
        None => fabric::latest_loader(&request.version).await,
    }
}

pub struct FabricServer;

#[async_trait]
impl ServerProvider for FabricServer {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }
    fn summary(&self) -> &'static str {
        SUMMARY
    }

    fn loads(&self) -> Loads {
        Some(ContentKind::Mod)
    }

    async fn versions(&self) -> Result<Vec<GameVersion>> {
        game_versions().await
    }

    async fn loader_versions(&self, game: &str) -> Result<Vec<String>> {
        fabric::loader_versions(game).await
    }

    async fn resolve(&self, request: &ResolveRequest) -> Result<ServerProfile> {
        let loader = resolve_loader(request).await?;
        let installer = fabric::latest_installer().await?;
        let base = mojang::version_json(&request.version).await?;
        let url = fabric::server_launcher_url(&request.version, &loader, &installer);
        Ok(ServerProfile {
            flavor: ID.to_string(),
            game_version: request.version.clone(),
            loader_version: Some(loader.clone()),
            primary: Artifact {
                filename: format!(
                    "fabric-server-mc.{}-loader.{}-launcher.{}.jar",
                    request.version, loader, installer
                ),
                url,
                size: 0,
                checksum: None,
            },
            libraries: Vec::new(),
            java_major: mojang::java_major(&base),
            main_class: String::new(),
            jvm_args: Vec::new(),
            args_file: String::new(),
        })
    }
}

/// Layer a modloader's libraries over the vanilla base the way an inheriting
/// version manifest merges: an artifact the loader re-pins (commonly
/// `org.ow2.asm:asm`, which vanilla and the loader carry at different versions)
/// resolves to a single entry — two versions of one artifact on the classpath
/// make Fabric's loader abort ("duplicate ASM classes found on classpath").
/// Keyed by the coordinate minus its version, so a jar and its `natives-*`
/// sibling (same artifact, different classifier) both survive; the overlay wins
/// a collision and first-seen order is preserved.
pub(super) fn merge_libraries(base: Vec<Library>, overlay: Vec<Library>) -> Vec<Library> {
    let mut order: Vec<String> = Vec::new();
    let mut by_key: HashMap<String, Library> = HashMap::new();
    for library in base.into_iter().chain(overlay) {
        let key = versionless_key(&library.name);
        if by_key.insert(key.clone(), library).is_none() {
            order.push(key);
        }
    }
    order
        .into_iter()
        .filter_map(|key| by_key.remove(&key))
        .collect()
}

/// A Maven coordinate (`group:artifact:version[:classifier][@ext]`) reduced to
/// its identity ignoring version. Falls back to the raw coordinate when it does
/// not parse, so an unrecognised entry is never merged into an unrelated one.
fn versionless_key(coord: &str) -> String {
    let (coord, ext) = coord.split_once('@').unwrap_or((coord, "jar"));
    let parts: Vec<&str> = coord.split(':').collect();
    if parts.len() < 3 {
        return coord.to_string();
    }
    match parts.get(3) {
        Some(classifier) => format!("{}:{}:{classifier}@{ext}", parts[0], parts[1]),
        None => format!("{}:{}@{ext}", parts[0], parts[1]),
    }
}

pub struct FabricInstance;

#[async_trait]
impl InstanceProvider for FabricInstance {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }
    fn summary(&self) -> &'static str {
        SUMMARY
    }

    fn loads(&self) -> Loads {
        Some(ContentKind::Mod)
    }

    async fn versions(&self) -> Result<Vec<GameVersion>> {
        game_versions().await
    }

    async fn loader_versions(&self, game: &str) -> Result<Vec<String>> {
        fabric::loader_versions(game).await
    }

    async fn resolve(&self, request: &ResolveRequest) -> Result<InstanceProfile> {
        let loader = resolve_loader(request).await?;
        let base = mojang::version_json(&request.version).await?;
        let profile = fabric::profile_json(&request.version, &loader).await?;

        let libraries = merge_libraries(mojang::libraries(&base), fabric::libraries(&profile));

        let mut jvm_args = mojang::jvm_args(&base);
        jvm_args.extend(fabric::jvm_args(&profile));

        Ok(InstanceProfile {
            flavor: ID.to_string(),
            game_version: request.version.clone(),
            loader_version: Some(loader),
            client: mojang::client_artifact(&base)?,
            libraries,
            natives: mojang::natives(&base),
            asset_index: mojang::asset_index(&base)?,
            java_major: mojang::java_major(&base),
            main_class: fabric::client_main_class(&profile),
            jvm_args,
            game_args: mojang::game_args(&base),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lib(name: &str) -> Library {
        Library {
            name: name.to_string(),
            path: name.to_string(),
            artifact: Artifact::default(),
        }
    }

    fn names(libs: &[Library]) -> Vec<&str> {
        libs.iter().map(|l| l.name.as_str()).collect()
    }

    #[test]
    fn overlay_repin_collapses_to_one_version() {
        let merged = merge_libraries(
            vec![
                lib("org.ow2.asm:asm:9.6"),
                lib("com.google.guava:guava:32.1"),
            ],
            vec![lib("org.ow2.asm:asm:9.10.1")],
        );
        assert_eq!(
            names(&merged),
            ["org.ow2.asm:asm:9.10.1", "com.google.guava:guava:32.1"],
            "the overlay version wins in the base's slot"
        );
    }

    #[test]
    fn distinct_classifiers_are_kept() {
        let merged = merge_libraries(
            vec![
                lib("org.lwjgl:lwjgl:3.3.3"),
                lib("org.lwjgl:lwjgl:3.3.3:natives-linux"),
            ],
            Vec::new(),
        );
        assert_eq!(
            names(&merged).len(),
            2,
            "jar and its natives sibling survive"
        );
    }
}
