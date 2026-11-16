//! Eclipse Temurin release catalogue via the Adoptium API. Fetches metadata
//! only; downloading, extraction, and registration live in `Java`.

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use proto::download::{Checksum, HashAlgorithm};
use proto::error::Service;
use proto::java::JavaRelease;
use proto::provider::Provider;
use serde::de::DeserializeOwned;
use serde::Deserialize;

use super::platform::JavaTarget;
use super::provider::{JavaPackage, JavaProvider};

const API_BASE: &str = "https://api.adoptium.net";

pub const ID: &str = "adoptium";

pub struct Adoptium {
    base: String,
}

impl Adoptium {
    pub fn new() -> Self {
        Self::at(API_BASE)
    }

    /// Point at another API root; the seam a test uses to serve a recorded index.
    pub fn at(base: impl Into<String>) -> Self {
        Adoptium { base: base.into() }
    }
}

impl Default for Adoptium {
    fn default() -> Self {
        Self::new()
    }
}

fn service() -> Service {
    Service::Provider(Provider::new(ID, "Adoptium"))
}

#[async_trait]
impl JavaProvider for Adoptium {
    fn id(&self) -> &str {
        ID
    }

    async fn releases(&self) -> Result<Vec<JavaRelease>> {
        releases(&self.base).await
    }

    async fn resolve(&self, major: i32, target: &JavaTarget) -> Result<JavaPackage> {
        resolve(&self.base, major, target).await
    }
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
    let response =
        crate::net::send(Some(service()), crate::net::client().get(url).query(query)).await?;
    let response = crate::net::require_success(service(), response)?;
    response
        .json()
        .await
        .context("adoptium returned malformed JSON")
}

async fn releases(base: &str) -> Result<Vec<JavaRelease>> {
    let j = fetch_json(&format!("{base}/v3/info/available_releases"), &[]).await?;
    releases_from_json(j)
}

async fn resolve(base: &str, major: i32, target: &JavaTarget) -> Result<JavaPackage> {
    let url = format!("{base}/v3/assets/latest/{major}/hotspot");
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
            policy: None,
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
