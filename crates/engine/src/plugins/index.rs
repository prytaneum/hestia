//! Where a repository's index lives and what an index may contain. Pure: the
//! caller fetches the bytes.

use std::collections::HashSet;

use proto::plugin::is_plugin_id;
use proto::repository::{IndexedPlugin, RepositoryIndex, INDEX_FILE, INDEX_SCHEMA_VERSION};
use serde::Deserialize;

use super::repos::{verify_index, RepoError};

const LABEL_LENGTH: std::ops::RangeInclusive<usize> = 1..=4;
const SHA256_HEX: usize = 64;

/// The index URL for what a user typed: `owner/repo`, `github.com/owner/repo`
/// or an https URL. A GitHub repository page maps to the fixed release asset
/// the repository shape guarantees; any other https URL is the index itself.
pub fn expand(input: &str) -> Result<String, RepoError> {
    let input = input.trim().trim_end_matches('/');
    if input.is_empty() {
        return Err(malformed("give an index URL or owner/repo"));
    }
    if input.contains("://") {
        return expand_url(input);
    }
    let path = input.strip_prefix("github.com/").unwrap_or(input);
    github_index(path)
        .ok_or_else(|| malformed("not an https URL, `github.com/owner/repo` or `owner/repo`"))
}

fn expand_url(input: &str) -> Result<String, RepoError> {
    let url = reqwest::Url::parse(input).map_err(|_| malformed("not a valid URL"))?;
    if !is_https(&url) {
        return Err(malformed(
            "only https addresses without credentials are accepted",
        ));
    }
    if url.host_str() == Some("github.com") {
        if let Some(index) = github_index(url.path().trim_matches('/')) {
            return Ok(index);
        }
    }
    Ok(url.into())
}

fn github_index(path: &str) -> Option<String> {
    let mut parts = path.split('/');
    let (owner, repo) = (parts.next()?, parts.next()?);
    let repo = repo.strip_suffix(".git").unwrap_or(repo);
    (parts.next().is_none() && is_repo_segment(owner) && is_repo_segment(repo)).then(|| {
        format!("https://github.com/{owner}/{repo}/releases/download/hestia-index/{INDEX_FILE}")
    })
}

