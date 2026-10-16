//! Mojang piston-meta: the version manifest and per-version detail JSON, plus
//! the extractors that turn a version's JSON into launch-profile pieces.

use anyhow::{Context, Result};
use proto::download::{Checksum, HashAlgorithm};
use proto::minecraft::{Artifact, GameVersion, VersionKind};
use serde_json::Value;

use proto::error::Service;

use super::{fetch_json, host_os, rules_allow};
use crate::minecraft::profile::{AssetIndex, Library, Native};

const MANIFEST: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";

struct Entry {
    id: String,
    kind: VersionKind,
    url: String,
}

async fn manifest() -> Result<Vec<Entry>> {
    let j = fetch_json(Service::Mojang, MANIFEST).await?;
    let versions = j
        .get("versions")
        .and_then(Value::as_array)
        .context("version manifest is missing its versions array")?;
    let mut out = Vec::with_capacity(versions.len());
    for v in versions {
        let id = str_field(v, "id");
        let url = str_field(v, "url");
        if id.is_empty() || url.is_empty() {
            continue;
        }
        let kind = match v.get("type").and_then(Value::as_str).unwrap_or_default() {
            "release" => VersionKind::Release,
            "old_beta" => VersionKind::OldBeta,
            "old_alpha" => VersionKind::OldAlpha,
            _ => VersionKind::Snapshot,
        };
        out.push(Entry { id, kind, url });
    }
    Ok(out)
}

pub async fn versions() -> Result<Vec<GameVersion>> {
    Ok(manifest()
        .await?
        .into_iter()
        .map(|e| GameVersion {
            stable: e.kind == VersionKind::Release,
            id: e.id,
            kind: e.kind,
        })
        .collect())
}

/// Fetch a version's detail JSON, resolving its id through the manifest.
pub async fn version_json(id: &str) -> Result<Value> {
    let entry = manifest()
        .await?
        .into_iter()
        .find(|e| e.id == id)
        .ok_or_else(|| proto::error::ErrorInfo::VersionNotFound {
            reference: id.to_string(),
        })?;
    fetch_json(Service::Mojang, &entry.url).await
}

pub fn client_artifact(version: &Value) -> Result<Artifact> {
    download_artifact(version, "client").context("version JSON has no client download")
}

pub fn server_artifact(version: &Value) -> Result<Artifact> {
    download_artifact(version, "server")
        .context("version JSON has no server download (too old to ship a server jar)")
}

pub fn java_major(version: &Value) -> i32 {
    version
        .get("javaVersion")
        .and_then(|j| j.get("majorVersion"))
        .and_then(Value::as_i64)
        .unwrap_or(8) as i32
}

pub fn main_class(version: &Value) -> String {
    str_field(version, "mainClass")
}

pub fn asset_index(version: &Value) -> Result<AssetIndex> {
    let index = version
        .get("assetIndex")
        .context("version JSON has no assetIndex")?;
    let artifact = artifact_from(index).context("assetIndex is missing its url")?;
    Ok(AssetIndex {
        id: str_field(index, "id"),
        total_size: index.get("totalSize").and_then(Value::as_u64).unwrap_or(0),
        artifact,
    })
}

/// Rule-filtered classpath libraries. A legacy library's native classifier is
/// *not* one of these — it is unpacked rather than put on the classpath, and
/// `natives()` collects it instead.
pub fn libraries(version: &Value) -> Vec<Library> {
    let mut out = Vec::new();
    let Some(libs) = version.get("libraries").and_then(Value::as_array) else {
        return out;
    };
    for lib in libs {
        if let Some(rules) = lib.get("rules") {
            if !rules_allow(rules) {
                continue;
            }
        }
        let Some(artifact_obj) = lib.get("downloads").and_then(|d| d.get("artifact")) else {
            continue;
        };
        let Some(artifact) = artifact_from(artifact_obj) else {
            continue;
        };
        out.push(Library {
            name: str_field(lib, "name"),
            path: str_field(artifact_obj, "path"),
            artifact,
        });
    }
    out
}

/// Rule-filtered native libraries, resolved to the classifier this host needs.
/// Empty for 1.19 and later, whose manifests carry no `natives` block.
pub fn natives(version: &Value) -> Vec<Native> {
    let mut out = Vec::new();
    let Some(libs) = version.get("libraries").and_then(Value::as_array) else {
        return out;
    };
    for lib in libs {
        if lib.get("rules").is_some_and(|rules| !rules_allow(rules)) {
            continue;
        }
        let Some(classifier) = native_classifier(lib) else {
            continue;
        };
        let Some(download) = lib
            .get("downloads")
            .and_then(|d| d.get("classifiers"))
            .and_then(|c| c.get(&classifier))
        else {
            continue;
        };
        let Some(artifact) = artifact_from(download) else {
            continue;
        };
        out.push(Native {
            library: Library {
                name: format!("{}:{classifier}", str_field(lib, "name")),
                path: str_field(download, "path"),
                artifact,
            },
            exclude: lib
                .get("extract")
                .and_then(|e| e.get("exclude"))
                .and_then(Value::as_array)
                .map(|v| {
                    v.iter()
                        .filter_map(Value::as_str)
                        .map(String::from)
                        .collect()
                })
                .unwrap_or_default(),
        });
    }
    out
}

/// The `natives` key for this host. `${arch}` is the only placeholder Mojang
/// spells in a classifier name, and it means the JVM's pointer width.
fn native_classifier(lib: &Value) -> Option<String> {
    let classifier = lib.get("natives")?.get(host_os())?.as_str()?;
    let arch = if cfg!(target_pointer_width = "64") {
        "64"
    } else {
        "32"
    };
    Some(classifier.replace("${arch}", arch))
}

