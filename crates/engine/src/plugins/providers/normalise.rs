//! Turns what a flavor's guest declared it adds into the plain libraries and
//! arguments the rest of the engine knows: a Maven library gets its path, URL
//! and checksum, and whatever the rules disallow on this machine is dropped.
//! It runs before vetting, so nothing downstream ever sees an unresolved form
//! ([0119](../../../../../docs/decisions/0119-downloads-are-host-side.md)).

use std::collections::{HashMap, HashSet};

use futures_util::{stream, StreamExt, TryStreamExt};
use proto::download::{Checksum, HashAlgorithm};
use proto::profile::Library;
use proto::provider::flavor::{
    Argument, ClientAddition, LibrarySpec, Maven, ServerAddition, Target,
};
use proto::rules::Rule;
use reqwest::Url;

use crate::minecraft::maven::{Coordinate, CoordinateError};
use crate::minecraft::overlay::{ClientLayer, ServerLayer};
use crate::minecraft::rules;
use crate::plugins::vouch::{Policy, Refused};

const SIDECAR_SUFFIX: &str = ".sha1";
const SIDECAR_LOOKUPS: usize = 8;
const SIDECAR_MAX_BYTES: usize = 1024;

/// Why a declared library cannot be completed.
#[derive(Debug, thiserror::Error)]
pub(super) enum Unresolved {
    #[error("{0}")]
    Coordinate(#[from] CoordinateError),
    #[error("the repository of `{0}` is not an http(s) base URL without a query or fragment")]
    Repository(String),
    #[error("a library given as maven coordinate `{0}` also names its own name, path, file or URL, or is built")]
    Explicit(String),
    #[error("{url}: {reason}")]
    Sidecar { url: String, reason: String },
}

/// Either the guest's answer is refused as a whole, or resolving it failed for
/// a reason that is not the guest's (no network).
#[derive(Debug, thiserror::Error)]
pub(super) enum Failure {
    #[error(transparent)]
    Unresolved(#[from] Unresolved),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

struct Pending {
    library: Library,
    sidecar: Option<String>,
}

/// Resolves a flavor's answer for one machine, fetching what it needs under
/// the plugin's own grant.
pub(super) struct Normaliser<'a> {
    pub policy: &'a Policy,
    pub target: &'a Target,
}

impl Normaliser<'_> {
    pub(super) async fn client(&self, addition: ClientAddition) -> Result<ClientLayer, Failure> {
        let declared = addition
            .libraries
            .into_iter()
            .map(complete)
            .collect::<Result<Vec<_>, _>>()?;
        let kept = declared
            .into_iter()
            .filter(|declared| rules::allow(&declared.rules, self.target))
            .map(|declared| declared.pending)
            .collect();
        Ok(ClientLayer {
            loader_version: addition.loader_version,
            libraries: self.sidecars(kept).await?,
            main_class: addition.main_class,
            jvm_args: self.arguments(addition.jvm_args),
            game_args: self.arguments(addition.game_args),
            java_major: addition.java_major,
        })
    }

    pub(super) fn server(&self, addition: ServerAddition) -> ServerLayer {
        ServerLayer {
            loader_version: addition.loader_version,
            jar: addition.jar,
            jvm_args: self.arguments(addition.jvm_args),
            java_major: addition.java_major,
        }
    }

    fn arguments(&self, arguments: Vec<Argument>) -> Vec<String> {
        arguments
            .into_iter()
            .flat_map(|argument| match argument {
                Argument::Plain(value) => vec![value],
                Argument::Ruled(ruled) if rules::allow(&ruled.rules, self.target) => ruled.value,
                Argument::Ruled(_) => Vec::new(),
            })
            .collect()
    }

