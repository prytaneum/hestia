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
use proto::net::NetworkState;
use proto::plugin::{
    api_supported, compare_versions, Outdated, OutdatedParams, OutdatedPlugin, PluginSource,
};
use proto::repository::{Build, IndexedPlugin, RepositoryIndex};
use proto::warning::WarningInfo;
use sha2::{Digest, Sha256};

use super::Engine;
use crate::checksum::hex;
use crate::net;
use crate::plugins::{
    build_catalogue, build_url, cache_index, cached_index, declared_keys, ensure_fresh, expand,
    is_official_repository, key_id_of, mirror_of, open, outdated, pick, pinned_keys,
    refuse_official_key, revocation, verify_build, Installed, Listings, Origin, Pick, PluginRecord,
    RepoError, RepositoryEntry, Source,
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
                official: is_official_repository(&self.entry.url),
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
        let keys = match &existing {
            Some(entry) => pinned_keys(&entry.key, &entry.next),
            None => {
                let declared = declared_keys(&bytes).map_err(ErrorInfo::from)?.all();
                for key in &declared {
                    refuse_official_key(key).map_err(ErrorInfo::from)?;
                }
                declared
            }
        };
        let cached = existing
            .as_ref()
            .and_then(|_| cached_index(&url))
            .map(|index| index.version);
        let index = open(&bytes, &signature, &keys, &url, cached, common::now_unix())
            .map_err(ErrorInfo::from)?;
        let source = Source::new(url.clone(), &index.key.clone(), index);
        if confirm {
            let added_unix = existing
                .as_ref()
                .map_or_else(common::now_unix, |entry| entry.added_unix);
            self.plugin_repositories()
                .save(entry_of(&source, added_unix))?;
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

    /// Signed installs are compared against the repositories pinning their
    /// key, refreshed once each; offline they are compared against the cache.
    /// Unsigned URL installs are re-downloaded only when `params.unsigned`.
    pub async fn outdated_plugins(&self, params: &OutdatedParams) -> Result<Outdated> {
        let network_down = self.network().status().state == NetworkState::Offline;
        self.outdated_given(network_down, params).await
    }

    async fn outdated_given(
        &self,
        network_down: bool,
        params: &OutdatedParams,
    ) -> Result<Outdated> {
        let synced = if network_down {
            self.local_repositories()
        } else {
            self.sync_repositories(None).await?
        };
        let unreachable = synced
            .iter()
            .filter_map(|s| s.stale.as_ref())
            .any(|reason| {
                matches!(
                    reason,
                    ErrorInfo::Offline { .. } | ErrorInfo::DownloadFailed { .. }
                )
            });
        let warnings = synced.iter().filter_map(Synced::warning).collect();
        let sources: Vec<Source> = synced
            .into_iter()
            .filter_map(|synced| synced.source)
            .collect();
        let mut plugins = outdated(&sources, &self.installed_plugins());
        if params.unsigned && !network_down {
            plugins.extend(self.outdated_unsigned().await);
        }
        Ok(Outdated {
            plugins,
            offline: network_down || unreachable,
            warnings,
        })
    }

    async fn outdated_unsigned(&self) -> Vec<OutdatedPlugin> {
        let mut found = Vec::new();
        for record in self.plugins().list() {
            let (PluginSource::Url, None, Some(url)) = (record.source, &record.signer, &record.url)
            else {
                continue;
            };
            match self.unsigned_offer(url, &record).await {
                Ok(Some(offer)) => found.push(offer),
                Ok(None) => {}
                Err(error) => {
                    tracing::warn!(plugin = %record.id, %error, "cannot check an unsigned install for updates")
                }
            }
        }
        found
    }

    async fn unsigned_offer(
        &self,
        url: &str,
        record: &PluginRecord,
    ) -> Result<Option<OutdatedPlugin>> {
        let installed = self.plugins().read_manifest(&record.id)?.version;
        let downloads = self.plugin_downloads();
        let token = downloads.fetch(url).await?;
        let inspected = downloads
            .staged(&token)
            .and_then(|staged| self.plugins().inspect(&staged.path));
        downloads.discard(&token);
        let manifest = inspected?.manifest;
        if manifest.id != record.id || !compare_versions(&manifest.version, &installed).is_gt() {
            return Ok(None);
        }
        let api_compatible = manifest.api.as_deref().is_none_or(api_supported);
        Ok(Some(OutdatedPlugin {
            id: record.id.clone(),
            installed,
            available: manifest.version.clone(),
            api_compatible,
            requires: manifest.api.clone().filter(|_| !api_compatible),
            widened: manifest.grant().widening_over(&record.grant),
            repository: String::new(),
        }))
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

    /// Downloads the newest build of an installed plugin from a repository
    /// pinning the key that signed it, or re-downloads an unsigned link
    /// install, and holds it for [`Plugins::apply_update`](crate::Plugins::apply_update).
    /// Returns the token.
    pub async fn stage_plugin_update(&self, id: &str) -> Result<String> {
        let record = self
            .plugins()
            .get(id)
            .ok_or_else(|| ErrorInfo::PluginNotFound { id: id.to_string() })?;
        match (
            record.signer.as_deref(),
            record.source,
            record.url.as_deref(),
        ) {
            (Some(signer), _, _) => {
                let sources: Vec<Source> = self
                    .sync_repositories(None)
                    .await?
                    .into_iter()
                    .filter_map(|synced| synced.source)
                    .collect();
                let url = mirror_of(&sources, id, signer)
                    .ok_or_else(|| ErrorInfo::PluginBuildUnavailable {
                        id: id.to_string(),
                        detail: "no repository you added lists it any more".to_string(),
                    })?
                    .url
                    .clone();
                self.stage_repository_build(&url, id).await
            }
            (None, PluginSource::Url, Some(url)) => self.plugin_downloads().fetch(url).await,
            _ => Err(ErrorInfo::PluginInvalid {
                detail: "only a repository or link install can be updated".to_string(),
            }
            .into()),
        }
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
        ensure_fresh(&index, &entry.url, common::now_unix()).map_err(ErrorInfo::from)?;
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
        let url = build_url(&entry.url, &build.file).map_err(ErrorInfo::from)?;

        let downloads = self.plugin_downloads();
        let token = downloads.fetch(&url).await?;
        let (warnings, signer) = match self
            .verify_staged(&token, &entry, plugin, build, &url)
            .await
        {
            Ok(verified) => verified,
            Err(error) => {
                downloads.discard(&token);
                tracing::warn!(plugin = id, repository = repo, "repository build refused");
                return Err(error);
            }
        };
        downloads.attest(
            &token,
            Origin {
                signer,
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
        plugin: &IndexedPlugin,
        build: &Build,
        url: &str,
    ) -> Result<(Vec<WarningInfo>, String)> {
        let file = file_name(url)?;
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
            .fetch_capped(&format!("{url}.minisig"), SIGNATURE_CAP)
            .await
            .map_err(|error| ErrorInfo::RepositoryRefused {
                detail: format!("the build has no signature: {error:#}"),
            })?;
        let signature = String::from_utf8(signature).map_err(|_| ErrorInfo::RepositoryRefused {
            detail: "the signature is not text".to_string(),
        })?;
        let signer = verify_build(
            &bytes,
            &signature,
            &pinned_keys(&entry.key, &entry.next),
            &plugin.id,
            &build.version,
            &file,
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
            tracing::warn!(plugin = %plugin.id, repository = %entry.url, "index permissions differ from the package");
        }
        let warnings = disagree
            .then(|| WarningInfo::PluginPermissionsDisagree {
                plugin: plugin.id.clone(),
            })
            .into_iter()
            .collect();
        Ok((warnings, signer))
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
            let index = accept_index(
                &entry,
                &bytes,
                &signature,
                cached.as_ref().map(|index| index.version),
                common::now_unix(),
            )
            .map_err(ErrorInfo::from)?;
            Ok::<_, anyhow::Error>(index)
        }
        .await;
        match fetched {
            Ok(index) => {
                let key = index.key.clone();
                let source = Source::new(entry.url.clone(), &key, index);
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
        let latest = entry_of(source, entry.added_unix);
        if latest != entry {
            if let Err(error) = self.plugin_repositories().save(latest.clone()) {
                tracing::warn!(url = %entry.url, %error, "plugin repository snapshot not saved");
            }
        }
        latest
    }

    /// Installed plugins whose own repository's cached index revokes the
    /// installed version, as (id, reason). Blocking; no network.
    pub fn revoked_installs(&self) -> Vec<(String, String)> {
        let plugins = self.plugins();
        plugins
            .list()
            .into_iter()
            .filter_map(|record| {
                let (signer, url) = (record.signer.as_deref()?, record.repository.as_deref()?);
                let index = cached_index(url)?;
                let version = plugins.read_manifest(&record.id).ok()?.version;
                let revoked = revocation(&index, signer, &record.id, &version)?;
                Some((record.id.clone(), revoked.reason.clone()))
            })
            .collect()
    }

    pub(crate) fn installed_plugins(&self) -> Vec<Installed> {
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
                    grant: record.grant,
                })
            })
            .collect()
    }
}

/// Verifies a fetched index against the repository's pinned keys and caches it
/// only when every check passed: a refused index never replaces the cache.
fn accept_index(
    entry: &RepositoryEntry,
    bytes: &[u8],
    signature: &str,
    cached_version: Option<u64>,
    now_unix: i64,
) -> Result<RepositoryIndex, RepoError> {
    let keys = pinned_keys(&entry.key, &entry.next);
    let index = open(
        bytes,
        signature,
        &keys,
        &entry.url,
        cached_version,
        now_unix,
    )?;
    cache_index(&entry.url, bytes);
    Ok(index)
}

/// The entry a verified index pins: the key that signed it and its spare, so an
/// index signed by the spare moves the pins on.
fn entry_of(source: &Source, added_unix: i64) -> RepositoryEntry {
    RepositoryEntry {
        url: source.url.clone(),
        key: source.index.key.clone(),
        next: source.index.next.clone(),
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
            | ErrorInfo::RepositoryExpired { .. }
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
    use proto::repository::INDEX_LIFETIME_SECONDS;

    use super::*;
    use crate::plugins::seed_official_with;
    use crate::plugins::testkit::{sample_index, sign_index, Signer, SAMPLE_NOW, SAMPLE_VERSION};

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
            .add_repository("http://127.0.0.1:1/plugins.json", false)
            .await
            .unwrap_err();

        assert!(matches!(
            crate::error_info(error),
            ErrorInfo::RepositoryInvalid { .. }
        ));
        assert_eq!(engine.repositories().len(), 1, "only the official one");
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

    #[test]
    fn the_official_repository_cannot_be_removed() {
        let home = tempfile::tempdir().unwrap();
        let engine = Engine::new(Some(home.path()));
        let [official] = engine.repositories().try_into().unwrap();

        let error = engine.remove_repository(&official.url).unwrap_err();

        assert!(official.official);
        assert!(matches!(
            crate::error_info(error),
            ErrorInfo::RepositoryOfficial { .. }
        ));
        assert_eq!(engine.repositories().len(), 1);
    }

    #[test]
    fn an_index_signed_by_either_pinned_official_key_is_accepted() {
        cache_home();
        let (current, spare) = (Signer::new(), Signer::new());
        let mut entries = Vec::new();
        seed_official_with(&mut entries, &[&current.key(), &spare.key()]);
        let official = entries.pop().unwrap();
        let url = official.url.clone();

        for signer in [&current, &spare] {
            let bytes = sample_index(&signer.key(), |_| {});
            let signature = sign_index(signer, &bytes);
            accept_index(&official, &bytes, &signature, None, SAMPLE_NOW).unwrap();
        }
        let stranger = Signer::new();
        let bytes = sample_index(&stranger.key(), |_| {});
        let signature = sign_index(&stranger, &bytes);
        assert!(accept_index(&official, &bytes, &signature, None, SAMPLE_NOW).is_err());
        assert_eq!(official.url, url);
    }

    fn entry_for(url: &str, signer: &Signer, spare: Option<&Signer>) -> RepositoryEntry {
        RepositoryEntry {
            url: url.to_string(),
            key: signer.key(),
            next: spare.map(Signer::key).unwrap_or_default(),
            ..RepositoryEntry::default()
        }
    }

    fn cache_home() {
        let dir = tempfile::tempdir().unwrap().keep();
        crate::net::store::set_root(dir);
    }

    #[test]
    fn a_refused_index_never_replaces_the_cache() {
        cache_home();
        let url = "https://refused.example/plugins.json";
        let (signer, other) = (Signer::new(), Signer::new());
        let entry = entry_for(url, &signer, None);
        let good = sample_index(&signer.key(), |_| {});
        let signature = sign_index(&signer, &good);
        accept_index(&entry, &good, &signature, None, SAMPLE_NOW).unwrap();
        assert_eq!(cached_index(url).unwrap().name, "hestia-plugins-sample");

        let renamed = |index: &mut RepositoryIndex| index.name = "replaced".to_string();
        let newer = sample_index(&signer.key(), |index| {
            renamed(index);
            index.version += 10;
            index.expires += 10;
        });
        let older = sample_index(&signer.key(), |index| {
            renamed(index);
            index.version -= 10;
            index.expires -= 10;
        });
        let bad_signature = sign_index(&other, &newer);
        let expired_at = SAMPLE_NOW + INDEX_LIFETIME_SECONDS;
        let refusals = [
            (
                accept_index(
                    &entry,
                    &newer,
                    &bad_signature,
                    Some(SAMPLE_VERSION),
                    SAMPLE_NOW,
                ),
                "a bad signature",
            ),
            (
                accept_index(
                    &entry,
                    &older,
                    &sign_index(&signer, &older),
                    Some(SAMPLE_VERSION),
                    SAMPLE_NOW,
                ),
                "a lower version",
            ),
            (
                accept_index(
                    &entry,
                    &newer,
                    &sign_index(&signer, &newer),
                    Some(SAMPLE_VERSION),
                    expired_at + 10,
                ),
                "an expired index",
            ),
        ];

        for (result, what) in refusals {
            assert!(result.is_err(), "{what} is refused");
            assert_eq!(
                cached_index(url).unwrap().name,
                "hestia-plugins-sample",
                "{what} leaves the cache alone"
            );
        }
    }

    #[test]
    fn an_index_signed_by_the_pinned_spare_verifies_and_moves_the_pins() {
        cache_home();
        let url = "https://rotating.example/plugins.json";
        let (current, spare, next_spare) = (Signer::new(), Signer::new(), Signer::new());
        let entry = entry_for(url, &current, Some(&spare));
        let bytes = sample_index(&spare.key(), |index| index.next = next_spare.key());

        let index = accept_index(
            &entry,
            &bytes,
            &sign_index(&spare, &bytes),
            None,
            SAMPLE_NOW,
        )
        .unwrap();

        let source = Source::new(url.to_string(), &index.key.clone(), index);
        let moved = entry_of(&source, 0);
        assert_eq!(moved.key, spare.key());
        assert_eq!(moved.next, next_spare.key());
        assert!(accept_index(
            &entry_for(url, &current, None),
            &bytes,
            &sign_index(&spare, &bytes),
            None,
            SAMPLE_NOW
        )
        .is_err());
    }

    #[test]
    fn an_installed_version_the_index_revokes_is_reported_with_its_reason() {
        cache_home();
        let url = "https://revoking.example/plugins.json";
        let home = tempfile::tempdir().unwrap();
        let engine = Engine::new(Some(home.path()));
        let signer = Signer::new();
        let signer_id = crate::plugins::key_id_of(&signer.key()).unwrap();
        let entry = entry_for(url, &signer, None);
        let bytes = sample_index(&signer.key(), |index| {
            index.revoked = vec![proto::repository::Revocation {
                id: "aa".to_string(),
                versions: vec!["0.1.0".to_string()],
                reason: "leaks tokens".to_string(),
            }];
        });
        accept_index(
            &entry,
            &bytes,
            &sign_index(&signer, &bytes),
            None,
            SAMPLE_NOW,
        )
        .unwrap();
        let install = |id: &str, version: &str, signer: Option<&str>| {
            let record = crate::plugins::PluginRecord {
                id: id.to_string(),
                signer: signer.map(str::to_string),
                repository: Some(url.to_string()),
                enabled: true,
                ..crate::plugins::PluginRecord::default()
            };
            engine.plugins().save(&record).unwrap();
            let package = engine.plugins().package_dir(id).unwrap();
            std::fs::create_dir_all(&package).unwrap();
            std::fs::write(
                package.join("plugin.toml"),
                format!("id = \"{id}\"\nversion = \"{version}\"\n"),
            )
            .unwrap();
        };
        install("aa", "0.1.0", Some(&signer_id));
        install("bb", "0.1.0", Some(&signer_id));
        install("cc", "0.1.0", None);

        assert_eq!(
            engine.revoked_installs(),
            [("aa".to_string(), "leaks tokens".to_string())]
        );
    }

    #[tokio::test]
    async fn offline_answers_from_the_cached_index_and_says_so() {
        cache_home();
        let url = "https://offline.example/plugins.json";
        let home = tempfile::tempdir().unwrap();
        let engine = Engine::new(Some(home.path()));
        let signer = Signer::new();
        let entry = entry_for(url, &signer, None);
        let bytes = sample_index(&signer.key(), |_| {});
        accept_index(
            &entry,
            &bytes,
            &sign_index(&signer, &bytes),
            None,
            SAMPLE_NOW,
        )
        .unwrap();
        engine.plugin_repositories().save(entry).unwrap();
        let record = crate::plugins::PluginRecord {
            id: "aa".to_string(),
            signer: Some(crate::plugins::key_id_of(&signer.key()).unwrap()),
            repository: Some(url.to_string()),
            ..crate::plugins::PluginRecord::default()
        };
        engine.plugins().save(&record).unwrap();
        let package = engine.plugins().package_dir("aa").unwrap();
        std::fs::create_dir_all(&package).unwrap();
        std::fs::write(
            package.join("plugin.toml"),
            "id = \"aa\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();

        let answer = engine
            .outdated_given(true, &OutdatedParams::default())
            .await
            .unwrap();
        assert!(answer.offline);
        assert_eq!(answer.plugins.len(), 1);
        assert_eq!(answer.plugins[0].available, "0.2.0");
    }
}
