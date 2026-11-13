//! Which signed installs a repository now offers a newer build of, and applying
//! one. The check is pure, like the catalogue it reads through: the caller
//! supplies the indexes and what is installed.

use std::future::Future;

use anyhow::Result;
use proto::net::NetworkState;
use proto::plugin::{compare_versions, Grant, Outdated, OutdatedPlugin, PluginInstallPhase};
use proto::repository::IndexedPlugin;

use super::catalogue::{best, pick, Installed, Pick, Source};
use super::official::is_official_signer_in;
use super::remote::Staged;
use super::store::{PluginRecord, Plugins};
use crate::cancel::Cancel;
use crate::Engine;

/// The listings of `id` by repositories pinning `signer`, mirrors included.
pub(crate) fn listings_of<'a>(
    sources: &'a [Source],
    id: &str,
    signer: &str,
) -> Vec<(&'a Source, &'a IndexedPlugin)> {
    sources
        .iter()
        .filter(|source| source.key_id == signer)
        .flat_map(|source| {
            source
                .index
                .plugins
                .iter()
                .filter(|plugin| plugin.id == id)
                .map(move |plugin| (source, plugin))
        })
        .collect()
}

/// The repository to take the newest build of `id` from, when `signer` signed
/// the install.
pub(crate) fn mirror_of<'a>(sources: &'a [Source], id: &str, signer: &str) -> Option<&'a Source> {
    best(&listings_of(sources, id, signer)).map(|(source, _)| source)
}

/// A verified package for an installed plugin, and what the user approved.
pub struct UpdateRequest<'a> {
    pub id: &'a str,
    pub staged: &'a Staged,
    pub approve: &'a str,
}

pub enum UpdateOutcome {
    Applied {
        record: PluginRecord,
        from: String,
    },
    /// The new version failed to activate and the previous one is installed again.
    RolledBack {
        record: PluginRecord,
        version: String,
        error: String,
    },
}

impl Plugins {
    /// Blocking swap, then `activate` on the new record when the plugin is
    /// enabled. If activation fails, the previous package and record come back
    /// and the failure is reported, not raised. Never swaps back to an older
    /// version for any other reason.
    pub async fn apply_update<Fut>(
        &self,
        request: UpdateRequest<'_>,
        cancel: &Cancel,
        on_phase: &(dyn Fn(PluginInstallPhase) + Sync),
        validate: impl FnOnce(&[u8]) -> Result<()>,
        activate: impl FnOnce(PluginRecord) -> Fut,
    ) -> Result<UpdateOutcome>
    where
        Fut: Future<Output = Result<()>>,
    {
        let from = self.read_manifest(request.id)?.version;
        let updated = self.update(
            request.staged,
            request.id,
            request.approve,
            cancel,
            on_phase,
            validate,
        )?;
        let version = self.read_manifest(request.id)?.version;
        if !updated.record.enabled {
            tracing::info!(plugin = %request.id, %from, %version, "plugin updated");
            return Ok(UpdateOutcome::Applied {
                record: updated.record,
                from,
            });
        }
        match activate(updated.record.clone()).await {
            Ok(()) => {
                tracing::info!(plugin = %request.id, %from, %version, "plugin updated");
                Ok(UpdateOutcome::Applied {
                    record: updated.record,
                    from,
                })
            }
            Err(error) => {
                let error = format!("{error:#}");
                tracing::warn!(plugin = %request.id, %version, %error, "plugin update failed to load, rolling back");
                let record = self.rollback(request.id)?;
                Ok(UpdateOutcome::RolledBack {
                    record,
                    version,
                    error,
                })
            }
        }
    }
}

/// The signed installs with a newer version on offer, by id. Only repositories
/// pinning the key that signed the install count, so another key listing the
/// same id is ignored, and an install nobody lists any more (obsolete) is left out.
pub fn outdated(sources: &[Source], installed: &[Installed]) -> Vec<OutdatedPlugin> {
    let mut found: Vec<OutdatedPlugin> = installed
        .iter()
        .filter_map(|record| {
            let signer = record.signer.as_deref()?;
            let (source, plugin) = best(&listings_of(sources, &record.id, signer))?;
            offer(source, plugin, record)
        })
        .collect();
    found.sort_by(|a, b| a.id.cmp(&b.id));
    found
}

