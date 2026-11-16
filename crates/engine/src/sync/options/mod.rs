//! `options.txt`, merged setting by setting: each side is read into canonical
//! values and written back in its own version's spelling.

mod keys;
mod settings;
mod vanilla;

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::Result;

pub use settings::LOCAL_BY_DEFAULT;

use super::document::Document;
use super::reconcile;

pub fn merge(
    baseline: &Path,
    store: &Path,
    data: &Path,
    excluded: &BTreeSet<String>,
    game_version: &str,
) -> Result<()> {
    let stored_doc = Document::read(store)?;
    let local_doc = Document::read(data)?;
    if stored_doc.is_none() && local_doc.is_none() {
        return Ok(());
    }
    let mut stored_doc = stored_doc.unwrap_or_default();
    let mut local_doc = local_doc.unwrap_or_default();
    let base_doc = Document::read(baseline).ok().flatten().unwrap_or_default();
    let version = crate::version::parse(game_version).unwrap_or(
        if crate::version::predates_releases(game_version) {
            settings::OLDEST
        } else {
            settings::MODERN
        },
    );
    let data_newer = reconcile::newer(data, store);

    let stored = canonical(&stored_doc, settings::MODERN);
    let local = canonical(&local_doc, version);
    let base = canonical(&base_doc, settings::MODERN);

    let shared: BTreeSet<String> = stored.keys().chain(local.keys()).cloned().collect();
    for id in shared {
        if pinned(&id, excluded) {
            continue;
        }
        let settled =
            reconcile::one(base.get(&id), stored.get(&id), local.get(&id), data_newer).cloned();
        let Some(value) = settled else {
            continue;
        };
        if let Some((key, raw)) = settings::encode(&id, &value, settings::MODERN) {
            stored_doc.set(&key, &raw);
        }
        let known = !settings::spellings(&id).is_empty() || vanilla::is_vanilla(&id);
        if !known && !local.contains_key(&id) {
            continue;
        }
        if let Some((key, raw)) = settings::encode(&id, &value, version) {
            local_doc.set(&key, &raw);
        }
    }

    reconcile::write_if_changed(data, local_doc.render().as_bytes())?;
    reconcile::write_if_changed(store, stored_doc.render().as_bytes())?;
    reconcile::write_if_changed(baseline, stored_doc.render().as_bytes())
}

fn canonical(document: &Document, version: (u64, u64, u64)) -> BTreeMap<String, String> {
    document
        .keys()
        .map(str::to_string)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter_map(|key| {
            let raw = document.get(&key)?;
            settings::decode(&key, raw, version)
        })
        .collect()
}

fn pinned(id: &str, excluded: &BTreeSet<String>) -> bool {
    excluded.contains(id)
        || settings::spellings(id)
            .iter()
            .any(|spelling| excluded.contains(*spelling))
}

pub fn read(path: &Path) -> BTreeMap<String, String> {
    let Ok(Some(document)) = Document::read(path) else {
        return BTreeMap::new();
    };
    document
        .keys()
        .map(str::to_string)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter_map(|key| {
            let value = document.get(&key)?.to_string();
            Some((key, value))
        })
        .collect()
}

pub fn set(path: &Path, key: &str, value: &str) -> Result<()> {
    let mut document = Document::read(path)?.unwrap_or_default();
    document.set(key, value);
    reconcile::write_if_changed(path, document.render().as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, text: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, text).unwrap();
    }

    fn backdate(path: &Path) {
        let when = std::time::SystemTime::now() - std::time::Duration::from_secs(60);
        std::fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(when)
            .unwrap();
    }

    struct Fixture {
        _dir: tempfile::TempDir,
        baseline: std::path::PathBuf,
        store: std::path::PathBuf,
        data: std::path::PathBuf,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        Fixture {
            baseline: dir.path().join("baseline.txt"),
            store: dir.path().join("store.txt"),
            data: dir.path().join("data.txt"),
            _dir: dir,
        }
    }

    #[test]
    fn a_legacy_instance_reads_the_shared_keybinds() {
        let f = fixture();
        write(&f.store, "key_key.forward:key.keyboard.up\n");
        merge(&f.baseline, &f.store, &f.data, &BTreeSet::new(), "1.12.2").unwrap();

        assert_eq!(
            std::fs::read_to_string(&f.data).unwrap(),
            "key_key.forward:200\n"
        );
    }

    #[test]
    fn a_legacy_instances_bind_reaches_a_modern_one() {
        let f = fixture();
        write(&f.data, "key_key.jump:57\n");
        merge(&f.baseline, &f.store, &f.data, &BTreeSet::new(), "1.12.2").unwrap();

        assert_eq!(
            std::fs::read_to_string(&f.store).unwrap(),
            "key_key.jump:key.keyboard.space\n"
        );
    }

    #[test]
    fn a_renamed_setting_keeps_being_shared() {
        let f = fixture();
        write(&f.store, "graphicsMode:2\n");
        merge(&f.baseline, &f.store, &f.data, &BTreeSet::new(), "1.15.2").unwrap();

        assert_eq!(
            std::fs::read_to_string(&f.data).unwrap(),
            "fancyGraphics:true\n"
        );
    }

    #[test]
    fn a_mods_setting_reaches_only_instances_that_have_it() {
        let f = fixture();
        write(&f.data, "guiScale:1\n");
        backdate(&f.data);
        write(&f.store, "guiScale:2\nsodium.options:on\n");
        merge(&f.baseline, &f.store, &f.data, &BTreeSet::new(), "1.21.4").unwrap();

        let local = std::fs::read_to_string(&f.data).unwrap();
        assert!(local.contains("guiScale:2"));
        assert!(!local.contains("sodium.options"));
        assert!(std::fs::read_to_string(&f.store)
            .unwrap()
            .contains("sodium.options:on"));
    }

    #[test]
    fn pinning_a_setting_pins_every_spelling_of_it() {
        let f = fixture();
        write(&f.store, "graphicsMode:2\n");
        write(&f.data, "fancyGraphics:false\n");
        let pinned = BTreeSet::from(["graphicsMode".to_string()]);
        merge(&f.baseline, &f.store, &f.data, &pinned, "1.15.2").unwrap();

        assert_eq!(
            std::fs::read_to_string(&f.data).unwrap(),
            "fancyGraphics:false\n"
        );
    }
}
