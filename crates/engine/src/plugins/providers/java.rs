//! Java distributions contributed by plugins. A plugin's `java-provider` export
//! is wrapped in a [`PluginJavaProvider`], which the Java aggregate treats like
//! the built-in one. A runtime is native code Hestia executes, so the guest only
//! names a build: the host downloads it, only from the plugin's network hosts
//! and only against a sha256, and extracts it itself.

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use proto::download::{Checksum, HashAlgorithm};
use proto::java::JavaRelease;
use proto::plugin::{Grant, Manifest};
use proto::provider::java::{self as op, Distribution, JavaBuild, TargetQuery};
use proto::Empty;

use super::{contributed, registered, Guest};
use crate::engine::Engine;
use crate::java::{JavaPackage, JavaProvider, JavaTarget};
use crate::plugins::http::download_allowed;
use crate::plugins::runtime::LoadedPlugin;

/// Why a guest's build was refused.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
enum Refusal {
    #[error("`{0}` is not downloadable from the plugin's network hosts")]
    Host(String),
    #[error("the build has no well-formed sha256")]
    Checksum,
    #[error("`{0}` is not an archive kind Hestia extracts (tar.gz or zip)")]
    Archive(String),
    #[error("the vendor `{0}` is not a plain name")]
    Vendor(String),
    #[error("the build names no version")]
    Version,
}

fn vet(grant: &Grant, major: i32, build: JavaBuild) -> Result<JavaPackage, Refusal> {
    let plain = !build.vendor.is_empty()
        && build
            .vendor
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    if !plain {
        return Err(Refusal::Vendor(build.vendor));
    }
    if build.version.trim().is_empty() {
        return Err(Refusal::Version);
    }
    if !download_allowed(grant, &build.url) {
        return Err(Refusal::Host(build.url));
    }
    let checksum = Checksum {
        algorithm: HashAlgorithm::Sha256,
        hex: build.sha256,
    };
    if !checksum.is_valid() {
        return Err(Refusal::Checksum);
    }
    if !matches!(build.archive.as_str(), "tar.gz" | "zip") {
        return Err(Refusal::Archive(build.archive));
    }
    Ok(JavaPackage {
        archive_name: format!("{}-{major}.{}", build.vendor, build.archive),
        vendor: build.vendor,
        major,
        release_name: build.version,
        url: build.url,
        checksum,
        size: 0,
    })
}

/// A Java provider a plugin contributes, registered as `<plugin>.<id>`, or bare when official.
pub struct PluginJavaProvider {
    guest: Guest,
}

#[async_trait]
impl JavaProvider for PluginJavaProvider {
    fn id(&self) -> &str {
        self.guest.id()
    }

    async fn releases(&self) -> Result<Vec<JavaRelease>> {
        let mut majors = self.guest.ask::<op::Majors>(&Empty {}).await?;
        majors.sort_by_key(|release| release.major);
        majors.dedup_by_key(|release| release.major);
        Ok(majors)
    }

    async fn resolve(&self, major: i32, target: &JavaTarget) -> Result<JavaPackage> {
        let query = TargetQuery {
            major,
            os: target.os.to_string(),
            arch: target.arch.to_string(),
        };
        let Some(build) = self.guest.ask::<op::Resolve>(&query).await? else {
            return Err(self.guest.failed(format!(
                "no java {major} build for {}/{}",
                target.os, target.arch
            )));
        };
        vet(self.guest.plugin().grant(), major, build).map_err(|refusal| {
            tracing::warn!(provider = %self.guest.id(), %refusal, "java build refused");
            self.guest
                .failed(format!("resolve returned a refused build: {refusal}"))
        })
    }
}

/// Nothing is asked of the guest until a build is resolved, so none can fail
/// to connect.
pub(super) fn connect(
    plugin: &Arc<LoadedPlugin>,
    manifest: &Manifest,
    official: bool,
) -> Vec<PluginJavaProvider> {
    contributed::<Distribution>(manifest)
        .map(|local| PluginJavaProvider {
            guest: Guest::new::<Distribution>(plugin.clone(), manifest, local, official),
        })
        .collect()
}

pub(super) fn register(
    engine: &Engine,
    owner: &str,
    official: bool,
    providers: Vec<PluginJavaProvider>,
) {
    for provider in providers {
        let id = provider.guest.id().to_string();
        let outcome = engine.java().register(owner, official, Arc::new(provider));
        registered(owner, "java", &id, outcome);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn grant() -> Grant {
        Grant {
            network: ["api.azul.com".to_string()].into(),
            ..Grant::default()
        }
    }

    fn build() -> JavaBuild {
        JavaBuild {
            vendor: "zulu".into(),
            version: "21.0.2".into(),
            url: "https://api.azul.com/zulu.tar.gz".into(),
            sha256: SHA.into(),
            archive: "tar.gz".into(),
        }
    }

    #[test]
    fn a_well_formed_build_becomes_a_package() {
        let package = vet(&grant(), 21, build()).expect("accepted");
        assert_eq!(package.archive_name, "zulu-21.tar.gz");
        assert_eq!(package.release_name, "21.0.2");
        assert_eq!(package.checksum.hex, SHA);
    }

    #[test]
    fn an_off_host_url_is_refused() {
        let off = JavaBuild {
            url: "https://evil.example.com/zulu.tar.gz".into(),
            ..build()
        };
        assert!(matches!(vet(&grant(), 21, off), Err(Refusal::Host(_))));
    }

    #[test]
    fn plain_http_is_refused() {
        let http = JavaBuild {
            url: "http://api.azul.com/zulu.tar.gz".into(),
            ..build()
        };
        assert!(matches!(vet(&grant(), 21, http), Err(Refusal::Host(_))));
    }

    #[test]
    fn a_bad_sha256_is_refused() {
        for sha in ["", "abc", &"g".repeat(64)] {
            let bad = JavaBuild {
                sha256: sha.into(),
                ..build()
            };
            assert_eq!(vet(&grant(), 21, bad).err(), Some(Refusal::Checksum));
        }
    }

    #[test]
    fn an_unknown_archive_kind_or_unsafe_vendor_is_refused() {
        let rar = JavaBuild {
            archive: "rar".into(),
            ..build()
        };
        assert!(matches!(vet(&grant(), 21, rar), Err(Refusal::Archive(_))));
        let traversal = JavaBuild {
            vendor: "../x".into(),
            ..build()
        };
        assert!(matches!(
            vet(&grant(), 21, traversal),
            Err(Refusal::Vendor(_))
        ));
    }
}
