use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::Url;
use sha2::{Digest, Sha256};

use super::error::TrustError;
use super::index::{Artifact, Index};
use super::repositories::Repository;
use super::signature::Signer;
use crate::install::{COMPONENT_MAX, MANIFEST_MAX};
use crate::vetting::{self, Failure, Hosts, Limits, Policy};

const INDEX_MAX: u64 = 4 << 20;
const SIGNATURE_MAX: u64 = 4 << 10;
const SIGNATURE_SUFFIX: &str = ".minisig";

fn limits(bytes: u64) -> Limits {
    Limits {
        time: Some(Duration::from_secs(120)),
        bytes,
    }
}

/// A policy for one repository's host and the hosts its signed index adds. A
/// test build with the `loopback` feature reaches plain-http loopback instead.
fn policy(url: &Url, extra: &[String], unhashed: bool) -> Result<Policy, TrustError> {
    #[cfg(feature = "loopback")]
    if url.scheme() == "http" {
        return Ok(Policy::new(
            Hosts::new::<&str>(&[], &[])?.with_loopback(unhashed),
        ));
    }
    let host = url
        .host_str()
        .ok_or_else(|| TrustError::Fetch("the repository URL has no host".into()))?;
    let mut connect = vec![host];
    connect.extend(extra.iter().map(String::as_str));
    let served: &[&str] = if unhashed { &connect[..1] } else { &[] };
    Ok(Policy::new(Hosts::new(&connect, served)?))
}

async fn get(policy: &Policy, url: &str, unhashed: bool, max: u64) -> Result<Vec<u8>, TrustError> {
    let fail = |failure: Failure<reqwest::Error>| match failure {
        Failure::Refused(refusal) => TrustError::Fetch(format!("{url}: {refusal}")),
        Failure::Send(error) => TrustError::Fetch(format!("{url}: {error}")),
    };
    let response = policy
        .get(url, unhashed, limits(max), |request| request.send())
        .await
        .map_err(fail)?;
    if !response.status().is_success() {
        return Err(TrustError::Fetch(format!("{url}: {}", response.status())));
    }
    vetting::read(response, limits(max)).await.map_err(fail)
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

/// A verified index and the policy its packages are fetched under.
pub struct Fetched {
    pub index: Index,
    packages: Policy,
}

impl Fetched {
    /// The index of `repository` and its signature, verified under `signer`.
    /// The signature is read from the index URL with `.minisig` added.
    pub async fn index(repository: &Repository, signer: &Signer) -> Result<Fetched, TrustError> {
        let url = Url::parse(&repository.url)
            .map_err(|_| TrustError::Fetch("the repository URL is not valid".into()))?;
        let policy = policy(&url, &[], true)?;
        let body = get(&policy, url.as_str(), true, INDEX_MAX).await?;
        let signature = get(
            &policy,
            &format!("{url}{SIGNATURE_SUFFIX}"),
            true,
            SIGNATURE_MAX,
        )
        .await?;
        let signature = String::from_utf8(signature).map_err(|_| TrustError::BadSignature)?;
        let index = Index::verify(
            &body,
            &signature,
            signer,
            &repository.id,
            repository.sequence,
            now(),
        )?;
        let packages = self::policy(&url, &index.hosts, false)?;
        Ok(Fetched { index, packages })
    }

    /// One file of a package, held to the digest the signed index lists.
    pub async fn artifact(&self, artifact: &Artifact, max: u64) -> Result<Vec<u8>, TrustError> {
        let bytes = get(&self.packages, &artifact.url, false, max).await?;
        let digest: String = Sha256::digest(&bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        match digest.eq_ignore_ascii_case(&artifact.sha256) {
            true => Ok(bytes),
            false => Err(TrustError::Digest(artifact.url.clone())),
        }
    }

    pub async fn manifest(&self, artifact: &Artifact) -> Result<Vec<u8>, TrustError> {
        self.artifact(artifact, MANIFEST_MAX).await
    }

    pub async fn component(&self, artifact: &Artifact) -> Result<Vec<u8>, TrustError> {
        self.artifact(artifact, COMPONENT_MAX).await
    }
}