fn offer(source: &Source, plugin: &IndexedPlugin, record: &Installed) -> Option<OutdatedPlugin> {
    let newer = |version: &str| compare_versions(version, &record.version).is_gt();
    let choice = pick(source, plugin);
    let (available, api_compatible, requires) = match &choice {
        Pick::Prebuilt(build) => (build.version.as_str(), true, None),
        Pick::Unreleased(older) => (older.as_ref()?.version.as_str(), true, None),
        Pick::Incompatible { older, requires } => {
            match older
                .map(|build| build.version.as_str())
                .filter(|v| newer(v))
            {
                Some(runnable) => (runnable, true, requires.map(str::to_string)),
                None => (plugin.version.as_str(), false, requires.map(str::to_string)),
            }
        }
    };
    newer(available).then(|| OutdatedPlugin {
        id: record.id.clone(),
        installed: record.version.clone(),
        available: available.to_string(),
        api_compatible,
        requires,
        widened: Grant {
            permissions: plugin
                .permissions
                .iter()
                .copied()
                .filter(|permission| !record.grant.permissions.contains(permission))
                .collect(),
            ..Grant::default()
        },
        repository: source.url.clone(),
    })
}

/// The offers an unattended update may apply: from an official signer, runnable
/// here, and asking for nothing beyond what is granted. The offer's `widened`
/// only knows the index's permission list, so the full grant is checked again
/// when the package is staged.
pub fn auto_updatable(
    outdated: &Outdated,
    installed: &[Installed],
    official_keys: &[&str],
) -> Vec<OutdatedPlugin> {
    if outdated.offline {
        return Vec::new();
    }
    outdated
        .plugins
        .iter()
        .filter(|offer| offer.api_compatible && offer.widened == Grant::default())
        .filter(|offer| {
            installed
                .iter()
                .find(|record| record.id == offer.id)
                .is_some_and(|record| {
                    is_official_signer_in(record.signer.as_deref(), official_keys)
                })
        })
        .cloned()
        .collect()
}

impl Engine {
    /// What the daemon may apply unattended after a check: nothing when
    /// `plugins.auto-update` is off or the network is down.
    pub fn auto_update_candidates(&self, outdated: &Outdated) -> Vec<OutdatedPlugin> {
        if !self.config().settings().plugins.auto_update {
            return Vec::new();
        }
        if self.network().status().state == NetworkState::Offline {
            return Vec::new();
        }
        auto_updatable(
            outdated,
            &self.installed_plugins(),
            &common::app::official_plugin_pubkeys(),
        )
    }
}

#[cfg(test)]
mod tests {
    use proto::plugin::{Permission, API_VERSION};
    use proto::repository::{Build, RepositoryIndex};

    use super::*;
    use crate::plugins::repos::key_id_of;
    use crate::plugins::testkit::{sample_index, Signer};

    fn source(url: &str, key: &str, change: impl FnOnce(&mut RepositoryIndex)) -> Source {
        let index = serde_json::from_slice(&sample_index(key, change)).unwrap();
        Source::new(url.to_string(), key, index)
    }

    fn installed(id: &str, version: &str, key: &str) -> Installed {
        Installed {
            id: id.to_string(),
            version: version.to_string(),
            signer: Some(key_id_of(key).unwrap()),
            grant: Grant::default(),
        }
    }

    fn build(version: &str, api: &str) -> Build {
        Build {
            version: version.to_string(),
            api: Some(api.to_string()),
            file: format!("packages/aa-{version}.hplugin"),
            sha256: "0".repeat(64),
        }
    }

