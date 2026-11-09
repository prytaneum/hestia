//! What the configured repositories offer, merged into one list. Pure: the
//! caller supplies the verified indexes and what is installed.

use std::collections::{BTreeMap, HashSet};

use proto::catalogue::{Availability, CataloguePlugin, Lister, RepositoryInfo};
use proto::plugin::{api_supported, compare_versions};
use proto::repository::{Build, IndexedPlugin, RepositoryIndex};

use super::index::icon_url;
use super::repos::{key_id_of, revocation};
use super::store::PluginRecord;

/// A configured repository with the index to read it by.
pub struct Source {
    pub url: String,
    pub key_id: String,
    pub index: RepositoryIndex,
}

impl Source {
    pub fn new(url: String, key: &str, index: RepositoryIndex) -> Source {
        Source {
            key_id: key_id_of(key).unwrap_or_else(|_| key.to_string()),
            url,
            index,
        }
    }

    pub fn info(&self) -> RepositoryInfo {
        RepositoryInfo {
            url: self.url.clone(),
            name: self.index.name.clone(),
            label: self.index.label.clone(),
            website: self.index.website.clone(),
            key_id: self.key_id.clone(),
            plugins: self.index.plugins.len() as u32,
            version: self.index.version,
        }
    }
}

/// What is installed, as far as the catalogue cares.
pub struct Installed {
    pub id: String,
    pub version: String,
    pub signer: Option<String>,
}

/// How one listed plugin can be installed.
#[derive(Debug, PartialEq, Eq)]
pub enum Pick<'a> {
    /// The latest version has a build this Hestia can run.
    Prebuilt(&'a Build),
    /// The latest version has no build yet; an older one may be installable.
    Unreleased(Option<&'a Build>),
    /// The latest version's builds need another API; an older one may be installable.
    Incompatible {
        older: Option<&'a Build>,
        requires: Option<&'a str>,
    },
}

impl<'a> Pick<'a> {
    /// The build an install would use.
    pub fn build(&self) -> Option<&'a Build> {
        match self {
            Pick::Prebuilt(build) => Some(build),
            Pick::Unreleased(older) | Pick::Incompatible { older, .. } => *older,
        }
    }

    fn availability(&self) -> Availability {
        match self {
            Pick::Prebuilt(_) => Availability::Prebuilt,
            Pick::Unreleased(_) => Availability::Unreleased,
            Pick::Incompatible { .. } => Availability::Incompatible,
        }
    }
}

/// The newest build of `plugin` this Hestia can install. A build its own
/// repository revoked is not a build.
pub fn pick<'a>(source: &Source, plugin: &'a IndexedPlugin) -> Pick<'a> {
    let live = |build: &&Build| {
        revocation(&source.index, &source.key_id, &plugin.id, &build.version).is_none()
    };
    let runs = |build: &&Build| build.api.as_deref().is_none_or(api_supported);
    let newest = |builds: Vec<&'a Build>| {
        builds
            .into_iter()
            .max_by(|a, b| compare_versions(&a.version, &b.version))
    };
    let live_builds: Vec<&Build> = plugin.builds.iter().filter(live).collect();
    let compatible = newest(live_builds.iter().copied().filter(runs).collect());
    let latest: Vec<&Build> = live_builds
        .iter()
        .copied()
        .filter(|build| build.version == plugin.version)
        .collect();
    match (compatible, latest.is_empty()) {
        (Some(build), _) if build.version == plugin.version => Pick::Prebuilt(build),
        (older, true) => Pick::Unreleased(older),
        (older, false) => Pick::Incompatible {
            older,
            requires: latest.first().and_then(|build| build.api.as_deref()),
        },
    }
}

/// The catalogue: one entry per (id, key), the newest listing of each across
/// the repositories that share the key, ordered by name.
pub fn build(
    sources: &[Source],
    installed: &[Installed],
    repo: Option<&str>,
    query: Option<&str>,
) -> Vec<CataloguePlugin> {
    let mut groups: BTreeMap<(&str, &str), Vec<(&Source, &IndexedPlugin)>> = BTreeMap::new();
    for source in sources {
        for plugin in &source.index.plugins {
            groups
                .entry((plugin.id.as_str(), source.key_id.as_str()))
                .or_default()
                .push((source, plugin));
        }
    }
    let query = query.map(str::to_lowercase).filter(|text| !text.is_empty());
    let mut entries: Vec<CataloguePlugin> = groups
        .into_iter()
        .filter(|(_, listings)| repo.is_none_or(|url| listings.iter().any(|(s, _)| s.url == url)))
        .map(|((id, key_id), listings)| entry(id, key_id, &listings, installed))
        .filter(|plugin| {
            query.as_deref().is_none_or(|text| {
                [&plugin.id, &plugin.name, &plugin.description]
                    .iter()
                    .any(|field| field.to_lowercase().contains(text))
            })
        })
        .collect();
    entries.sort_by_cached_key(|plugin| (plugin.name.to_lowercase(), plugin.id.clone()));
    entries
}