fn is_repo_segment(segment: &str) -> bool {
    !segment.is_empty()
        && !segment.starts_with('.')
        && segment
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

/// The key an index declares, read before anything is verified. It only tells
/// the caller which key to verify against and show to the user: trusting it is
/// the user's decision at add time, and a refresh uses the pinned key instead.
pub fn declared_key(bytes: &[u8]) -> Result<String, RepoError> {
    #[derive(Deserialize)]
    struct Declared {
        key: String,
    }
    serde_json::from_slice::<Declared>(bytes)
        .map(|declared| declared.key)
        .map_err(|_| malformed("it is not a Hestia index"))
}

/// Verifies the index against `pinned_key`, then checks its contents.
pub fn open(
    bytes: &[u8],
    signature: &str,
    pinned_key: &str,
    cached_version: Option<u64>,
) -> Result<RepositoryIndex, RepoError> {
    let index = verify_index(bytes, signature, pinned_key, cached_version)?;
    validate(&index)?;
    Ok(index)
}

fn validate(index: &RepositoryIndex) -> Result<(), RepoError> {
    if index.schema_version != INDEX_SCHEMA_VERSION {
        return Err(malformed(format!(
            "schema {} is not supported (this Hestia reads {INDEX_SCHEMA_VERSION})",
            index.schema_version
        )));
    }
    if !LABEL_LENGTH.contains(&index.label.chars().count()) {
        return Err(malformed("the label must be 1 to 4 characters"));
    }
    if !index.website.is_empty() && !reqwest::Url::parse(&index.website).is_ok_and(|u| is_https(&u))
    {
        return Err(malformed("the website must be an https address"));
    }
    let mut seen = HashSet::new();
    for plugin in &index.plugins {
        validate_plugin(plugin)?;
        if !seen.insert(plugin.id.as_str()) {
            return Err(malformed(format!("plugin `{}` is listed twice", plugin.id)));
        }
    }
    Ok(())
}

fn validate_plugin(plugin: &IndexedPlugin) -> Result<(), RepoError> {
    if !is_plugin_id(&plugin.id) {
        return Err(malformed(format!("`{}` is not a plugin id", plugin.id)));
    }
    for build in &plugin.builds {
        if !reqwest::Url::parse(&build.url).is_ok_and(|url| is_https(&url)) {
            return Err(malformed(format!(
                "`{}` {} has a build that is not an https address",
                plugin.id, build.version
            )));
        }
        let hex = build.sha256.len() == SHA256_HEX
            && build.sha256.bytes().all(|byte| byte.is_ascii_hexdigit());
        if !hex {
            return Err(malformed(format!(
                "`{}` {} has no valid sha256",
                plugin.id, build.version
            )));
        }
    }
    Ok(())
}

fn is_https(url: &reqwest::Url) -> bool {
    url.scheme() == "https"
        && url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none()
}

/// An https URL for a plugin's icon, which the index gives as a path inside
/// the repository. Only GitHub has a fixed place to serve a file at a commit.
pub fn icon_url(index: &RepositoryIndex, plugin: &IndexedPlugin) -> Option<String> {
    let icon = plugin.icon.as_deref()?;
    let inside = !icon.contains(['\\', ':'])
        && icon
            .split('/')
            .all(|segment| !matches!(segment, "" | "." | ".."));
    let source = reqwest::Url::parse(&index.repository).ok()?;
    if !inside || source.host_str() != Some("github.com") || index.commit.is_empty() {
        return None;
    }
    let mut parts = source.path_segments()?;
    let (owner, repo) = (parts.next()?, parts.next()?);
    let repo = repo.strip_suffix(".git").unwrap_or(repo);
    (is_repo_segment(owner) && is_repo_segment(repo)).then(|| {
        format!(
            "https://raw.githubusercontent.com/{owner}/{repo}/{}/{icon}",
            index.commit
        )
    })
}

fn malformed(detail: impl std::fmt::Display) -> RepoError {
    RepoError::Malformed(detail.to_string())
}

#[cfg(test)]
mod tests {
    use proto::repository::index_comment;

    use super::*;
    use crate::plugins::testkit::{sample_index, Signer};

    const RELEASE: &str =
        "https://github.com/acme/tools/releases/download/hestia-index/hestia-index.json";

    #[test]
    fn a_repository_name_expands_to_the_fixed_release_asset() {
        for input in [
            "acme/tools",
            "github.com/acme/tools",
            "https://github.com/acme/tools",
            "https://github.com/acme/tools/",
            "acme/tools.git",
            RELEASE,
        ] {
            assert_eq!(expand(input).unwrap(), RELEASE, "{input}");
        }
    }

    #[test]
    fn any_other_https_index_url_is_kept_as_it_is() {
        let url = "https://example.com/hestia/hestia-index.json";

        assert_eq!(expand(url).unwrap(), url);
    }

    #[test]
    fn an_address_that_is_not_https_or_a_repository_name_is_refused() {
        for input in [
            "",
            "http://example.com/hestia-index.json",
            "ftp://example.com/x",
            "https://user:secret@example.com/hestia-index.json",
            "acme",
            "acme/tools/extra",
            "../tools",
            "acme/..",
            "file:///tmp/hestia-index.json",
        ] {
            assert!(
                matches!(expand(input), Err(RepoError::Malformed(_))),
                "{input}"
            );
        }
    }

    #[test]
    fn a_real_index_opens_with_its_own_key() {
        let signer = Signer::new();
        let bytes = sample_index(&signer.key(), |_| {});
        let signature = signer.sign(&bytes, &index_comment(3));

        assert_eq!(declared_key(&bytes).unwrap(), signer.key());
        let index = open(&bytes, &signature, &signer.key(), None).unwrap();

        assert_eq!(index.label, "SMP");
        assert_eq!(index.plugins.len(), 2);
        assert_eq!(index.plugins[0].builds.len(), 2);
    }

    fn refused(mutate: impl FnOnce(&mut RepositoryIndex)) -> RepoError {
        let signer = Signer::new();
        let bytes = sample_index(&signer.key(), mutate);
        let signature = signer.sign(&bytes, &index_comment(3));
        open(&bytes, &signature, &signer.key(), None).unwrap_err()
    }

    #[test]
    fn a_build_that_is_not_https_refuses_the_index() {
        let error = refused(|index| {
            index.plugins[0].builds[0].url = "http://example.com/aa.hplugin".to_string();
        });

        assert!(matches!(error, RepoError::Malformed(_)), "{error:?}");
    }

    #[test]
    fn a_plugin_id_that_is_not_a_slug_refuses_the_index() {
        for id in ["Aa", "a", "-aa", "aa/../bb", ""] {
            let error = refused(|index| index.plugins[0].id = id.to_string());
            assert!(matches!(error, RepoError::Malformed(_)), "{id}: {error:?}");
        }
    }

    #[test]
    fn a_duplicate_id_a_bad_label_or_a_bad_checksum_refuses_the_index() {
        let cases: [fn(&mut RepositoryIndex); 4] = [
            |index| index.plugins[1].id = "aa".to_string(),
            |index| index.label = "TOOLONG".to_string(),
            |index| index.label = String::new(),
            |index| index.plugins[0].builds[0].sha256 = "abc".to_string(),
        ];
        for mutate in cases {
            assert!(matches!(refused(mutate), RepoError::Malformed(_)));
        }
    }

    #[test]
    fn something_that_is_not_an_index_has_no_declared_key() {
        assert!(matches!(
            declared_key(b"<html>404</html>"),
            Err(RepoError::Malformed(_))
        ));
    }

    #[test]
    fn an_icon_resolves_only_to_a_pinned_github_file() {
        let index = RepositoryIndex {
            repository: "https://github.com/acme/tools".to_string(),
            commit: "abc123".to_string(),
            ..RepositoryIndex::default()
        };
        let plugin = |icon: &str| IndexedPlugin {
            icon: Some(icon.to_string()),
            ..IndexedPlugin::default()
        };

        assert_eq!(
            icon_url(&index, &plugin("plugins/aa/icon.png")).as_deref(),
            Some("https://raw.githubusercontent.com/acme/tools/abc123/plugins/aa/icon.png")
        );
        assert_eq!(icon_url(&index, &plugin("../icon.png")), None);
        assert_eq!(icon_url(&index, &plugin("/etc/passwd")), None);
        let elsewhere = RepositoryIndex {
            repository: "https://gitlab.com/acme/tools".to_string(),
            ..index
        };
        assert_eq!(icon_url(&elsewhere, &plugin("icon.png")), None);
    }
}
