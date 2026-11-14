//! Taking the loose jars of somebody else's game directory into the content
//! pool.
//!
//! Every launcher that is not hestia keeps its mods where the game loads them
//! and nowhere else. hestia keeps a managed copy under the entry root and
//! mirrors it into `data/` at launch ([`crate::content::install`]), because
//! that is what makes an item listable and able to survive a restore. So an
//! import from another launcher has one extra step:
//! move what arrived into the pool and record it.
//!
//! Shared rather than per-format: it is the same step for any archive that
//! carries a game directory, which is every format except hestia's own (whose
//! pool travels with it) and a modpack (whose files the modpack flow installs
//! from their sources, with provenance).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use proto::content::{ContentKind, InstalledContent};

use super::exclude::Rules;
use crate::content::{install, record};

/// A pool item an export may carry: the item, where its file is in the game
/// directory, and the managed file that holds its bytes.
pub(crate) struct Pooled {
    pub(crate) path: String,
    pub(crate) managed: PathBuf,
    pub(crate) item: InstalledContent,
}

/// The pool items an export can carry or name. A datapack lives inside a world,
/// not in a load directory an archive format can name, so it travels with
/// `saves/` instead; an item the caller excluded is in neither place, since an
/// exclusion names an entry-relative path and must mean the same thing
/// whichever format is being written.
pub(crate) fn exported(entry_dir: &Path, rules: &Rules) -> Result<Vec<Pooled>> {
    let mut pooled = Vec::new();
    for item in install::load(entry_dir) {
        if item.kind == ContentKind::DataPack {
            continue;
        }
        let Ok(managed) = install::managed_path(entry_dir, &item) else {
            continue;
        };
        if !managed.is_file() {
            continue;
        }
        let path = format!("{}/{}", install::kind_dir(item.kind)?, item.filename);
        if rules.keeps(&path) {
            pooled.push(Pooled {
                path,
                managed,
                item,
            });
        }
    }
    Ok(pooled)
}

/// How another launcher marks a mod as turned off. hestia has a flag for it, so
/// the name comes back and the flag carries the meaning.
const DISABLED: &str = ".disabled";

/// The kinds that live as single managed files in a flat load directory. A
/// datapack is not among them: it lives inside a world, arrives with that
/// world, and loads from there.
const ADOPTED_KINDS: &[ContentKind] = &[
    ContentKind::Mod,
    ContentKind::ResourcePack,
    ContentKind::Shader,
];

/// Move the game directory's loose content files into the entry's managed
/// directories and index them, then re-mirror. Items arrive with no provenance,
/// so each is recorded as a local file; the import flow asks the sources about
/// them afterwards and warns about whatever stays that way.
///
/// Returns the filenames adopted, in the order they were found.
pub(crate) fn adopt(entry_dir: &Path, data_dir: &Path) -> Result<Vec<String>> {
    let mut items = install::load(entry_dir);
    let known: Vec<String> = items.iter().map(|i| i.filename.clone()).collect();
    let mut adopted = Vec::new();

    for kind in ADOPTED_KINDS {
        let dir = install::kind_dir(*kind)?;
        let Ok(entries) = std::fs::read_dir(data_dir.join(dir)) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            // A folder resourcepack is not a single managed file; it stays
            // where it is and the game loads it from there.
            if path.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let (filename, enabled) = match name.strip_suffix(DISABLED) {
                Some(stem) => (stem.to_string(), false),
                None => (name, true),
            };
            if !install::is_content_file(*kind, &filename) {
                tracing::debug!(dir, file = %filename, "left in place, not a content archive");
                continue;
            }
            if known.contains(&filename) {
                continue;
            }
            let managed_dir = entry_dir.join(dir);
            std::fs::create_dir_all(&managed_dir)
                .with_context(|| format!("cannot create {}", managed_dir.display()))?;
            let managed = managed_dir.join(&filename);
            std::fs::rename(&path, &managed)
                .with_context(|| format!("cannot adopt {}", path.display()))?;
            items.push(record::assemble(
                record::Project::untracked(*kind, filename.clone(), &managed),
                record::Release::local(filename.clone(), install::sha1_file(&managed)?),
                record::Holding {
                    enabled,
                    ..record::Holding::fresh(&[])
                },
            ));
            adopted.push(filename);
        }
    }

    if adopted.is_empty() {
        return Ok(adopted);
    }
    install::save(entry_dir, items.clone())?;
    let worlds = crate::instances::save_worlds(data_dir);
    for item in &items {
        install::apply_files(entry_dir, data_dir, item, &worlds)?;
    }
    Ok(adopted)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(tag: &str) -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix(&format!("hestia-adopt-{tag}-"))
            .tempdir()
            .expect("temp dir")
    }

    fn write(root: &Path, relative: &str, body: &str) {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }

    #[test]
    fn loose_jars_become_pool_items_and_are_mirrored_back() {
        let dir = temp("basic");
        let entry_dir = dir.path();
        let data_dir = entry_dir.join("data");
        write(&data_dir, "mods/sodium.jar", "jar");
        write(&data_dir, "resourcepacks/faithful.zip", "zip");

        let adopted = adopt(entry_dir, &data_dir).unwrap();

        assert_eq!(adopted.len(), 2);
        assert!(entry_dir.join("mods/sodium.jar").is_file(), "managed copy");
        assert!(data_dir.join("mods/sodium.jar").is_file(), "re-mirrored");
        let items = install::load(entry_dir);
        assert_eq!(items.len(), 2);
        assert!(items.iter().all(|i| i.source == "file"));
        assert!(items.iter().all(|i| !i.sha1.is_empty()));
    }

    #[test]
    fn a_disabled_mod_keeps_its_name_and_loses_its_suffix() {
        let dir = temp("disabled");
        let entry_dir = dir.path();
        let data_dir = entry_dir.join("data");
        write(&data_dir, "mods/extra.jar.disabled", "jar");

        adopt(entry_dir, &data_dir).unwrap();

        let items = install::load(entry_dir);
        assert_eq!(items[0].filename, "extra.jar");
        assert!(!items[0].enabled);
        assert!(entry_dir.join("mods/extra.jar").is_file());
        assert!(
            !data_dir.join("mods/extra.jar").exists(),
            "a disabled item is not mirrored into the load directory"
        );
    }

    #[test]
    fn a_file_beside_a_pack_is_not_content() {
        let dir = temp("sidecar");
        let entry_dir = dir.path();
        let data_dir = entry_dir.join("data");
        let sidecars = [
            "shaderpacks/bsl.zip.txt",
            "shaderpacks/patcher-errors.txt",
            "resourcepacks/faithful.zip.rpo",
            "resourcepacks/stray.mrpack",
            "mods/notes.zip",
            "mods/README",
        ];
        for sidecar in sidecars {
            write(&data_dir, sidecar, "state");
        }
        write(&data_dir, "shaderpacks/bsl.zip", "zip");

        let adopted = adopt(entry_dir, &data_dir).unwrap();

        assert_eq!(adopted, vec!["bsl.zip".to_string()]);
        for sidecar in sidecars {
            assert!(data_dir.join(sidecar).is_file(), "{sidecar} stays put");
            assert!(!entry_dir.join(sidecar).exists(), "{sidecar} is not pooled");
        }
    }
}