fn entry(
    id: &str,
    key_id: &str,
    listings: &[(&Source, &IndexedPlugin)],
    installed: &[Installed],
) -> CataloguePlugin {
    let (source, plugin) = listings
        .iter()
        .max_by(|(a_source, a), (b_source, b)| {
            compare_versions(&a.version, &b.version)
                .then_with(|| rank(&pick(a_source, a)).cmp(&rank(&pick(b_source, b))))
        })
        .copied()
        .expect("a group has a listing");
    let choice = pick(source, plugin);
    CataloguePlugin {
        id: id.to_string(),
        name: plugin.name.clone(),
        description: plugin.description.clone(),
        icon: icon_url(&source.index, plugin),
        version: plugin.version.clone(),
        permissions: plugin.permissions.clone(),
        key_id: key_id.to_string(),
        listed_by: std::iter::once(source)
            .chain(
                listings
                    .iter()
                    .map(|(other, _)| *other)
                    .filter(|other| other.url != source.url),
            )
            .map(|source| Lister {
                url: source.url.clone(),
                label: source.index.label.clone(),
                name: source.index.name.clone(),
            })
            .collect(),
        availability: choice.availability(),
        build: choice.build().map(|build| build.version.clone()),
        requires: match &choice {
            Pick::Incompatible { requires, .. } => requires.map(str::to_string),
            _ => None,
        },
        installed: installed
            .iter()
            .find(|record| record.id == id && record.signer.as_deref() == Some(key_id))
            .map(|record| record.version.clone()),
    }
}

fn rank(choice: &Pick<'_>) -> bool {
    matches!(choice, Pick::Prebuilt(_))
}

/// Which signed plugins some configured repository still lists, by (key id, plugin id).
#[derive(Default)]
pub struct Listings(HashSet<(String, String)>);

impl Listings {
    pub fn of(sources: &[Source]) -> Listings {
        Listings(
            sources
                .iter()
                .flat_map(|source| {
                    source
                        .index
                        .plugins
                        .iter()
                        .map(|plugin| (source.key_id.clone(), plugin.id.clone()))
                })
                .collect(),
        )
    }

    /// Signed by a key that no configured repository lists the plugin under.
    /// An unsigned install has no update source by design, so it is never obsolete.
    pub fn obsolete(&self, record: &PluginRecord) -> bool {
        record
            .signer
            .as_ref()
            .is_some_and(|signer| !self.0.contains(&(signer.clone(), record.id.clone())))
    }
}

#[cfg(test)]
mod tests {
    use proto::plugin::API_VERSION;
    use proto::repository::Revocation;

    use super::*;
    use crate::plugins::testkit::{sample_index, Signer};

    fn source(url: &str, key: &str, change: impl FnOnce(&mut RepositoryIndex)) -> Source {
        let index = serde_json::from_slice(&sample_index(key, change)).unwrap();
        Source::new(url.to_string(), key, index)
    }

    fn build_of(version: &str, api: Option<&str>) -> Build {
        Build {
            version: version.to_string(),
            api: api.map(str::to_string),
            file: format!("packages/{version}.hplugin"),
            sha256: "0".repeat(64),
        }
    }

    fn plugin(version: &str, builds: Vec<Build>) -> IndexedPlugin {
        IndexedPlugin {
            id: "aa".to_string(),
            version: version.to_string(),
            builds,
            ..IndexedPlugin::default()
        }
    }

    fn host_source() -> Source {
        Source::new(
            "https://a.example/i.json".to_string(),
            &Signer::new().key(),
            RepositoryIndex::default(),
        )
    }

    #[test]
    fn the_latest_version_with_a_build_for_this_api_is_prebuilt() {
        let api = format!("^{API_VERSION}");
        let listed = plugin(
            "0.2.0",
            vec![build_of("0.1.0", Some(&api)), build_of("0.2.0", Some(&api))],
        );

        let source = host_source();
        let choice = pick(&source, &listed);

        assert!(matches!(choice, Pick::Prebuilt(b) if b.version == "0.2.0"));
    }

    #[test]
    fn a_version_without_a_build_is_unreleased_and_names_the_older_build() {
        let api = format!("^{API_VERSION}");
        let listed = plugin("0.3.0", vec![build_of("0.2.0", Some(&api))]);
        let nothing = plugin("0.3.0", vec![]);
        let source = host_source();

        assert!(matches!(
            pick(&source, &listed),
            Pick::Unreleased(Some(b)) if b.version == "0.2.0"
        ));
        assert_eq!(pick(&source, &nothing), Pick::Unreleased(None));
    }

    #[test]
    fn builds_for_another_api_are_incompatible_and_name_what_they_need() {
        let listed = plugin(
            "0.2.0",
            vec![
                build_of("0.1.0", Some("^1.0.0")),
                build_of("0.2.0", Some("^9.0.0")),
            ],
        );
        let source = host_source();

        let choice = pick(&source, &listed);

        assert!(matches!(
            choice,
            Pick::Incompatible { older: Some(b), requires: Some("^9.0.0") } if b.version == "0.1.0"
        ));
    }

