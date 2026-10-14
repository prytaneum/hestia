//! What an export would carry, as a tree — so a caller can leave part of it
//! out knowingly rather than guessing.
//!
//! Derived from the **same plan the export writes**, not from a second walk
//! with its own idea of what counts: a listing that disagreed with the archive
//! would be worse than no listing, since it is the thing people make decisions
//! against.

use std::collections::BTreeMap;
use std::path::Path;

use proto::transfer::ArchiveEntry;

use super::archive;
use super::exclude::Rules;
use crate::content::install;

/// How deep the tree goes. Three levels reaches `data/saves/<world>` — the
/// deepest thing anyone actually wants to exclude individually — while keeping
/// a config tree from becoming thousands of nodes. Anything deeper is folded
/// into its ancestor's size.
const MAX_DEPTH: usize = 3;

pub(crate) fn list(entry_dir: &Path, data_dir: &Path) -> Vec<ArchiveEntry> {
    let rules = Rules::new(entry_dir, data_dir, &[]);
    let referenced = install::referenceable_paths(entry_dir);
    let mut nodes: BTreeMap<String, ArchiveEntry> = BTreeMap::new();

    for member in archive::plan(entry_dir, "", &|relative| rules.keeps(relative)) {
        let is_referenced = referenced.contains(&member.name);
        let size = match is_referenced {
            true => 0,
            false => std::fs::metadata(&member.source)
                .map(|m| m.len())
                .unwrap_or(0),
        };
        let segments: Vec<&str> = member.name.split('/').collect();

        // Every ancestor directory carries what is under it, however deep.
        for depth in 1..segments.len().min(MAX_DEPTH + 1) {
            let path = segments[..depth].join("/");
            nodes
                .entry(path.clone())
                .or_insert_with(|| ArchiveEntry {
                    name: segments[depth - 1].to_string(),
                    path,
                    directory: true,
                    size_bytes: 0,
                    referenced: false,
                })
                .size_bytes += size;
        }
        if segments.len() <= MAX_DEPTH {
            nodes.insert(
                member.name.clone(),
                ArchiveEntry {
                    name: segments[segments.len() - 1].to_string(),
                    path: member.name,
                    directory: false,
                    size_bytes: size,
                    referenced: is_referenced,
                },
            );
        }
    }
    nodes.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(tag: &str) -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix(&format!("hestia-contents-{tag}-"))
            .tempdir()
            .expect("temp dir")
    }

    fn write(root: &Path, relative: &str, body: &str) {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }

    fn node<'a>(entries: &'a [ArchiveEntry], path: &str) -> &'a ArchiveEntry {
        entries
            .iter()
            .find(|e| e.path == path)
            .unwrap_or_else(|| panic!("{path} is missing from {entries:#?}"))
    }

    #[test]
    fn directories_carry_what_is_under_them_and_the_tree_stops_at_a_world() {
        let dir = temp("tree");
        let entry_dir = dir.path();
        let data_dir = entry_dir.join("data");
        write(entry_dir, "mods/sodium.jar", "0123456789");
        write(&data_dir, "options.txt", "12345");
        write(&data_dir, "saves/New World/level.dat", "1234");
        write(&data_dir, "saves/New World/region/r.0.0.mca", "123456");

        let entries = list(entry_dir, &data_dir);

        assert_eq!(node(&entries, "mods").size_bytes, 10);
        assert!(node(&entries, "mods").directory);
        assert_eq!(node(&entries, "mods/sodium.jar").size_bytes, 10);
        assert_eq!(node(&entries, "data/options.txt").size_bytes, 5);
        assert_eq!(
            node(&entries, "data/saves").size_bytes,
            10,
            "a directory totals everything the archive would carry under it"
        );
        assert_eq!(
            node(&entries, "data/saves/New World").size_bytes,
            10,
            "the deepest node folds the whole world into one leaf"
        );
        assert!(
            !entries
                .iter()
                .any(|e| e.path.starts_with("data/saves/New World/")),
            "nothing below the cutoff is listed individually"
        );
    }

    #[test]
    fn what_the_archive_skips_is_not_offered() {
        let dir = temp("skips");
        let entry_dir = dir.path();
        let data_dir = entry_dir.join("data");
        write(entry_dir, "instance.json", "{}");
        write(entry_dir, "logs/session-1.log", "noise");
        write(&data_dir, "crash-reports/crash.txt", "noise");
        write(&data_dir, "options.txt", "12345");

        let entries = list(entry_dir, &data_dir);

        let paths: Vec<&str> = entries.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(paths, ["data", "data/options.txt"]);
    }

    #[test]
    fn an_instance_with_nothing_in_it_lists_nothing() {
        let dir = temp("empty");
        assert!(list(dir.path(), &dir.path().join("data")).is_empty());
    }
}
