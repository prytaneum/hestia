//! Plugin repositories: adding one, keeping its index fresh, merging every
//! index into one catalogue and staging a build for install. Trust follows the
//! key pinned at add time ([0097](../../../../../docs/decisions/0097-trust-follows-the-repository-key.md)).

use std::collections::BTreeSet;

use anyhow::Result;
use futures_util::future::join_all;
use proto::catalogue::{
    Catalogue, CatalogueParams, RepositoryAdded, RepositoryInfo, RepositoryRefreshed,
};
use proto::error::ErrorInfo;
use proto::repository::{Build, IndexedPlugin};
use proto::warning::WarningInfo;
use sha2::{Digest, Sha256};

use super::Engine;
use crate::checksum::hex;
use crate::net;
use crate::plugins::{
    build_catalogue, cache_index, cached_index, declared_key, expand, key_id_of, open, pick,
    refuse_official_key, verify_build, Installed, Listings, Origin, Pick, RepositoryEntry, Source,
};

const INDEX_CAP: u64 = 8 * 1024 * 1024;
const SIGNATURE_CAP: u64 = 16 * 1024;

/// A configured repository and what is known of it right now.
struct Synced {
    entry: RepositoryEntry,
    source: Option<Source>,
    /// Why the index could not be refreshed; `source` is then the cached copy.
    stale: Option<ErrorInfo>,
}

impl Synced {
    fn info(&self) -> RepositoryInfo {
        match &self.source {
            Some(source) => source.info(),
            None => RepositoryInfo {
                url: self.entry.url.clone(),
                name: self.entry.name.clone(),
                label: self.entry.label.clone(),
                website: self.entry.website.clone(),
                key_id: key_id_of(&self.entry.key).unwrap_or_default(),
                ..RepositoryInfo::default()
            },
        }
    }

    fn warning(&self) -> Option<WarningInfo> {
        self.stale
            .as_ref()
            .map(|reason| WarningInfo::RepositoryStale {
                repository: if self.entry.label.is_empty() {
                    self.entry.url.clone()
                } else {
                    self.entry.label.clone()
                },
                detail: reason.to_string(),
            })
    }
}

impl Engine {
    /// Fetches and verifies the index at what the user typed. Nothing is saved
    /// without `confirm`, so a front-end can show the key before it is trusted.
    pub async fn add_repository(&self, input: &str, confirm: bool) -> Result<RepositoryAdded> {
        let url = expand(input).map_err(ErrorInfo::from)?;
        let existing = self.plugin_repositories().get(&url);
        let (bytes, signature) = self.fetch_signed(&url).await.map_err(not_an_index)?;
        let key = match &existing {
            Some(entry) => entry.key.clone(),
            None => {
                let declared = declared_key(&bytes).map_err(ErrorInfo::from)?;
                refuse_official_key(&declared).map_err(ErrorInfo::from)?;
                declared
            }
        };
        let cached = existing
            .as_ref()
            .and_then(|_| cached_index(&url))
            .map(|index| index.version);
        let index = open(&bytes, &signature, &key, cached).map_err(ErrorInfo::from)?;
        let source = Source::new(url.clone(), &key, index);
        if confirm {
            let added_unix = existing
                .as_ref()
                .map_or_else(common::now_unix, |entry| entry.added_unix);
            self.plugin_repositories()
                .save(entry_of(&source, &key, added_unix))?;
            cache_index(&url, &bytes);
            tracing::info!(url, key = %source.key_id, "plugin repository added");
        }
        Ok(RepositoryAdded {
            repository: source.info(),
            configured: existing.is_some(),
            saved: confirm,
        })
    }

    /// Plugins installed from it keep running, as obsolete.
    pub fn remove_repository(&self, url: &str) -> Result<()> {
        if !self.plugin_repositories().remove(url)? {
            return Err(ErrorInfo::RepositoryNotFound {
                reference: url.to_string(),
            }
            .into());
        }
        tracing::info!(url, "plugin repository removed");
        Ok(())
    }

    /// The repositories as last fetched; no network.
    pub fn repositories(&self) -> Vec<RepositoryInfo> {
        self.local_repositories().iter().map(Synced::info).collect()
    }

    /// Which signed plugins some configured repository still lists. Blocking.
    pub fn listings(&self) -> Listings {
        let sources: Vec<Source> = self
            .local_repositories()
            .into_iter()
            .filter_map(|synced| synced.source)
            .collect();
        Listings::of(&sources)
    }

