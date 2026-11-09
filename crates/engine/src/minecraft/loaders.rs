//! How the interchange formats name a flavor's loader. A Modrinth pack keys its
//! `dependencies` by a loader key and a Prism instance lists a loader as a
//! component uid; each registered flavor declares its own, so the formats read
//! the registry instead of keeping a table that a new flavor would have to edit.

use std::sync::Arc;

use super::provider::FlavorProvider;

#[derive(Debug, Clone)]
struct Entry {
    flavor: String,
    mrpack_key: Option<String>,
    prism_uid: Option<String>,
}

/// The loader names of the flavors registered when it was taken.
#[derive(Debug, Clone, Default)]
pub struct Loaders(Vec<Entry>);

impl Loaders {
    pub(super) fn of(flavors: &[Arc<dyn FlavorProvider>]) -> Self {
        Loaders(
            flavors
                .iter()
                .map(|provider| provider.info())
                .filter(|info| info.mrpack_key.is_some() || info.prism_uid.is_some())
                .map(|info| Entry {
                    flavor: info.id.to_string(),
                    mrpack_key: info.mrpack_key.as_deref().map(str::to_string),
                    prism_uid: info.prism_uid.as_deref().map(str::to_string),
                })
                .collect(),
        )
    }

    /// The key a pack names `flavor`'s loader by.
    pub fn mrpack_key(&self, flavor: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|entry| entry.flavor == flavor)
            .and_then(|entry| entry.mrpack_key.as_deref())
    }

    /// The flavor a pack dependency `key` names.
    pub fn flavor_of_mrpack_key(&self, key: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|entry| entry.mrpack_key.as_deref() == Some(key))
            .map(|entry| entry.flavor.as_str())
    }

    /// The flavor a Prism component `uid` names.
    pub fn flavor_of_prism_uid(&self, uid: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|entry| entry.prism_uid.as_deref() == Some(uid))
            .map(|entry| entry.flavor.as_str())
    }
}