    #[test]
    fn a_theme_only_build_has_no_api_and_runs_anywhere() {
        let listed = plugin("1.0.0", vec![build_of("1.0.0", None)]);

        assert!(matches!(pick(&host_source(), &listed), Pick::Prebuilt(_)));
    }

    #[test]
    fn a_revoked_build_is_not_a_build() {
        let mut source = host_source();
        let api = format!("^{API_VERSION}");
        let listed = plugin("0.2.0", vec![build_of("0.2.0", Some(&api))]);
        source.index.key = Signer::new().key();
        source.key_id = key_id_of(&source.index.key).unwrap();
        source.index.revoked = vec![Revocation {
            id: "aa".to_string(),
            versions: vec!["0.2.0".to_string()],
            reason: "bad".to_string(),
        }];

        assert_eq!(pick(&source, &listed), Pick::Unreleased(None));
    }

    #[test]
    fn mirrors_with_one_key_are_one_entry_and_other_keys_are_not_merged() {
        let key = Signer::new().key();
        let sources = [
            source("https://a.example/i.json", &key, |_| {}),
            source("https://b.example/i.json", &key, |_| {}),
            source("https://c.example/i.json", &Signer::new().key(), |index| {
                index.plugins.retain(|plugin| plugin.id == "aa")
            }),
        ];

        let catalogue = build(&sources, &[], None, None);

        let aa: Vec<_> = catalogue.iter().filter(|p| p.id == "aa").collect();
        assert_eq!(aa.len(), 2, "one per key");
        let shared = aa.iter().find(|p| p.listed_by.len() == 2).unwrap();
        assert_eq!(shared.version, "0.2.0");
        assert_eq!(shared.build.as_deref(), Some("0.2.0"));
        assert_eq!(shared.availability, Availability::Prebuilt);
        assert_eq!(catalogue.len(), 3, "bb is listed by the mirrors only");
    }

    #[test]
    fn the_newest_listing_among_mirrors_wins() {
        let key = Signer::new().key();
        let sources = [
            source("https://a.example/i.json", &key, |index| {
                index.plugins[0].version = "0.1.0".to_string();
                index.plugins[0].builds.truncate(1);
            }),
            source("https://b.example/i.json", &key, |_| {}),
        ];

        let catalogue = build(&sources, &[], None, None);

        let aa = catalogue.iter().find(|p| p.id == "aa").unwrap();
        assert_eq!(aa.version, "0.2.0");
    }

    #[test]
    fn the_repository_filter_and_the_query_narrow_the_list() {
        let (first, second) = (Signer::new().key(), Signer::new().key());
        let sources = [
            source("https://a.example/i.json", &first, |_| {}),
            source("https://b.example/i.json", &second, |index| {
                index.plugins.retain(|plugin| plugin.id == "bb")
            }),
        ];

        assert_eq!(
            build(&sources, &[], Some("https://b.example/i.json"), None).len(),
            1
        );
        assert_eq!(build(&sources, &[], None, Some("BB")).len(), 2);
        assert_eq!(build(&sources, &[], None, Some("nothing")).len(), 0);
    }

    #[test]
    fn installed_shows_only_for_the_key_that_signed_it() {
        let (first, second) = (Signer::new().key(), Signer::new().key());
        let sources = [
            source("https://a.example/i.json", &first, |_| {}),
            source("https://b.example/i.json", &second, |_| {}),
        ];
        let installed = [Installed {
            id: "aa".to_string(),
            version: "0.1.0".to_string(),
            signer: Some(key_id_of(&first).unwrap()),
        }];

        let catalogue = build(&sources, &installed, None, None);

        let versions: Vec<_> = catalogue
            .iter()
            .filter(|p| p.id == "aa")
            .map(|p| p.installed.as_deref())
            .collect();
        assert_eq!(versions.iter().filter(|v| **v == Some("0.1.0")).count(), 1);
        assert_eq!(versions.iter().filter(|v| v.is_none()).count(), 1);
    }

    #[test]
    fn a_signed_plugin_is_obsolete_when_no_repository_with_its_key_lists_it() {
        let (listed_key, other_key) = (Signer::new().key(), Signer::new().key());
        let sources = [source("https://a.example/i.json", &listed_key, |_| {})];
        let listings = Listings::of(&sources);
        let record = |id: &str, signer: Option<String>| PluginRecord {
            id: id.to_string(),
            signer,
            ..PluginRecord::default()
        };

        assert!(!listings.obsolete(&record("aa", Some(key_id_of(&listed_key).unwrap()))));
        assert!(listings.obsolete(&record("aa", Some(key_id_of(&other_key).unwrap()))));
        assert!(listings.obsolete(&record("zz", Some(key_id_of(&listed_key).unwrap()))));
        assert!(!listings.obsolete(&record("zz", None)), "unsigned never");
    }
}