    /// For one named repository a refused index is an error; refreshing all
    /// reports it as a warning, so one bad repository does not hide the rest.
    pub async fn refresh_repositories(&self, url: Option<&str>) -> Result<RepositoryRefreshed> {
        let synced = self.sync_repositories(url).await?;
        if url.is_some() {
            if let Some(refused) = synced
                .iter()
                .filter_map(|synced| synced.stale.clone())
                .find(is_refusal)
            {
                return Err(refused.into());
            }
        }
        Ok(RepositoryRefreshed {
            warnings: synced.iter().filter_map(Synced::warning).collect(),
            repositories: synced.iter().map(Synced::info).collect(),
        })
    }

    pub async fn catalogue(&self, params: &CatalogueParams) -> Result<Catalogue> {
        let synced = self.sync_repositories(None).await?;
        let repo = params.repo.as_deref();
        if let Some(url) = repo.filter(|url| synced.iter().all(|s| s.entry.url != *url)) {
            return Err(ErrorInfo::RepositoryNotFound {
                reference: url.to_string(),
            }
            .into());
        }
        let installed = self.installed_plugins();
        let repositories = synced.iter().map(Synced::info).collect();
        let warnings = synced.iter().filter_map(Synced::warning).collect();
        let sources: Vec<Source> = synced
            .into_iter()
            .filter_map(|synced| synced.source)
            .collect();
        Ok(Catalogue {
            plugins: build_catalogue(&sources, &installed, repo, params.query.as_deref()),
            repositories,
            warnings,
        })
    }

    /// Downloads the newest installable build of `id` from the repository at
    /// `repo`, checks its checksum and its signature against the repository's
    /// pinned key, and holds it for inspect and install. Returns the token.
    pub async fn stage_repository_build(&self, repo: &str, id: &str) -> Result<String> {
        let entry =
            self.plugin_repositories()
                .get(repo)
                .ok_or_else(|| ErrorInfo::RepositoryNotFound {
                    reference: repo.to_string(),
                })?;
        let index = cached_index(&entry.url).ok_or_else(|| ErrorInfo::RepositoryInvalid {
            detail: "its index has not been fetched yet; refresh the repository".to_string(),
        })?;
        let source = Source::new(entry.url.clone(), &entry.key, index);
        let unavailable = |detail: String| ErrorInfo::PluginBuildUnavailable {
            id: id.to_string(),
            detail,
        };
        let plugin = source
            .index
            .plugins
            .iter()
            .find(|plugin| plugin.id == id)
            .ok_or_else(|| unavailable("the repository does not list it".to_string()))?;
        let choice = pick(&source, plugin);
        let build = choice
            .build()
            .ok_or_else(|| unavailable(describe(&choice, plugin)))?;
        let file = file_name(&build.url)?;

        let downloads = self.plugin_downloads();
        let token = downloads.fetch(&build.url).await?;
        let warnings = match self
            .verify_staged(&token, &entry, &source, plugin, build, &file)
            .await
        {
            Ok(warnings) => warnings,
            Err(error) => {
                downloads.discard(&token);
                tracing::warn!(plugin = id, repository = repo, "repository build refused");
                return Err(error);
            }
        };
        downloads.attest(
            &token,
            Origin {
                signer: source.key_id.clone(),
                repository: entry.url.clone(),
            },
            warnings,
        );
        Ok(token)
    }

