//! The Bedrock Dedicated Server catalogue: Mojang's download links name the
//! current stable and preview builds, and the Bedrock-OSS feed carries the
//! history Mojang no longer publishes. Both are merged newest first; Mojang's
//! own URL wins for the builds it names.

use std::collections::HashSet;

use anyhow::{Context, Result};
use proto::error::{ErrorInfo, Service, Unsupported};
use proto::minecraft::{GameVersion, VersionKind};
use serde_json::Value;

use super::fetch_json;

const LINKS: &str = "https://net-secondary.web.minecraft-services.net/api/v1.0/download/links";
const FEED: &str = "https://raw.githubusercontent.com/Bedrock-OSS/BDS-Versions/main/versions.json";
const CDN_PREFIX: &str = "https://www.minecraft.net/bedrockdedicatedserver/";

#[derive(Debug, Clone, Copy)]
struct Host {
    feed_key: &'static str,
    mojang_suffix: &'static str,
    cdn_dir: &'static str,
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn host() -> Option<Host> {
    Some(Host {
        feed_key: "linux",
        mojang_suffix: "Linux",
        cdn_dir: "bin-linux",
    })
}

#[cfg(all(target_os = "windows", target_arch = "x86_64"))]
fn host() -> Option<Host> {
    Some(Host {
        feed_key: "windows",
        mojang_suffix: "Windows",
        cdn_dir: "bin-win",
    })
}

#[cfg(not(any(
    all(target_os = "linux", target_arch = "x86_64"),
    all(target_os = "windows", target_arch = "x86_64")
)))]
fn host() -> Option<Host> {
    None
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Build {
    id: String,
    preview: bool,
    mojang_url: Option<String>,
}

struct Feed {
    cdn_root: String,
    stable: Vec<String>,
    preview: Vec<String>,
}

/// Newest first, stable and preview interleaved by version number.
pub async fn versions() -> Result<Vec<GameVersion>> {
    let Some(host) = host() else {
        return Ok(Vec::new());
    };
    let (builds, _) = catalogue(host).await?;
    Ok(builds
        .into_iter()
        .map(|build| GameVersion {
            id: build.id,
            kind: if build.preview {
                VersionKind::Snapshot
            } else {
                VersionKind::Release
            },
            stable: !build.preview,
        })
        .collect())
}

pub async fn download_url(version: &str) -> Result<String> {
    let host = host().ok_or(ErrorInfo::UnsupportedOperation {
        reason: Unsupported::BedrockHost,
    })?;
    let (builds, cdn_root) = catalogue(host).await?;
    let build = builds
        .iter()
        .find(|build| build.id == version)
        .with_context(|| format!("Bedrock Dedicated Server {version} is not published"))?;
    resolve_url(host, build, cdn_root.as_deref())
}

async fn catalogue(host: Host) -> Result<(Vec<Build>, Option<String>)> {
    let (links, feed) = tokio::join!(
        fetch_json(Service::Mojang, LINKS),
        fetch_json(Service::Mojang, FEED)
    );
    let links = links
        .and_then(|body| parse_links(&body, host))
        .map_err(|error| tracing::warn!(%error, "bedrock: Mojang download links unavailable"))
        .ok();
    let feed = feed
        .and_then(|body| parse_feed(&body, host))
        .map_err(|error| tracing::warn!(%error, "bedrock: version feed unavailable"))
        .ok();
    if links.is_none() && feed.is_none() {
        anyhow::bail!("no Bedrock Dedicated Server catalogue is reachable");
    }
    let cdn_root = feed.as_ref().map(|feed| feed.cdn_root.clone());
    let builds = merge(feed, links.unwrap_or_default());
    tracing::debug!(count = builds.len(), "bedrock: merged catalogue");
    Ok((builds, cdn_root))
}

fn parse_links(body: &Value, host: Host) -> Result<Vec<Build>> {
    let links = body
        .pointer("/result/links")
        .and_then(Value::as_array)
        .context("Mojang download links carry no list")?;
    let find = |kind: &str| {
        links
            .iter()
            .find(|link| link.get("downloadType").and_then(Value::as_str) == Some(kind))
            .and_then(|link| link.get("downloadUrl")?.as_str())
    };
    let mut builds = Vec::new();
    for (kind, preview) in [
        (format!("serverBedrock{}", host.mojang_suffix), false),
        (format!("serverBedrockPreview{}", host.mojang_suffix), true),
    ] {
        let Some(url) = find(&kind) else { continue };
        let id = url
            .rsplit('/')
            .next()
            .and_then(|file| file.strip_prefix("bedrock-server-")?.strip_suffix(".zip"))
            .with_context(|| format!("unrecognised Bedrock server link {url}"))?;
        builds.push(Build {
            id: id.to_string(),
            preview,
            mojang_url: Some(url.to_string()),
        });
    }
    Ok(builds)
}

fn parse_feed(body: &Value, host: Host) -> Result<Feed> {
    let cdn_root = body
        .get("cdn_root")
        .and_then(Value::as_str)
        .context("Bedrock version feed names no cdn_root")?
        .trim_end_matches('/')
        .to_string();
    let section = body
        .get(host.feed_key)
        .with_context(|| format!("Bedrock version feed has no {} section", host.feed_key))?;
    let strings = |key: &str| -> Vec<String> {
        section
            .get(key)
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(Value::as_str)
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default()
    };
    let mut stable = strings("versions");
    stable.extend(
        section
            .get("stable")
            .and_then(Value::as_str)
            .map(String::from),
    );
    let mut preview = strings("preview_versions");
    preview.extend(
        section
            .get("preview")
            .and_then(Value::as_str)
            .map(String::from),
    );
    Ok(Feed {
        cdn_root,
        stable,
        preview,
    })
}

fn merge(feed: Option<Feed>, links: Vec<Build>) -> Vec<Build> {
    let mut builds = links;
    if let Some(feed) = feed {
        for (ids, preview) in [(feed.stable, false), (feed.preview, true)] {
            builds.extend(ids.into_iter().map(|id| Build {
                id,
                preview,
                mojang_url: None,
            }));
        }
    }
    let mut seen = HashSet::new();
    builds.retain(|build| seen.insert(build.id.clone()));
    builds.sort_by_cached_key(|build| std::cmp::Reverse(version_key(&build.id)));
    builds
}

fn version_key(id: &str) -> Vec<u64> {
    id.split('.')
        .map(|part| part.parse().unwrap_or(0))
        .collect()
}

fn resolve_url(host: Host, build: &Build, cdn_root: Option<&str>) -> Result<String> {
    let url = match (&build.mojang_url, cdn_root) {
        (Some(url), _) => url.clone(),
        (None, Some(root)) => format!(
            "{root}/{}{}/bedrock-server-{}.zip",
            host.cdn_dir,
            if build.preview { "-preview" } else { "" },
            build.id
        ),
        (None, None) => anyhow::bail!("no download location is known for {}", build.id),
    };
    if !url.starts_with(CDN_PREFIX) || url.contains("..") {
        anyhow::bail!("refusing a Bedrock download outside minecraft.net: {url}");
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const ROOT: &str = "https://www.minecraft.net/bedrockdedicatedserver";
    const LINUX: Host = Host {
        feed_key: "linux",
        mojang_suffix: "Linux",
        cdn_dir: "bin-linux",
    };
    const WINDOWS: Host = Host {
        feed_key: "windows",
        mojang_suffix: "Windows",
        cdn_dir: "bin-win",
    };

    fn links() -> Value {
        json!({"result": {"links": [
            {"downloadType": "serverBedrockWindows", "downloadUrl": format!("{ROOT}/bin-win/bedrock-server-1.26.63.1.zip")},
            {"downloadType": "serverBedrockLinux", "downloadUrl": format!("{ROOT}/bin-linux/bedrock-server-1.26.63.1.zip")},
            {"downloadType": "serverBedrockPreviewLinux", "downloadUrl": format!("{ROOT}/bin-linux-preview/bedrock-server-1.26.70.23.zip")},
            {"downloadType": "serverJar", "downloadUrl": "https://piston-data.mojang.com/x/server.jar"}
        ]}})
    }

    fn feed() -> Value {
        json!({"cdn_root": ROOT, "linux": {
            "stable": "1.21.0.3", "preview": "1.21.10.20",
            "versions": ["1.9.0.15", "1.21.0.3", "1.10.0.7"],
            "preview_versions": ["1.21.10.20"]
        }, "windows": {"versions": ["1.9.0.15"], "stable": "1.9.0.15"}})
    }

    fn built(host: Host) -> Vec<Build> {
        merge(
            Some(parse_feed(&feed(), host).unwrap()),
            parse_links(&links(), host).unwrap(),
        )
    }

    #[test]
    fn parses_mojang_links_for_the_host() {
        let linux = parse_links(&links(), LINUX).unwrap();
        assert_eq!(linux.len(), 2);
        assert_eq!(linux[0].id, "1.26.63.1");
        assert!(linux[1].preview && linux[1].id == "1.26.70.23");
        assert_eq!(parse_links(&links(), WINDOWS).unwrap().len(), 1);
    }

    #[test]
    fn parses_the_feed_for_the_host() {
        let feed = parse_feed(&feed(), WINDOWS).unwrap();
        assert_eq!(feed.stable, ["1.9.0.15", "1.9.0.15"]);
        assert!(feed.preview.is_empty());
    }

    #[test]
    fn merge_adds_mojang_build_the_feed_lacks_newest_first() {
        let builds = built(LINUX);
        let ids: Vec<_> = builds.iter().map(|b| b.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "1.26.70.23",
                "1.26.63.1",
                "1.21.10.20",
                "1.21.0.3",
                "1.10.0.7",
                "1.9.0.15"
            ]
        );
    }

    #[test]
    fn previews_are_flagged() {
        let builds = built(LINUX);
        assert!(builds[0].preview);
        assert!(!builds[1].preview);
        assert!(
            builds
                .iter()
                .find(|b| b.id == "1.21.10.20")
                .unwrap()
                .preview
        );
    }

    #[test]
    fn builds_cdn_urls_for_both_platforms() {
        let stable = Build {
            id: "1.21.0.3".into(),
            preview: false,
            mojang_url: None,
        };
        let preview = Build {
            preview: true,
            ..stable.clone()
        };
        let url = |host, build: &Build| resolve_url(host, build, Some(ROOT)).unwrap();
        assert_eq!(
            url(LINUX, &stable),
            format!("{ROOT}/bin-linux/bedrock-server-1.21.0.3.zip")
        );
        assert_eq!(
            url(LINUX, &preview),
            format!("{ROOT}/bin-linux-preview/bedrock-server-1.21.0.3.zip")
        );
        assert_eq!(
            url(WINDOWS, &stable),
            format!("{ROOT}/bin-win/bedrock-server-1.21.0.3.zip")
        );
        assert_eq!(
            url(WINDOWS, &preview),
            format!("{ROOT}/bin-win-preview/bedrock-server-1.21.0.3.zip")
        );
    }

    #[test]
    fn mojang_url_wins_for_its_own_build() {
        let builds = built(LINUX);
        let current = builds.iter().find(|b| b.id == "1.26.63.1").unwrap();
        assert_eq!(
            resolve_url(LINUX, current, Some(ROOT)).unwrap(),
            format!("{ROOT}/bin-linux/bedrock-server-1.26.63.1.zip")
        );
    }

    #[test]
    fn refuses_urls_outside_minecraft_net() {
        let build = Build {
            id: "1.21.0.3".into(),
            preview: false,
            mojang_url: None,
        };
        assert!(resolve_url(LINUX, &build, Some("https://evil.example/x")).is_err());
        let hostile = Build {
            mojang_url: Some("https://evil.example/bedrock-server-1.0.0.0.zip".into()),
            ..build
        };
        assert!(resolve_url(LINUX, &hostile, None).is_err());
    }
}
