//! What an export leaves out.
//!
//! Shared by every exporter on purpose: "which files *are* the instance" is one
//! question with one answer, and two writers deciding it separately is how a
//! format ends up quietly shipping somebody's crash reports.
//!
//! The list follows Prism Launcher's own export defaults — logs, crash reports
//! and the loader caches (`.fabric`, `.quilt`) — plus two of hestia's own: the
//! `data/` mirror of a pool item, since the managed copy is the record and the
//! mirror is re-made at every launch, and anything a writer was still building.
//! Saves are **in**. An instance without its worlds is not the instance.

use std::collections::HashSet;
use std::path::Path;

use proto::content::ContentKind;

use crate::content::install;
use crate::schema::Document;

/// Names skipped wherever they appear: OS turds, the world lock (transient
/// state, never data), and partial files.
const SKIP_NAMES: &[&str] = &[".DS_Store", "thumbs.db", "Thumbs.db", "session.lock"];

/// Entry-root directories an export never carries.
const SKIP_ROOTS: &[&str] = &["logs", "backups"];

/// Game-directory paths an export never carries — Prism's own default ignore
/// list, which exists because every one of these is regenerated on next launch.
const SKIP_DATA: &[&str] = &["logs", "crash-reports", ".cache", ".fabric", ".quilt"];

/// The rule for one instance: which of its files an archive carries.
pub(crate) struct Rules {
    mirrors: HashSet<String>,
    exclude: Vec<String>,
}

impl Rules {
    pub(crate) fn new(entry_dir: &Path, data_dir: &Path, exclude: &[String]) -> Rules {
        Rules {
            mirrors: mirrored_paths(entry_dir, data_dir),
            exclude: exclude.to_vec(),
        }
    }

    /// Whether an entry-relative path goes into the archive. Directory names
    /// are asked too, so refusing one prunes its whole subtree.
    pub(crate) fn keeps(&self, relative: &str) -> bool {
        !self.skipped(relative)
    }

    /// The same question for a path relative to the game directory — what an
    /// `overrides/` tree is named by.
    pub(crate) fn keeps_in_data(&self, relative: &str) -> bool {
        !self.skipped(&format!("data/{relative}"))
    }

    fn skipped(&self, relative: &str) -> bool {
        let name = relative.rsplit('/').next().unwrap_or(relative);
        if SKIP_NAMES.contains(&name) || name.ends_with(".part") || name.starts_with(".discard-") {
            return true;
        }
        // The record travels in the manifest, which an import reads before it
        // has anywhere to put a file.
        if relative == crate::instances::InstanceRecord::NAME || SKIP_ROOTS.contains(&relative) {
            return true;
        }
        // The exporter rewrites the index to match what it keeps, so the file on
        // disk is never copied and is not a choice.
        if relative == install::INDEX {
            return true;
        }
        // Where and when its owner played is not part of the instance.
        if relative == crate::recent::FILE {
            return true;
        }
        if relative
            .strip_prefix("data/")
            .is_some_and(|rest| SKIP_DATA.contains(&rest))
        {
            return true;
        }
        if self.mirrors.contains(relative) {
            return true;
        }
        self.exclude
            .iter()
            .any(|path| relative == path || relative.starts_with(&format!("{path}/")))
    }
}

/// Every entry-relative `data/` path that is a mirror of a pool item. The
/// managed copy under the entry root is the record of what is installed; the
/// mirror is a hardlink the launch pass re-makes, and archiving both would
/// carry every mod twice.
fn mirrored_paths(entry_dir: &Path, data_dir: &Path) -> HashSet<String> {
    let worlds = crate::instances::save_worlds(data_dir);
    install::load(entry_dir)
        .iter()
        .flat_map(|item| match item.kind {
            ContentKind::DataPack => install::target_worlds(item, &worlds)
                .iter()
                .map(|world| format!("data/{world}/datapacks/{}", item.filename))
                .collect(),
            kind => install::kind_dir(kind)
                .map(|dir| vec![format!("data/{dir}/{}", item.filename)])
                .unwrap_or_default(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(mirrors: &[&str], exclude: &[&str]) -> Rules {
        Rules {
            mirrors: mirrors.iter().map(|p| p.to_string()).collect(),
            exclude: exclude.iter().map(|p| p.to_string()).collect(),
        }
    }

    #[test]
    fn regenerables_and_os_turds_stay_out() {
        let rules = rules(&[], &[]);
        for path in [
            "logs",
            "backups",
            "instance.json",
            "recent.json",
            "content.json",
            "data/logs",
            "data/crash-reports",
            "data/.fabric",
            "data/.quilt",
            "data/.cache",
            "data/saves/world/session.lock",
            "data/.DS_Store",
            "mods/sodium.jar.part",
        ] {
            assert!(!rules.keeps(path), "{path} should be skipped");
        }
    }

    #[test]
    fn the_instances_own_files_stay_in() {
        let rules = rules(&[], &[]);
        for path in [
            "modpack.json",
            "mods/sodium.jar",
            "data/servers.dat",
            "data/options.txt",
            "data/config/sodium.json",
            "data/saves/world/level.dat",
        ] {
            assert!(rules.keeps(path), "{path} should be kept");
        }
    }

    #[test]
    fn a_pool_items_mirror_is_skipped_but_an_untracked_file_is_not() {
        let rules = rules(&["data/mods/sodium.jar"], &[]);
        assert!(!rules.keeps("data/mods/sodium.jar"));
        assert!(
            rules.keeps("data/mods/handmade.jar"),
            "a jar the user dropped in has no managed copy, so the archive is its only chance"
        );
        assert!(
            rules.keeps("mods/sodium.jar"),
            "the managed copy is the one that travels"
        );
    }

    #[test]
    fn the_data_relative_question_is_the_same_question() {
        let rules = rules(
            &["data/mods/sodium.jar"],
            &["data/saves".to_string().as_str()],
        );
        assert!(!rules.keeps_in_data("mods/sodium.jar"));
        assert!(!rules.keeps_in_data("saves/world/level.dat"));
        assert!(rules.keeps_in_data("options.txt"));
    }

    #[test]
    fn a_caller_exclusion_takes_the_whole_subtree() {
        let rules = rules(&[], &["data/saves"]);
        assert!(!rules.keeps("data/saves"));
        assert!(!rules.keeps("data/saves/world/level.dat"));
        assert!(rules.keeps("data/savesomething"));
        assert!(rules.keeps("data/options.txt"));
    }
}