    async fn verify_staged(
        &self,
        token: &str,
        entry: &RepositoryEntry,
        source: &Source,
        plugin: &IndexedPlugin,
        build: &Build,
        file: &str,
    ) -> Result<Vec<WarningInfo>> {
        let downloads = self.plugin_downloads();
        let staged = downloads.staged(token)?;
        let bytes = tokio::fs::read(&staged.path).await?;
        if !hex(&Sha256::digest(&bytes)).eq_ignore_ascii_case(&build.sha256) {
            return Err(ErrorInfo::RepositoryRefused {
                detail: "the download does not match the checksum the repository lists".to_string(),
            }
            .into());
        }
        let signature = downloads
            .fetch_capped(&format!("{}.minisig", build.url), SIGNATURE_CAP)
            .await
            .map_err(|error| ErrorInfo::RepositoryRefused {
                detail: format!("the build has no signature: {error:#}"),
            })?;
        let signature = String::from_utf8(signature).map_err(|_| ErrorInfo::RepositoryRefused {
            detail: "the signature is not text".to_string(),
        })?;
        verify_build(
            &bytes,
            &signature,
            &entry.key,
            &plugin.id,
            &build.version,
            file,
        )
        .map_err(ErrorInfo::from)?;

        let manifest = self.plugins().inspect(&staged.path)?.manifest;
        if manifest.id != plugin.id || manifest.version != build.version {
            return Err(ErrorInfo::PluginInvalid {
                detail: format!(
                    "the package is {} {}, not {} {}",
                    manifest.id, manifest.version, plugin.id, build.version
                ),
            }
            .into());
        }
        let listed: BTreeSet<_> = plugin.permissions.iter().collect();
        let asked: BTreeSet<_> = manifest.permissions.iter().collect();
        let disagree = build.version == plugin.version && listed != asked;
        if disagree {
            tracing::warn!(plugin = %plugin.id, repository = %source.url, "index permissions differ from the package");
        }
        Ok(disagree
            .then(|| WarningInfo::PluginPermissionsDisagree {
                plugin: plugin.id.clone(),
            })
            .into_iter()
            .collect())
    }

    async fn fetch_signed(&self, url: &str) -> Result<(Vec<u8>, String)> {
        let downloads = self.plugin_downloads();
        let bytes = downloads.fetch_capped(url, INDEX_CAP).await?;
        let signature = downloads
            .fetch_capped(&format!("{url}.minisig"), SIGNATURE_CAP)
            .await?;
        let signature = String::from_utf8(signature).map_err(|_| ErrorInfo::RepositoryInvalid {
            detail: "its signature is not text".to_string(),
        })?;
        Ok((bytes, signature))
    }

    fn local_repositories(&self) -> Vec<Synced> {
        self.plugin_repositories()
            .list()
            .into_iter()
            .map(|entry| Synced {
                source: cached_index(&entry.url)
                    .map(|index| Source::new(entry.url.clone(), &entry.key, index)),
                entry,
                stale: None,
            })
            .collect()
    }

    async fn sync_repositories(&self, only: Option<&str>) -> Result<Vec<Synced>> {
        let mut entries = self.plugin_repositories().list();
        if let Some(url) = only {
            entries.retain(|entry| entry.url == url);
            if entries.is_empty() {
                return Err(ErrorInfo::RepositoryNotFound {
                    reference: url.to_string(),
                }
                .into());
            }
        }
        Ok(join_all(entries.into_iter().map(|entry| self.sync_repository(entry))).await)
    }

    async fn sync_repository(&self, entry: RepositoryEntry) -> Synced {
        let cached = cached_index(&entry.url);
        let fetched = async {
            let (bytes, signature) = self.fetch_signed(&entry.url).await?;
            let index = open(
                &bytes,
                &signature,
                &entry.key,
                cached.as_ref().map(|index| index.version),
            )
            .map_err(ErrorInfo::from)?;
            Ok::<_, anyhow::Error>((bytes, index))
        }
        .await;
        match fetched {
            Ok((bytes, index)) => {
                cache_index(&entry.url, &bytes);
                let source = Source::new(entry.url.clone(), &entry.key, index);
                let entry = self.remember(entry, &source);
                Synced {
                    entry,
                    source: Some(source),
                    stale: None,
                }
            }
            Err(error) => {
                let reason = crate::error_info(error);
                tracing::warn!(url = %entry.url, %reason, "plugin repository not refreshed");
                Synced {
                    source: cached.map(|index| Source::new(entry.url.clone(), &entry.key, index)),
                    entry,
                    stale: Some(reason),
                }
            }
        }
    }

    fn remember(&self, entry: RepositoryEntry, source: &Source) -> RepositoryEntry {
        let latest = entry_of(source, &entry.key, entry.added_unix);
        if latest != entry {
            if let Err(error) = self.plugin_repositories().save(latest.clone()) {
                tracing::warn!(url = %entry.url, %error, "plugin repository snapshot not saved");
            }
        }
        latest
    }

    fn installed_plugins(&self) -> Vec<Installed> {
        let plugins = self.plugins();
        plugins
            .list()
            .into_iter()
            .filter_map(|record| {
                let version = plugins.read_manifest(&record.id).ok()?.version;
                Some(Installed {
                    id: record.id,
                    version,
                    signer: record.signer,
                })
            })
            .collect()
    }
}

