//! The per-entry modpack record: which pack an entry runs, and which
//! game-directory files that pack owns.
//!
//! The pack's *mods* need no record here — they are ordinary pool items tagged
//! `modpack:<project>` in `content.json`, so the mirror, the backup heal and the
//! untracked check all work on them unchanged. What does need recording is
//! everything the pack writes straight into `data/`: its `overrides/` and any
//! index file outside a managed kind directory. Those are configs and keymaps
//! the game owns and the user edits, so the record stores the **hash each was
//! written with** — that is what lets an update replace a file the pack still
//! owns while leaving one the user has since changed exactly as they left it.

use std::path::Path;

use anyhow::Result;
use proto::modpack::{InstalledModpack, ModpackOverride};

use crate::content::install;
use crate::registry;
use crate::schema::Document;

impl Document for InstalledModpack {
    const NAME: &'static str = "modpack.json";
}

pub(crate) fn load(entry_dir: &Path) -> Option<InstalledModpack> {
    registry::read_record::<InstalledModpack>(entry_dir)
}

pub(crate) fn save(entry_dir: &Path, pack: &InstalledModpack) -> Result<()> {
    registry::write_record(entry_dir, pack)
}

pub(crate) fn clear(entry_dir: &Path) {
    let path = registry::record_path::<InstalledModpack>(entry_dir);
    if let Err(e) = std::fs::remove_file(&path) {
        if e.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!(path = %path.display(), error = %e, "cannot remove the modpack record");
        }
    }
}

/// Whether the launcher may still write or delete this game-directory file:
/// true when it is missing (nothing to lose) or still byte-for-byte what the
/// pack wrote. A file whose hash has moved is the user's edit, and the whole
/// point of recording hashes is to leave it alone.
pub(crate) fn ours(data_dir: &Path, entry: &ModpackOverride) -> bool {
    let path = data_dir.join(&entry.path);
    if !path.exists() {
        return true;
    }
    match install::sha1_file(&path) {
        Ok(hex) => hex == entry.sha1,
        // Unreadable is not "ours to overwrite" — refuse rather than clobber.
        Err(e) => {
            tracing::warn!(path = %path.display(), error = %e, "cannot hash a modpack file");
            false
        }
    }
}

/// Delete the pack-owned game-directory files, leaving any the user has edited.
/// Returns how many went and which were kept.
pub(crate) fn remove_overrides(
    data_dir: &Path,
    overrides: &[ModpackOverride],
) -> (u32, Vec<String>) {
    let mut removed = 0;
    let mut kept = Vec::new();
    for entry in overrides {
        if !ours(data_dir, entry) {
            kept.push(entry.path.clone());
            continue;
        }
        let path = data_dir.join(&entry.path);
        match std::fs::remove_file(&path) {
            Ok(()) => removed += 1,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                tracing::warn!(path = %path.display(), error = %e, "cannot remove a modpack file");
                kept.push(entry.path.clone());
            }
        }
    }
    (removed, kept)
}

/// The origin tag a pack's pool items carry. Keyed by the pack's project rather
/// than its display name: a name is free text a pack author changes between
/// versions, and the tag has to survive that to keep identifying its own items.
pub(crate) fn origin(pack: &InstalledModpack) -> String {
    match pack.project_id.is_empty() {
        true => format!("modpack:{}", pack.name),
        false => format!("modpack:{}", pack.project_id),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(tag: &str) -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix(&format!("hestia-modpack-{tag}-"))
            .tempdir()
            .expect("temp dir")
    }

    fn wrote(data: &Path, path: &str, body: &str) -> ModpackOverride {
        let full = data.join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(&full, body).unwrap();
        ModpackOverride {
            path: path.to_string(),
            sha1: install::sha1_file(&full).unwrap(),
        }
    }

    #[test]
    fn an_untouched_file_is_ours_and_an_edited_one_is_not() {
        let dir = temp("ours");
        let data = dir.path();
        let entry = wrote(data, "config/cozy.toml", "pack default");
        assert!(ours(data, &entry));

        std::fs::write(data.join("config/cozy.toml"), "my tweak").unwrap();
        assert!(!ours(data, &entry), "a user edit is not ours to overwrite");

        std::fs::remove_file(data.join("config/cozy.toml")).unwrap();
        assert!(ours(data, &entry), "a missing file has nothing to lose");
    }

    #[test]
    fn removal_skips_what_the_user_edited() {
        let dir = temp("remove");
        let data = dir.path();
        let keep = wrote(data, "config/mine.toml", "pack default");
        let go = wrote(data, "config/theirs.toml", "pack default");
        std::fs::write(data.join("config/mine.toml"), "edited").unwrap();

        let (removed, kept) = remove_overrides(data, &[keep, go]);

        assert_eq!(removed, 1);
        assert_eq!(kept, vec!["config/mine.toml".to_string()]);
        assert!(data.join("config/mine.toml").is_file());
        assert!(!data.join("config/theirs.toml").exists());
    }
}
