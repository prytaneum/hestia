//! Which signed installs a repository now offers a newer build of. Pure, like
//! the catalogue it reads through: the caller supplies the indexes and what is
//! installed.

use proto::plugin::{compare_versions, Grant, OutdatedPlugin};
use proto::repository::IndexedPlugin;

use super::catalogue::{best, pick, Installed, Pick, Source};

/// The signed installs with a newer version on offer, by id. Only repositories
/// pinning the key that signed the install count, so another key listing the
/// same id is ignored, and an install nobody lists any more (obsolete) is left out.
pub fn outdated(sources: &[Source], installed: &[Installed]) -> Vec<OutdatedPlugin> {
    let mut found: Vec<OutdatedPlugin> = installed
        .iter()
        .filter_map(|record| {
            let signer = record.signer.as_deref()?;
            let listings: Vec<(&Source, &IndexedPlugin)> = sources
                .iter()
                .filter(|source| source.key_id == signer)
                .flat_map(|source| {
                    source
                        .index
                        .plugins
                        .iter()
                        .filter(|plugin| plugin.id == record.id)
                        .map(move |plugin| (source, plugin))
                })
                .collect();
            let (source, plugin) = best(&listings)?;
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
}