    async fn sidecars(&self, pending: Vec<Pending>) -> Result<Vec<Library>, Failure> {
        let urls = sidecar_urls(&pending);
        let fetched: HashMap<String, Checksum> = stream::iter(urls)
            .map(|url| async move {
                let checksum = self.sidecar(&url).await?;
                Ok::<_, Failure>((url, checksum))
            })
            .buffer_unordered(SIDECAR_LOOKUPS)
            .try_collect()
            .await?;
        Ok(pending
            .into_iter()
            .map(|pending| {
                let mut library = pending.library;
                if let Some(checksum) = pending.sidecar.and_then(|url| fetched.get(&url)) {
                    library.artifact.checksum = Some(checksum.clone());
                }
                library
            })
            .collect())
    }

    async fn sidecar(&self, url: &str) -> Result<Checksum, Failure> {
        let unresolved = |reason: String| Unresolved::Sidecar {
            url: place(url),
            reason,
        };
        let mut response = self.policy.get(url, false).await.map_err(|error| {
            match error.downcast_ref::<Refused>() {
                Some(refused) => Failure::Unresolved(unresolved(refused.to_string())),
                None => Failure::Other(error),
            }
        })?;
        let status = response.status();
        if !status.is_success() {
            return Err(Failure::Unresolved(unresolved(format!("HTTP {status}"))));
        }
        let mut body = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|error| Failure::Other(error.into()))?
        {
            body.extend_from_slice(&chunk);
            if body.len() > SIDECAR_MAX_BYTES {
                return Err(Failure::Unresolved(unresolved("far too long".into())));
            }
        }
        parse_sidecar(&String::from_utf8_lossy(&body))
            .ok_or_else(|| Failure::Unresolved(unresolved("holds no sha1".into())))
    }
}

/// Host and path only: a repository URL may carry a token in its query.
fn place(url: &str) -> String {
    Url::parse(url)
        .map(|url| format!("{}{}", url.host_str().unwrap_or_default(), url.path()))
        .unwrap_or_default()
}

/// The sha1 a `.sha1` sidecar carries: its first word, with or without the file
/// name the `sha1sum` tool appends.
pub(super) fn parse_sidecar(body: &str) -> Option<Checksum> {
    let checksum = Checksum {
        algorithm: HashAlgorithm::Sha1,
        hex: body.split_whitespace().next()?.to_ascii_lowercase(),
    };
    checksum.is_valid().then_some(checksum)
}

fn sidecar_urls(pending: &[Pending]) -> Vec<String> {
    let mut seen = HashSet::new();
    pending
        .iter()
        .filter_map(|pending| pending.sidecar.clone())
        .filter(|url| seen.insert(url.clone()))
        .collect()
}

struct Declared {
    rules: Vec<Rule>,
    pending: Pending,
}

/// A declared library as a plain one, with the rules that gate it. A Maven
/// library has its sidecar URL noted when it needs one and names no checksum.
fn complete(spec: LibrarySpec) -> Result<Declared, Unresolved> {
    let mut library = spec.library;
    let mut sidecar = None;
    if let Some(maven) = &spec.maven {
        fill(&mut library, maven)?;
        sidecar = (maven.sha1_sidecar && library.artifact.checksum.is_none())
            .then(|| format!("{}{SIDECAR_SUFFIX}", library.artifact.url));
    }
    Ok(Declared {
        rules: spec.rules,
        pending: Pending { library, sidecar },
    })
}

fn fill(library: &mut Library, maven: &Maven) -> Result<(), Unresolved> {
    let coordinate = Coordinate::parse(&maven.coordinate)?;
    let explicit = [
        &library.name,
        &library.path,
        &library.artifact.url,
        &library.artifact.filename,
    ];
    if library.built || explicit.iter().any(|field| !field.is_empty()) {
        return Err(Unresolved::Explicit(maven.coordinate.clone()));
    }
    let repository = Url::parse(&maven.repository)
        .ok()
        .filter(|url| {
            matches!(url.scheme(), "http" | "https")
                && url.query().is_none()
                && url.fragment().is_none()
        })
        .ok_or_else(|| Unresolved::Repository(maven.coordinate.clone()))?;
    library.name = maven.coordinate.clone();
    library.path = coordinate.path();
    library.artifact.url = coordinate.url_in(repository.as_str());
    library.artifact.filename = coordinate.filename();
    Ok(())
}

#[cfg(test)]
mod tests;