pub fn jvm_args(version: &Value) -> Vec<String> {
    collect_args(version, "jvm")
}

pub fn game_args(version: &Value) -> Vec<String> {
    let modern = collect_args(version, "game");
    if !modern.is_empty() {
        return modern;
    }
    version
        .get("minecraftArguments")
        .and_then(Value::as_str)
        .map(|s| s.split_whitespace().map(String::from).collect())
        .unwrap_or_default()
}

fn collect_args(version: &Value, key: &str) -> Vec<String> {
    let Some(arr) = version
        .get("arguments")
        .and_then(|a| a.get(key))
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in arr {
        match entry {
            Value::String(s) => out.push(s.clone()),
            Value::Object(_) => {
                if entry.get("rules").is_some_and(|r| !rules_allow(r)) {
                    continue;
                }
                match entry.get("value") {
                    Some(Value::String(s)) => out.push(s.clone()),
                    Some(Value::Array(vals)) => {
                        out.extend(vals.iter().filter_map(Value::as_str).map(String::from))
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    out
}

fn download_artifact(version: &Value, key: &str) -> Option<Artifact> {
    version
        .get("downloads")
        .and_then(|d| d.get(key))
        .and_then(artifact_from)
}

fn artifact_from(obj: &Value) -> Option<Artifact> {
    let url = obj.get("url").and_then(Value::as_str)?.to_string();
    if url.is_empty() {
        return None;
    }
    let sha1 = str_field(obj, "sha1");
    Some(Artifact {
        filename: url.rsplit('/').next().unwrap_or_default().to_string(),
        url,
        size: obj.get("size").and_then(Value::as_u64).unwrap_or(0),
        checksum: (!sha1.is_empty()).then_some(Checksum {
            algorithm: HashAlgorithm::Sha1,
            hex: sha1,
        }),
    })
}

fn str_field(obj: &Value, key: &str) -> String {
    obj.get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn legacy_version() -> Value {
        serde_json::json!({
            "libraries": [{
                "name": "org.lwjgl.lwjgl:lwjgl-platform:2.9.4",
                "natives": {
                    "linux": "natives-linux",
                    "windows": "natives-windows",
                    "osx": "natives-osx"
                },
                "extract": { "exclude": ["META-INF/"] },
                "downloads": {
                    "artifact": {
                        "path": "org/lwjgl/lwjgl/lwjgl-platform/2.9.4/lwjgl-platform-2.9.4.jar",
                        "url": "https://libraries.minecraft.net/base.jar",
                        "sha1": "aa",
                        "size": 10
                    },
                    "classifiers": {
                        "natives-linux": {
                            "path": "org/lwjgl/natives-linux.jar",
                            "url": "https://libraries.minecraft.net/natives-linux.jar",
                            "sha1": "bb",
                            "size": 20
                        },
                        "natives-windows": {
                            "path": "org/lwjgl/natives-windows.jar",
                            "url": "https://libraries.minecraft.net/natives-windows.jar",
                            "sha1": "cc",
                            "size": 30
                        },
                        "natives-osx": {
                            "path": "org/lwjgl/natives-osx.jar",
                            "url": "https://libraries.minecraft.net/natives-osx.jar",
                            "sha1": "dd",
                            "size": 40
                        }
                    }
                }
            }]
        })
    }

    #[test]
    fn natives_take_the_host_classifier_and_its_exclusions() {
        let natives = natives(&legacy_version());
        assert_eq!(natives.len(), 1);
        assert_eq!(
            natives[0].library.path,
            format!("org/lwjgl/natives-{}.jar", host_os())
        );
        assert_eq!(natives[0].exclude, ["META-INF/"]);
    }

    #[test]
    fn a_native_classifier_stays_off_the_classpath() {
        let libraries = libraries(&legacy_version());
        assert_eq!(libraries.len(), 1, "only the base artifact is a library");
        assert!(libraries[0].path.ends_with("lwjgl-platform-2.9.4.jar"));
    }

    #[test]
    fn a_modern_version_has_no_natives() {
        let version = serde_json::json!({
            "libraries": [{
                "name": "org.lwjgl:lwjgl:3.3.3:natives-linux",
                "downloads": { "artifact": {
                    "path": "org/lwjgl/lwjgl/3.3.3/lwjgl-3.3.3-natives-linux.jar",
                    "url": "https://libraries.minecraft.net/x.jar",
                    "sha1": "ee",
                    "size": 50
                }}
            }]
        });
        assert!(natives(&version).is_empty());
        assert_eq!(libraries(&version).len(), 1);
    }

    #[test]
    fn the_arch_placeholder_resolves_to_the_pointer_width() {
        let version = serde_json::json!({
            "libraries": [{
                "name": "tv.twitch:twitch-platform:5.16",
                "natives": { "linux": "natives-linux-${arch}",
                             "windows": "natives-windows-${arch}",
                             "osx": "natives-osx" },
                "downloads": { "classifiers": {
                    format!("natives-{}-64", host_os()): {
                        "path": "tv/twitch/native.jar",
                        "url": "https://libraries.minecraft.net/native.jar",
                        "sha1": "ff",
                        "size": 60
                    }
                }}
            }]
        });
        let expected = if cfg!(target_pointer_width = "64") {
            1
        } else {
            0
        };
        assert_eq!(natives(&version).len(), expected);
    }
}
