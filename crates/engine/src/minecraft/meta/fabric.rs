//! Fabric Meta API: supported game versions, loader versions, and the loader
//! profile JSON (a vanilla-style version JSON carrying Fabric's libraries and
//! main class, meant to be merged over the base game version).

use anyhow::{Context, Result};
use proto::download::{Checksum, HashAlgorithm};
use proto::minecraft::Artifact;
use serde_json::Value;

use proto::error::Service;

use super::{fetch_json, filename_of, maven_path};
use crate::minecraft::profile::Library;

const META: &str = "https://meta.fabricmc.net/v2";
const DEFAULT_MAVEN: &str = "https://maven.fabricmc.net/";

/// The game versions Fabric supports, paired with their stability flag.
pub async fn game_versions() -> Result<Vec<(String, bool)>> {
    let j = fetch_json(Service::Fabric, &format!("{META}/versions/game")).await?;
    let arr = j
        .as_array()
        .context("fabric game-versions response is not an array")?;
    Ok(arr
        .iter()
        .filter_map(|v| {
            let version = v.get("version").and_then(Value::as_str)?.to_string();
            let stable = v.get("stable").and_then(Value::as_bool).unwrap_or(false);
            Some((version, stable))
        })
        .collect())
}

async fn loader_entries(game: &str) -> Result<Vec<Value>> {
    match fetch_json(Service::Fabric, &format!("{META}/versions/loader/{game}")).await? {
        Value::Array(entries) => Ok(entries),
        _ => anyhow::bail!("fabric loader response is not an array"),
    }
}

fn newest_stable(entries: &[Value], is_stable: impl Fn(&Value) -> bool) -> Option<&Value> {
    entries
        .iter()
        .find(|e| is_stable(e))
        .or_else(|| entries.first())
}

/// Every loader build published for a game version, newest first.
pub async fn loader_versions(game: &str) -> Result<Vec<String>> {
    Ok(loader_entries(game)
        .await?
        .iter()
        .filter_map(|e| e.get("loader")?.get("version")?.as_str().map(String::from))
        .collect())
}

/// The newest loader for a game version: the first stable build, else the newest.
pub async fn latest_loader(game: &str) -> Result<String> {
    let entries = loader_entries(game).await?;
    newest_stable(&entries, |e| {
        e.pointer("/loader/stable")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    })
    .and_then(|e| e.pointer("/loader/version"))
    .and_then(Value::as_str)
    .map(String::from)
    .with_context(|| format!("no fabric loader is published for Minecraft {game}"))
}

/// The newest installer: the first stable build, else the newest. The server
/// launcher endpoint is keyed by it.
pub async fn latest_installer() -> Result<String> {
    let j = fetch_json(Service::Fabric, &format!("{META}/versions/installer")).await?;
    let arr = j
        .as_array()
        .context("fabric installer response is not an array")?;
    newest_stable(arr, |e| {
        e.get("stable").and_then(Value::as_bool).unwrap_or(false)
    })
    .and_then(|e| e.get("version"))
    .and_then(Value::as_str)
    .map(String::from)
    .context("no fabric installer is published")
}

pub async fn profile_json(game: &str, loader: &str) -> Result<Value> {
    fetch_json(
        Service::Fabric,
        &format!("{META}/versions/loader/{game}/{loader}/profile/json"),
    )
    .await
}

/// The self-contained Fabric server launcher jar for a game/loader/installer
/// triple (the endpoint 404s without the installer segment).
pub fn server_launcher_url(game: &str, loader: &str, installer: &str) -> String {
    format!("{META}/versions/loader/{game}/{loader}/{installer}/server/jar")
}

/// `mainClass` in a loader profile is a bare string for the client and an object
/// (`{client, server}`) in some builds — this reads the client entry.
pub fn client_main_class(profile: &Value) -> String {
    match profile.get("mainClass") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Object(_)) => profile["mainClass"]
            .get("client")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        _ => String::new(),
    }
}

/// Fabric's libraries carry a Maven coordinate and a repository base URL (no
/// path); the download URL and path are derived from the coordinate.
pub fn libraries(profile: &Value) -> Vec<Library> {
    let mut out = Vec::new();
    let Some(libs) = profile.get("libraries").and_then(Value::as_array) else {
        return out;
    };
    for lib in libs {
        let name = lib.get("name").and_then(Value::as_str).unwrap_or_default();
        let Some(path) = maven_path(name) else {
            continue;
        };
        let base = lib
            .get("url")
            .and_then(Value::as_str)
            .unwrap_or(DEFAULT_MAVEN);
        let sep = if base.ends_with('/') { "" } else { "/" };
        let sha1 = lib
            .get("sha1")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        out.push(Library {
            name: name.to_string(),
            artifact: Artifact {
                url: format!("{base}{sep}{path}"),
                filename: filename_of(&path),
                size: lib.get("size").and_then(Value::as_u64).unwrap_or(0),
                checksum: (!sha1.is_empty()).then_some(Checksum {
                    algorithm: HashAlgorithm::Sha1,
                    hex: sha1,
                }),
            },
            path,
        });
    }
    out
}

pub fn jvm_args(profile: &Value) -> Vec<String> {
    profile
        .get("arguments")
        .and_then(|a| a.get("jvm"))
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(String::from)
                .collect()
        })
        .unwrap_or_default()
}
