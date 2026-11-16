use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::Url;
use sha2::{Digest, Sha256};

use super::error::TrustError;
use super::index::{Index, Release};
use super::repositories::Repository;
use super::signature::Signer;
use crate::vetting::{self, Failure, Hosts, Limits, Policy};

const INDEX_MAX: u64 = 4 << 20;
const SIGNATURE_MAX: u64 = 4 << 10;
const SIGNATURE_SUFFIX: &str = ".minisig";
/// Per request. A repository costs an index and its signature, so adding one
/// or checking one for updates takes 40 s at most.
const INDEX_TIME: Duration = Duration::from_secs(20);
const ARCHIVE_TIME: Duration = Duration::from_secs(120);

/// How a repository's URL and the hosts its index names become the policy
/// every request to it passes. [`policy`] is the one production uses; a test
/// that serves files locally hands in its own.
pub(crate) type PolicyFor = fn(&Url, &[String], bool) -> Result<Policy, TrustError>;

fn limits(time: Duration, bytes: u64) -> Limits {
    Limits {
        time: Some(time),
        bytes,
    }
}

/// A policy for one repository's host and the hosts its signed index adds. Only
/// the repository's own host may serve what nothing verifies, and only while
/// `unhashed`.
pub(crate) fn policy(url: &Url, extra: &[String], unhashed: bool) -> Result<Policy, TrustError> {
    let host = url
        .host_str()
        .ok_or_else(|| TrustError::Fetch("the repository URL has no host".into()))?;
    let mut connect = vec![host];
    connect.extend(extra.iter().map(String::as_str));
    let served: &[&str] = if unhashed { &connect[..1] } else { &[] };
    Ok(Policy::new(Hosts::new(&connect, served)?))
}

/// The policy of a test that serves its files over plain-http loopback.
#[cfg(feature = "loopback")]
pub(crate) fn loopback(_: &Url, _: &[String], unhashed: bool) -> Result<Policy, TrustError> {
    Ok(Policy::new(
        Hosts::new::<&str>(&[], &[])?.with_loopback(unhashed),
    ))
}

async fn get(
    policy: &Policy,
    url: &str,
    unhashed: bool,
    limits: Limits,
) -> Result<Vec<u8>, TrustError> {
    let fail = |failure: Failure<reqwest::Error>| match failure {
        Failure::Refused(refusal) => TrustError::Fetch(format!("{url}: {refusal}")),
        Failure::Send(error) => TrustError::Fetch(format!("{url}: {error}")),
    };
    let response = policy
        .get(url, unhashed, limits, |request| request.send())
        .await
        .map_err(fail)?;
    if !response.status().is_success() {
        return Err(TrustError::Fetch(format!("{url}: {}", response.status())));
    }
    vetting::read(response, limits).await.map_err(fail)
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
    pub async fn index(
        repository: &Repository,
        signer: &Signer,
        policy_for: PolicyFor,
    ) -> Result<Fetched, TrustError> {
        let url = Url::parse(&repository.url)
            .map_err(|_| TrustError::Fetch("the repository URL is not valid".into()))?;
        let policy = policy_for(&url, &[], true)?;
        let body = get(&policy, url.as_str(), true, limits(INDEX_TIME, INDEX_MAX)).await?;
        let signature = get(
            &policy,
            &format!("{url}{SIGNATURE_SUFFIX}"),
            true,
            limits(INDEX_TIME, SIGNATURE_MAX),
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
        let packages = policy_for(&url, &index.hosts, false)?;
        Ok(Fetched { index, packages })
    }

    /// A `.hplugin`, held to the size and digest the signed index lists.
    pub async fn archive(&self, release: &Release) -> Result<Vec<u8>, TrustError> {
        let limits = limits(ARCHIVE_TIME, release.size);
        let bytes = get(&self.packages, &release.url, false, limits).await?;
        let digest: String = Sha256::digest(&bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        match digest.eq_ignore_ascii_case(&release.sha256) {
            true => Ok(bytes),
            false => Err(TrustError::Digest(release.url.clone())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_repository_reaches_its_own_host_and_the_hosts_its_index_names_and_nothing_else() {
        let url = Url::parse("https://repo.example/index.json").unwrap();
        let reach = |policy: &Policy, url: &str, unhashed| {
            policy.vet(&Url::parse(url).unwrap(), unhashed).is_ok()
        };

        let index = policy(&url, &[], true).unwrap();
        assert!(reach(&index, "https://repo.example/index.json", true));
        assert!(!reach(&index, "https://cdn.example/a", true));
        assert!(!reach(&index, "http://repo.example/index.json", true));

        let packages = policy(&url, &["cdn.example".into()], false).unwrap();
        assert!(reach(&packages, "https://cdn.example/a.hplugin", false));
        assert!(reach(&packages, "https://repo.example/a.hplugin", false));
        assert!(
            !reach(&packages, "https://cdn.example/a.hplugin", true),
            "a package is always held to a digest"
        );
        assert!(!reach(&packages, "https://other.example/a.hplugin", false));
    }
}