fn entry_of(source: &Source, key: &str, added_unix: i64) -> RepositoryEntry {
    RepositoryEntry {
        url: source.url.clone(),
        key: key.to_string(),
        name: source.index.name.clone(),
        label: source.index.label.clone(),
        website: source.index.website.clone(),
        added_unix,
    }
}

fn is_refusal(error: &ErrorInfo) -> bool {
    matches!(
        error,
        ErrorInfo::RepositoryInvalid { .. }
            | ErrorInfo::RepositoryKeyChanged { .. }
            | ErrorInfo::RepositoryRefused { .. }
    )
}

/// Only adding treats an unanswered address as not being a repository: a
/// refresh keeps the cached copy instead.
fn not_an_index(error: anyhow::Error) -> anyhow::Error {
    if net::is_offline(&error) {
        return error;
    }
    match error.downcast::<ErrorInfo>() {
        Ok(info @ ErrorInfo::RepositoryInvalid { .. }) => info.into(),
        Ok(ErrorInfo::DownloadFailed { detail } | ErrorInfo::Upstream { detail, .. }) => {
            ErrorInfo::RepositoryInvalid {
                detail: format!("no signed index there ({detail})"),
            }
            .into()
        }
        Ok(info) => info.into(),
        Err(error) => error,
    }
}

fn describe(choice: &Pick<'_>, plugin: &IndexedPlugin) -> String {
    match choice {
        Pick::Incompatible { requires, .. } => format!(
            "version {} needs plugin API {}",
            plugin.version,
            requires.unwrap_or("another version")
        ),
        _ => format!("version {} has no build yet", plugin.version),
    }
}

fn file_name(url: &str) -> Result<String> {
    reqwest::Url::parse(url)
        .ok()
        .and_then(|url| url.path_segments()?.next_back().map(str::to_string))
        .filter(|name| !name.is_empty())
        .ok_or_else(|| {
            ErrorInfo::RepositoryInvalid {
                detail: format!("`{url}` names no file"),
            }
            .into()
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_build_file_name_is_the_last_path_segment() {
        assert_eq!(
            file_name("https://example.com/r/aa-v1/aa-1.0.0.hplugin").unwrap(),
            "aa-1.0.0.hplugin"
        );
        assert!(file_name("https://example.com/").is_err());
        assert!(file_name("not a url").is_err());
    }

    #[test]
    fn only_a_refused_index_is_a_refusal() {
        assert!(is_refusal(&ErrorInfo::RepositoryRefused {
            detail: String::new()
        }));
        assert!(is_refusal(&ErrorInfo::RepositoryKeyChanged {
            pinned: String::new(),
            declared: String::new()
        }));
        assert!(!is_refusal(&ErrorInfo::Offline { service: None }));
        assert!(!is_refusal(&ErrorInfo::DownloadFailed {
            detail: String::new()
        }));
    }

    #[test]
    fn adding_treats_a_missing_index_as_not_a_repository_but_keeps_offline() {
        let missing: anyhow::Error = ErrorInfo::DownloadFailed {
            detail: "HTTP 404".to_string(),
        }
        .into();
        let offline: anyhow::Error = ErrorInfo::Offline { service: None }.into();

        assert!(matches!(
            crate::error_info(not_an_index(missing)),
            ErrorInfo::RepositoryInvalid { .. }
        ));
        assert!(matches!(
            crate::error_info(not_an_index(offline)),
            ErrorInfo::Offline { .. }
        ));
    }

    #[tokio::test]
    async fn an_address_that_is_not_https_never_reaches_the_network() {
        let home = tempfile::tempdir().unwrap();
        let engine = Engine::new(Some(home.path()));

        let error = engine
            .add_repository("http://127.0.0.1:1/hestia-index.json", false)
            .await
            .unwrap_err();

        assert!(matches!(
            crate::error_info(error),
            ErrorInfo::RepositoryInvalid { .. }
        ));
        assert!(engine.repositories().is_empty());
    }

    #[test]
    fn removing_an_unknown_repository_is_not_found() {
        let home = tempfile::tempdir().unwrap();
        let engine = Engine::new(Some(home.path()));

        let error = engine
            .remove_repository("https://nowhere.example/i.json")
            .unwrap_err();

        assert!(matches!(
            crate::error_info(error),
            ErrorInfo::RepositoryNotFound { .. }
        ));
    }
}
