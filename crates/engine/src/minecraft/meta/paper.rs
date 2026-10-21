//! The PaperMC Fill v3 API: the game versions a project publishes, its builds,
//! and the java a version needs. One client serves every project on the
//! service — Paper and Folia are the two Hestia registers as flavors.
//!
//! Fill v2 (`api.papermc.io`) stopped receiving builds at the end of 2025 and
//! was disabled on 1 July 2026, so v3 is the only live surface. Its docs ask
//! for a user agent identifying the caller; it does not enforce one, so that is
//! etiquette rather than a requirement (see `common::app::user_agent`).

use anyhow::{Context, Result};
use proto::download::{Checksum, HashAlgorithm};
use proto::minecraft::Artifact;
use serde_json::Value;

use proto::error::Service;

use super::fetch_json;

const META: &str = "https://fill.papermc.io/v3";

/// The download every server build publishes: Fill names artifacts by role, and
/// a launchable server jar is always this one.
const SERVER_DOWNLOAD: &str = "server:default";

/// What a build was released as. Only `Stable` is picked automatically; the
/// rest are reachable by pinning the build explicitly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Stable,
    Beta,
    Alpha,
}

impl Channel {
    fn parse(raw: &str) -> Option<Self> {
        match raw.to_ascii_uppercase().as_str() {
            // RECOMMENDED is the newest STABLE re-labelled, not a fourth kind.
            "STABLE" | "RECOMMENDED" => Some(Channel::Stable),
            "BETA" => Some(Channel::Beta),
            "ALPHA" => Some(Channel::Alpha),
            _ => None,
        }
    }
}

pub struct Build {
    pub number: u64,
    pub channel: Channel,
    pub download: Artifact,
}

/// Every game version a project publishes, in no meaningful order: Fill groups
/// them under a JSON object keyed by version group (`{"1.21": […], "1.20": […]}`)
/// and a parsed object sorts its keys as strings, which puts `1.9` after `1.21`.
/// The caller orders the set against Mojang's manifest, which is authoritative
/// newest-first and is the ground truth `downgrade_between` reads.
pub async fn game_versions(project: &str) -> Result<Vec<String>> {
    let body = fetch_json(Service::Paper, &format!("{META}/projects/{project}")).await?;
    let groups = body
        .get("versions")
        .and_then(Value::as_object)
        .with_context(|| format!("papermc {project} response carries no versions"))?;
    Ok(groups
        .values()
        .filter_map(Value::as_array)
        .flatten()
        .filter_map(Value::as_str)
        .map(String::from)
        .collect())
}

/// Every build published for a game version, newest first.
pub async fn builds(project: &str, version: &str) -> Result<Vec<Build>> {
    let body = fetch_json(
        Service::Paper,
        &format!("{META}/projects/{project}/versions/{version}/builds"),
    )
    .await?;
    let list = body
        .as_array()
        .with_context(|| format!("papermc {project} {version} builds is not an array"))?;
    Ok(list.iter().filter_map(parse_build).collect())
}

/// The build a create resolves to when none was pinned: the newest stable one,
/// falling back to the newest of any channel — a version whose builds are all
/// experimental (a fresh game release) is still installable, deliberately, since
/// refusing it would make the version unusable until Paper promotes a build.
pub async fn newest_build(project: &str, version: &str) -> Result<Build> {
    let builds = builds(project, version).await?;
    let stable = builds.iter().position(|b| b.channel == Channel::Stable);
    let index = stable.unwrap_or(0);
    builds
        .into_iter()
        .nth(index)
        .with_context(|| format!("no {project} build is published for Minecraft {version}"))
}

/// The build a pinned `loader_version` names.
pub async fn build(project: &str, version: &str, number: &str) -> Result<Build> {
    let body = fetch_json(
        Service::Paper,
        &format!("{META}/projects/{project}/versions/{version}/builds/{number}"),
    )
    .await?;
    parse_build(&body)
        .with_context(|| format!("{project} build {number} for Minecraft {version} is unusable"))
}

/// What a version's builds need from the JVM: the minimum major, and the flags
/// PaperMC itself recommends running with.
pub struct Java {
    pub major: i32,
    pub flags: Vec<String>,
}

pub async fn java(project: &str, version: &str) -> Result<Java> {
    let body = fetch_json(
        Service::Paper,
        &format!("{META}/projects/{project}/versions/{version}"),
    )
    .await?;
    let java = body.get("version").and_then(|v| v.get("java"));
    let major = java
        .and_then(|j| j.get("version"))
        .and_then(|v| v.get("minimum"))
        .and_then(Value::as_i64)
        .with_context(|| format!("papermc {project} {version} states no java version"))?;
    let flags = java
        .and_then(|j| j.get("flags"))
        .and_then(|f| f.get("recommended"))
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();
    Ok(Java {
        major: major as i32,
        flags,
    })
}

/// A build entry, or `None` when it carries no launchable server jar — a build
/// that published only sources or failed to upload is skipped rather than
/// offered as a version that cannot start.
fn parse_build(value: &Value) -> Option<Build> {
    let download = value.get("downloads")?.get(SERVER_DOWNLOAD)?;
    let sha256 = download
        .get("checksums")
        .and_then(|c| c.get("sha256"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    Some(Build {
        number: value.get("id").and_then(Value::as_u64)?,
        channel: Channel::parse(value.get("channel").and_then(Value::as_str)?)?,
        download: Artifact {
            url: download.get("url").and_then(Value::as_str)?.to_string(),
            filename: download.get("name").and_then(Value::as_str)?.to_string(),
            size: download.get("size").and_then(Value::as_u64).unwrap_or(0),
            checksum: (!sha256.is_empty()).then_some(Checksum {
                algorithm: HashAlgorithm::Sha256,
                hex: sha256,
            }),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn build_json(id: u64, channel: &str) -> Value {
        json!({
            "id": id,
            "channel": channel,
            "downloads": {
                "server:default": {
                    "name": format!("paper-1.21.8-{id}.jar"),
                    "size": 52811717,
                    "url": "https://fill-data.papermc.io/v1/objects/abc/paper.jar",
                    "checksums": { "sha256": "abc" },
                }
            }
        })
    }

    #[test]
    fn a_build_carries_its_jar_and_sha256() {
        let build = parse_build(&build_json(60, "STABLE")).unwrap();
        assert_eq!(build.number, 60);
        assert_eq!(build.channel, Channel::Stable);
        assert_eq!(
            build.download.checksum.unwrap().algorithm,
            HashAlgorithm::Sha256
        );
    }

    #[test]
    fn a_build_with_no_server_jar_is_skipped() {
        let mut value = build_json(61, "STABLE");
        value["downloads"] = json!({ "sources:default": { "url": "x" } });
        assert!(
            parse_build(&value).is_none(),
            "a build with nothing launchable is not offered"
        );
    }
}
