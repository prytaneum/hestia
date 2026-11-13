//! Which official plugins an app update that changes the plugin API must carry
//! over, and the build to carry for each. Pure, like the update check: the
//! caller supplies the indexes and what is installed
//! ([0112](../../../../docs/decisions/0112-the-app-and-official-plugins-update-together.md)).

use proto::plugin::{api_supported_by, compare_versions, PluginSource};
use proto::repository::Build;

use super::catalogue::{pick_for, Installed, Source};
use super::official::is_official_signer_in;
use super::store::Plugins;
use super::update::listings_of;

/// A build to download and stage for an installed official plugin.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Carry {
    pub id: String,
    pub repository: String,
    pub build: Build,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Plan {
    pub carry: Vec<Carry>,
    /// Official plugins no listing has a newer build of for the new API.
    pub stranded: Vec<String>,
}

/// For each of `broken`, the newest build any listing under its signer's key
/// offers that runs on `host` and is newer than what is installed.
pub(crate) fn plan(sources: &[Source], broken: &[Installed], host: &str) -> Plan {
    let mut plan = Plan::default();
    for installed in broken {
        let signer = installed.signer.as_deref().unwrap_or_default();
        let found = listings_of(sources, &installed.id, signer)
            .into_iter()
            .filter_map(|(source, plugin)| {
                let build = pick_for(source, plugin, host).build()?;
                compare_versions(&build.version, &installed.version)
                    .is_gt()
                    .then_some((source, build))
            })
            .max_by(|(_, a), (_, b)| compare_versions(&a.version, &b.version));
        match found {
            Some((source, build)) => plan.carry.push(Carry {
                id: installed.id.clone(),
                repository: source.url.clone(),
                build: build.clone(),
            }),
            None => plan.stranded.push(installed.id.clone()),
        }
    }
    plan
}

impl Plugins {
    /// Blocking. The installed plugins signed by one of the official `keys`
    /// that a host exposing plugin API `host` could not run. A dev install is
    /// never official, and one whose manifest cannot be read is left to fail
    /// on its own.
    pub fn official_broken_by(&self, host: &str, keys: &[&str]) -> Vec<Installed> {
        self.list()
            .into_iter()
            .filter(|record| {
                record.source != PluginSource::Dev
                    && is_official_signer_in(record.signer.as_deref(), keys)
            })
            .filter_map(|record| {
                let manifest = self.read_declared(&record.id).ok()?;
                let supported = manifest
                    .api
                    .as_deref()
                    .is_none_or(|api| api_supported_by(api, host));
                (!supported).then_some(Installed {
                    id: record.id,
                    version: manifest.version,
                    signer: record.signer,
                    grant: record.grant,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use proto::plugin::API_VERSION;

    use super::*;
    use crate::plugins::repos::key_id_of;
    use crate::plugins::testkit::{sample_index, Signer};

    const NEXT_API: &str = "2.0.0";

    fn source(key: &str, change: impl FnOnce(&mut proto::repository::RepositoryIndex)) -> Source {
        let index = serde_json::from_slice(&sample_index(key, change)).unwrap();
        Source::new("https://a.example/plugins.json".to_string(), key, index)
    }

    fn installed(id: &str, version: &str, key: &str) -> Installed {
        Installed {
            id: id.to_string(),
            version: version.to_string(),
            signer: Some(key_id_of(key).unwrap()),
            grant: Default::default(),
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
    fn a_build_for_the_new_api_is_carried_over_the_one_for_the_old() {
        let key = Signer::new().key();
        let sources = [source(&key, |index| {
            index.plugins[0].version = "0.3.0".to_string();
            index.plugins[0].builds = vec![
                build("0.2.0", &format!("^{API_VERSION}")),
                build("0.3.0", &format!("^{NEXT_API}")),
            ];
        })];

        let plan = plan(&sources, &[installed("aa", "0.2.0", &key)], NEXT_API);

        assert!(plan.stranded.is_empty());
        assert_eq!(plan.carry.len(), 1);
        assert_eq!(plan.carry[0].build.version, "0.3.0");
        assert_eq!(plan.carry[0].repository, "https://a.example/plugins.json");
    }

    #[test]
    fn a_plugin_with_no_build_for_the_new_api_is_stranded() {
        let key = Signer::new().key();
        let sources = [source(&key, |_| {})];
        let broken = [
            installed("aa", "0.1.0", &key),
            installed("zz", "0.1.0", &key),
        ];

        let plan = plan(&sources, &broken, NEXT_API);

        assert!(plan.carry.is_empty());
        assert_eq!(plan.stranded, ["aa", "zz"]);
    }

    #[test]
    fn a_build_no_newer_than_the_install_does_not_save_it() {
        let key = Signer::new().key();
        let api = format!("^{NEXT_API}");
        let sources = [source(&key, |index| {
            index.plugins[0].builds = vec![build("0.2.0", &api)];
        })];

        let plan = plan(&sources, &[installed("aa", "0.2.0", &key)], NEXT_API);

        assert_eq!(plan.stranded, ["aa"]);
    }

    #[test]
    fn a_listing_under_another_key_is_not_a_source_of_builds() {
        let (key, other) = (Signer::new().key(), Signer::new().key());
        let api = format!("^{NEXT_API}");
        let sources = [source(&other, |index| {
            index.plugins[0].version = "0.3.0".to_string();
            index.plugins[0].builds = vec![build("0.3.0", &api)];
        })];

        let plan = plan(&sources, &[installed("aa", "0.1.0", &key)], NEXT_API);

        assert_eq!(plan.stranded, ["aa"]);
    }
}
