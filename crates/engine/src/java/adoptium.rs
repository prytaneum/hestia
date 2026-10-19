//! Eclipse Temurin release catalogue via the Adoptium API. Fetches metadata
//! only; downloading, extraction, and registration live in `Java`.

use anyhow::{bail, Context, Result};
use proto::download::{Checksum, HashAlgorithm};
use proto::error::Service;
use proto::java::JavaRelease;
use serde::de::DeserializeOwned;
use serde::Deserialize;

use super::platform::JavaTarget;

const API_BASE: &str = "https://api.adoptium.net";

pub struct JavaPackage {
    pub vendor: String,
    pub major: i32,
    pub release_name: String,
    pub url: String,
    pub archive_name: String,
    pub checksum: Checksum,
    pub size: u64,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct AvailableReleases {
    available_releases: Option<Vec<i32>>,
    available_lts_releases: Vec<i32>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Asset {
    release_name: String,
    binary: Binary,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Binary {
    os: String,
    architecture: String,
    image_type: String,
    package: Package,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Package {
    link: String,
    name: String,
    checksum: String,
    size: u64,
}

async fn fetch_json<T: DeserializeOwned>(url: &str, query: &[(&str, &str)]) -> Result<T> {
    tracing::debug!(url, "adoptium GET");
    let response = crate::net::send(
        Some(Service::Adoptium),
        crate::net::client().get(url).query(query),
    )
    .await?;
    let response = crate::net::require_success(Service::Adoptium, response)?;
    response
        .json()
        .await
        .context("adoptium returned malformed JSON")
}

pub async fn releases() -> Result<Vec<JavaRelease>> {
    let j = fetch_json(&format!("{API_BASE}/v3/info/available_releases"), &[]).await?;
    releases_from_json(j)
}

pub async fn resolve(major: i32, target: &JavaTarget) -> Result<JavaPackage> {
    let url = format!("{API_BASE}/v3/assets/latest/{major}/hotspot");
    let assets = fetch_json(
        &url,
        &[
            ("os", &target.os),
            ("architecture", &target.arch),
            ("image_type", "jdk"),
            ("vendor", "eclipse"),
        ],
    )
    .await?;
    package_from_json(assets, major, target)
}

fn releases_from_json(j: AvailableReleases) -> Result<Vec<JavaRelease>> {
    let available = j
        .available_releases
        .context("adoptium response is missing available_releases")?;
    let mut releases: Vec<JavaRelease> = available
        .into_iter()
        .map(|major| JavaRelease {
            major,
            lts: j.available_lts_releases.contains(&major),
        })
        .collect();
    releases.sort_by_key(|r| r.major);
    Ok(releases)
}

fn package_from_json(assets: Vec<Asset>, major: i32, target: &JavaTarget) -> Result<JavaPackage> {
    for asset in assets {
        let binary = asset.binary;
        if binary.os != target.os
            || binary.architecture != target.arch
            || binary.image_type != "jdk"
        {
            continue;
        }
        let package = binary.package;
        let resolved = JavaPackage {
            vendor: "temurin".to_string(),
            major,
            release_name: asset.release_name,
            url: package.link,
            archive_name: package.name,
            checksum: Checksum {
                algorithm: HashAlgorithm::Sha256,
                hex: package.checksum,
            },
            size: package.size,
        };
        if resolved.url.is_empty()
            || resolved.archive_name.is_empty()
            || !resolved.checksum.is_valid()
        {
            bail!("adoptium build for temurin {major} is missing its download link or checksum");
        }
        return Ok(resolved);
    }
    bail!(
        "no temurin {major} jdk build is published for {}/{}",
        target.os,
        target.arch
    )
}