    #[test]
    fn a_newer_compatible_build_is_reported() {
        let key = Signer::new().key();
        let sources = [source("https://a.example/i.json", &key, |_| {})];

        let found = outdated(&sources, &[installed("aa", "0.1.0", &key)]);

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].available, "0.2.0");
        assert!(found[0].api_compatible);
        assert_eq!(found[0].repository, "https://a.example/i.json");
        assert!(outdated(&sources, &[installed("aa", "0.2.0", &key)]).is_empty());
    }

    #[test]
    fn a_newer_version_needing_a_newer_api_is_flagged_and_the_older_build_is_offered() {
        let key = Signer::new().key();
        let api = format!("^{API_VERSION}");
        let sources = [source("https://a.example/i.json", &key, |index| {
            index.plugins[0].version = "0.3.0".to_string();
            index.plugins[0].builds = vec![build("0.2.0", &api), build("0.3.0", "^9.0.0")];
        })];

        let found = outdated(&sources, &[installed("aa", "0.1.0", &key)]);
        assert_eq!(found[0].available, "0.2.0");
        assert!(found[0].api_compatible);
        assert_eq!(found[0].requires.as_deref(), Some("^9.0.0"));

        let found = outdated(&sources, &[installed("aa", "0.2.0", &key)]);
        assert_eq!(found[0].available, "0.3.0");
        assert!(!found[0].api_compatible);
    }

    #[test]
    fn a_newer_source_version_without_a_build_is_not_reported() {
        let key = Signer::new().key();
        let sources = [source("https://a.example/i.json", &key, |index| {
            index.plugins[0].version = "0.3.0".to_string();
        })];

        assert!(outdated(&sources, &[installed("aa", "0.2.0", &key)]).is_empty());
    }

    #[test]
    fn unsigned_and_obsolete_installs_are_not_reported() {
        let key = Signer::new().key();
        let sources = [source("https://a.example/i.json", &key, |_| {})];
        let unsigned = Installed {
            signer: None,
            ..installed("aa", "0.1.0", &key)
        };

        assert!(outdated(&sources, &[unsigned]).is_empty());
        assert!(outdated(&sources, &[installed("zz", "0.1.0", &key)]).is_empty());
    }

    #[test]
    fn mirrors_under_one_key_are_one_entry_with_the_newest_build() {
        let key = Signer::new().key();
        let sources = [
            source("https://a.example/i.json", &key, |index| {
                index.plugins[0].version = "0.1.5".to_string();
                index.plugins[0].builds = vec![build("0.1.5", &format!("^{API_VERSION}"))];
            }),
            source("https://b.example/i.json", &key, |_| {}),
        ];

        let found = outdated(&sources, &[installed("aa", "0.1.0", &key)]);

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].available, "0.2.0");
        assert_eq!(found[0].repository, "https://b.example/i.json");
    }

    #[test]
    fn a_repository_under_another_key_is_ignored() {
        let (key, other) = (Signer::new().key(), Signer::new().key());
        let sources = [source("https://a.example/i.json", &other, |_| {})];

        assert!(outdated(&sources, &[installed("aa", "0.1.0", &key)]).is_empty());
    }

    #[test]
    fn widened_lists_the_permissions_the_update_adds() {
        let key = Signer::new().key();
        let sources = [source("https://a.example/i.json", &key, |index| {
            index.plugins[0].permissions = vec![Permission::EntryRead, Permission::EntryWrite];
        })];
        let mut record = installed("aa", "0.1.0", &key);
        record.grant.permissions.insert(Permission::EntryRead);

        let found = outdated(&sources, &[record]);

        let widened: Vec<_> = found[0].widened.permissions.iter().collect();
        assert_eq!(widened, [&Permission::EntryWrite]);
    }

    fn offer(id: &str) -> OutdatedPlugin {
        OutdatedPlugin {
            id: id.to_string(),
            installed: "0.1.0".to_string(),
            available: "0.2.0".to_string(),
            api_compatible: true,
            ..OutdatedPlugin::default()
        }
    }

    fn outdated_of(plugins: Vec<OutdatedPlugin>) -> Outdated {
        Outdated {
            plugins,
            ..Outdated::default()
        }
    }

    #[test]
    fn an_official_update_that_asks_for_nothing_is_applied() {
        let (official, other) = (Signer::new().key(), Signer::new().key());
        let installed = [installed("aa", "0.1.0", &official)];

        let found = auto_updatable(&outdated_of(vec![offer("aa")]), &installed, &[&official]);
        assert_eq!(found.len(), 1);

        let found = auto_updatable(&outdated_of(vec![offer("aa")]), &installed, &[&other]);
        assert!(found.is_empty(), "a third-party plugin waits for the user");
    }

    #[test]
    fn a_widened_official_update_is_not_applied() {
        let official = Signer::new().key();
        let installed = [installed("aa", "0.1.0", &official)];
        let mut widened = offer("aa");
        widened.widened.permissions.insert(Permission::EntryWrite);

        let found = auto_updatable(&outdated_of(vec![widened]), &installed, &[&official]);

        assert!(found.is_empty());
    }

    #[test]
    fn an_update_this_build_cannot_run_or_learned_offline_is_not_applied() {
        let official = Signer::new().key();
        let installed = [installed("aa", "0.1.0", &official)];
        let mut incompatible = offer("aa");
        incompatible.api_compatible = false;
        let mut offline = outdated_of(vec![offer("aa")]);
        offline.offline = true;

        assert!(
            auto_updatable(&outdated_of(vec![incompatible]), &installed, &[&official]).is_empty()
        );
        assert!(auto_updatable(&offline, &installed, &[&official]).is_empty());
    }

    #[test]
    fn candidates_follow_the_config_switch() {
        let home = tempfile::tempdir().unwrap();
        let engine = Engine::new(Some(home.path()));
        let outdated = outdated_of(vec![offer("aa")]);

        engine
            .config()
            .set("plugins.auto-update", serde_json::json!(false))
            .unwrap();

        assert!(engine.auto_update_candidates(&outdated).is_empty());
        assert!(!engine.config().settings().plugins.auto_update);
    }

    #[test]
    fn auto_update_is_on_by_default() {
        let home = tempfile::tempdir().unwrap();
        let engine = Engine::new(Some(home.path()));

        assert!(engine.config().settings().plugins.auto_update);
    }
}
