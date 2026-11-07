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
use proto::plugin::{Grant, Manifest, JAVA_PROVIDER};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};

use super::call_failed;
use crate::java::{Java, JavaPackage, JavaProvider, JavaTarget};
use crate::plugins::chain::CallChain;
use crate::plugins::http::download_allowed;
use crate::plugins::runtime::{LoadedPlugin, PROVIDER_TIMEOUT};

/// One function of the `java-provider` export.
#[derive(Debug, Clone, Copy)]
pub(in crate::plugins) enum JavaOp {
    Majors,
    Resolve,
}

impl JavaOp {
    fn name(self) -> &'static str {
        match self {
            JavaOp::Majors => "majors",
            JavaOp::Resolve => "resolve",
        }
    }
}

/// What a guest answers to `resolve`.
#[derive(Deserialize)]
struct Build {
    vendor: String,
    version: String,
    url: String,
    sha256: String,
    archive: String,
}

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

fn vet(grant: &Grant, major: i32, build: Build) -> Result<JavaPackage, Refusal> {
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

/// A Java provider a plugin contributes, registered as `<plugin>.<id>`.
pub struct PluginJavaProvider {
    id: String,
    local: String,
    plugin: Arc<LoadedPlugin>,
}

impl PluginJavaProvider {
    pub fn new(plugin: Arc<LoadedPlugin>, local: &str) -> Self {
        PluginJavaProvider {
            id: format!("{}.{local}", plugin.id()),
            local: local.to_string(),
            plugin,
        }
    }

    async fn ask<T: DeserializeOwned>(&self, op: JavaOp, args: Value) -> Result<T> {
        let reply = self
            .plugin
            .java_provider(
                &CallChain::default(),
                op,
                &self.local,
                &args.to_string(),
                PROVIDER_TIMEOUT,
            )
            .await
            .map_err(|error| self.failed(format!("{}: {error}", op.name())))?;
        serde_json::from_str(&reply).map_err(|error| {
            tracing::warn!(plugin = self.plugin.id(), provider = %self.local, op = op.name(), %error, "malformed java provider reply");
            self.failed(format!("{} returned an unreadable reply", op.name()))
        })
    }

    fn failed(&self, detail: String) -> anyhow::Error {
        call_failed(self.plugin.id(), detail)
    }
}

#[async_trait]
impl JavaProvider for PluginJavaProvider {
    fn id(&self) -> &str {
        &self.id
    }

    async fn releases(&self) -> Result<Vec<JavaRelease>> {
        let mut majors: Vec<i32> = self.ask(JavaOp::Majors, Value::Null).await?;
        majors.sort_unstable();
        majors.dedup();
        Ok(majors
            .into_iter()
            .map(|major| JavaRelease { major, lts: false })
            .collect())
    }

    async fn resolve(&self, major: i32, target: &JavaTarget) -> Result<JavaPackage> {
        let args = json!({ "major": major, "os": target.os, "arch": target.arch });
        let build: Option<Build> = self.ask(JavaOp::Resolve, args).await?;
        let Some(build) = build else {
            return Err(self.failed(format!(
                "no java {major} build for {}/{}",
                target.os, target.arch
            )));
        };
        vet(self.plugin.grant(), major, build).map_err(|refusal| {
            tracing::warn!(plugin = self.plugin.id(), provider = %self.local, %refusal, "java build refused");
            self.failed(format!("resolve returned a refused build: {refusal}"))
        })
    }
}

/// The Java providers the manifest contributes. Nothing is asked of the guest
/// until a build is resolved, so none can fail to connect.
pub fn connect_java(plugin: &Arc<LoadedPlugin>, manifest: &Manifest) -> Vec<PluginJavaProvider> {
    manifest
        .contributes
        .providers
        .iter()
        .filter(|contribution| contribution.kind == JAVA_PROVIDER)
        .map(|contribution| PluginJavaProvider::new(plugin.clone(), &contribution.id))
        .collect()
}

/// Adds `providers` to `java` under `owner`. `official` is whether the
/// plugin's key is the official one; see [`Roster::register`](crate::roster).
pub fn register_java(java: &Java, owner: &str, official: bool, providers: Vec<PluginJavaProvider>) {
    for provider in providers {
        let id = provider.id.clone();
        match java.register(owner, official, Arc::new(provider)) {
            Ok(()) => tracing::info!(plugin = owner, provider = %id, "java provider registered"),
            Err(error) => {
                tracing::warn!(plugin = owner, provider = %id, %error, "java provider refused")
            }
        }
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

    fn build() -> Build {
        Build {
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
        let off = Build {
            url: "https://evil.example.com/zulu.tar.gz".into(),
            ..build()
        };
        assert!(matches!(vet(&grant(), 21, off), Err(Refusal::Host(_))));
    }

    #[test]
    fn plain_http_is_refused() {
        let http = Build {
            url: "http://api.azul.com/zulu.tar.gz".into(),
            ..build()
        };
        assert!(matches!(vet(&grant(), 21, http), Err(Refusal::Host(_))));
    }

    #[test]
    fn a_bad_sha256_is_refused() {
        for sha in ["", "abc", &"g".repeat(64)] {
            let bad = Build {
                sha256: sha.into(),
                ..build()
            };
            assert_eq!(vet(&grant(), 21, bad).err(), Some(Refusal::Checksum));
        }
    }

    #[test]
    fn an_unknown_archive_kind_or_unsafe_vendor_is_refused() {
        let rar = Build {
            archive: "rar".into(),
            ..build()
        };
        assert!(matches!(vet(&grant(), 21, rar), Err(Refusal::Archive(_))));
        let traversal = Build {
            vendor: "../x".into(),
            ..build()
        };
        assert!(matches!(
            vet(&grant(), 21, traversal),
            Err(Refusal::Vendor(_))
        ));
    }
}
