//! How the interchange formats name a flavor's loader. A Modrinth pack keys its
//! `dependencies` by a loader key, a Prism instance lists a loader as a
//! component uid, and a plugin's pack format names it its own way; each
//! registered flavor declares the key per format id, so the formats read the
//! registry instead of keeping a table that a new flavor would have to edit.

use std::sync::Arc;

use super::provider::FlavorProvider;

/// The id of the Prism and MultiMC instance format.
pub(crate) const PRISM: &str = "prism";

#[derive(Debug, Clone)]
struct Entry {
    flavor: String,
    keys: Vec<(String, String)>,
}

impl Entry {
    fn key(&self, format: &str) -> Option<&str> {
        self.keys
            .iter()
            .find(|(id, _)| id == format)
            .map(|(_, key)| key.as_str())
    }
}

/// The interchange keys of the flavors registered when it was taken.
#[derive(Debug, Clone, Default)]
pub struct Loaders(Vec<Entry>);

impl Loaders {
    /// A key one flavor already claimed stays with it: the roster lists the
    /// launcher's own flavors first, so a plugin's flavor cannot take over the
    /// loader of one it did not replace.
    pub(super) fn of(flavors: &[Arc<dyn FlavorProvider>]) -> Self {
        let mut claimed: Vec<(String, String)> = Vec::new();
        let mut entries = Vec::new();
        for info in flavors.iter().map(|provider| provider.info()) {
            let keys: Vec<(String, String)> = info
                .interchange
                .iter()
                .map(|(format, key)| (format.to_string(), key.to_string()))
                .filter(|claim| !claimed.contains(claim))
                .collect();
            claimed.extend(keys.iter().cloned());
            if !keys.is_empty() {
                entries.push(Entry {
                    flavor: info.id.to_string(),
                    keys,
                });
            }
        }
        Loaders(entries)
    }

    /// The key the format `format` names `flavor`'s loader by.
    pub fn key_of(&self, format: &str, flavor: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|entry| entry.flavor == flavor)
            .and_then(|entry| entry.key(format))
    }

    /// Every loader key the format `format` has a flavor for, in the roster's
    /// order and without repeats.
    pub fn keys_of(&self, format: &str) -> Vec<String> {
        let mut keys: Vec<String> = Vec::new();
        for key in self.0.iter().filter_map(|entry| entry.key(format)) {
            if !keys.iter().any(|known| known == key) {
                keys.push(key.to_string());
            }
        }
        keys
    }

    /// The flavor the format `format` means by the loader `key`.
    pub fn flavor_of(&self, format: &str, key: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|entry| entry.key(format) == Some(key))
            .map(|entry| entry.flavor.as_str())
    }
}
