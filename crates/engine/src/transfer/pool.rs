//! The content pool as an export sees it.

use std::path::{Path, PathBuf};

use anyhow::Result;
use proto::content::{ContentKind, InstalledContent};

use super::exclude::Rules;
use crate::content::install;

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
