//! Noticing that a plugin's sources changed, for `plugin dev`: mtimes polled
//! with the standard library alone, so it behaves the same on every platform.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use super::repo::{CRATE_MANIFEST, MANIFEST};

pub const POLL: Duration = Duration::from_millis(500);
const SETTLE: Duration = Duration::from_millis(500);
const WATCHED_DIRS: [&str; 3] = ["src", "assets", "themes"];

/// Every watched file with its modification time and size. The size catches an
/// edit that lands inside the clock's resolution.
pub type Snapshot = BTreeMap<PathBuf, (SystemTime, u64)>;

pub fn snapshot(plugin: &Path) -> Snapshot {
    let mut files = Snapshot::new();
    for file in [CRATE_MANIFEST, MANIFEST] {
        record(&plugin.join(file), &mut files);
    }
    for dir in WATCHED_DIRS {
        record_tree(&plugin.join(dir), &mut files);
    }
    files
}

fn record(path: &Path, files: &mut Snapshot) {
    if let Ok(meta) = std::fs::metadata(path) {
        if let Ok(modified) = meta.modified() {
            files.insert(path.to_path_buf(), (modified, meta.len()));
        }
    }
}

fn record_tree(dir: &Path, files: &mut Snapshot) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            record_tree(&path, files);
        } else {
            record(&path, files);
        }
    }
}

/// A plugin's sources and the quiet period a burst of edits must end in before
/// a rebuild starts.
pub struct Watch {
    plugin: PathBuf,
    seen: Snapshot,
    last_change: Option<Instant>,
}

impl Watch {
    pub fn new(plugin: PathBuf) -> Watch {
        Watch {
            seen: snapshot(&plugin),
            plugin,
            last_change: None,
        }
    }

    /// Looks at the sources now; true once a change has been quiet long enough.
    pub fn poll(&mut self) -> bool {
        self.observe(snapshot(&self.plugin), Instant::now())
    }

    fn observe(&mut self, now: Snapshot, at: Instant) -> bool {
        if now != self.seen {
            self.seen = now;
            self.last_change = Some(at);
            return false;
        }
        match self.last_change {
            Some(changed) if at.duration_since(changed) >= SETTLE => {
                self.last_change = None;
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(files: &[(&str, u64, u64)]) -> Snapshot {
        files
            .iter()
            .map(|(name, secs, len)| {
                (
                    PathBuf::from(name),
                    (SystemTime::UNIX_EPOCH + Duration::from_secs(*secs), *len),
                )
            })
            .collect()
    }

    fn watch(initial: Snapshot) -> Watch {
        Watch {
            plugin: PathBuf::new(),
            seen: initial,
            last_change: None,
        }
    }

    #[test]
    fn nothing_changing_never_rebuilds() {
        let start = Instant::now();
        let mut watch = watch(snap(&[("src/lib.rs", 1, 10)]));

        for step in 0..10 {
            let at = start + SETTLE * step;
            assert!(!watch.observe(snap(&[("src/lib.rs", 1, 10)]), at));
        }
    }

    #[test]
    fn a_change_rebuilds_once_it_has_been_quiet_for_the_settle_time() {
        let start = Instant::now();
        let mut watch = watch(snap(&[("src/lib.rs", 1, 10)]));
        let edited = snap(&[("src/lib.rs", 2, 12)]);

        assert!(!watch.observe(edited.clone(), start));
        assert!(!watch.observe(edited.clone(), start + SETTLE / 2));
        assert!(watch.observe(edited.clone(), start + SETTLE));
        assert!(!watch.observe(edited, start + SETTLE * 2));
    }

    #[test]
    fn a_burst_of_edits_is_one_rebuild_after_the_last_one() {
        let start = Instant::now();
        let mut watch = watch(snap(&[("src/lib.rs", 1, 10)]));

        assert!(!watch.observe(snap(&[("src/lib.rs", 2, 11)]), start));
        let later = start + SETTLE / 2;
        assert!(!watch.observe(snap(&[("src/lib.rs", 3, 12)]), later));
        let still_editing = later + SETTLE / 2;
        assert!(!watch.observe(snap(&[("src/lib.rs", 3, 12)]), still_editing));
        assert!(watch.observe(snap(&[("src/lib.rs", 3, 12)]), later + SETTLE));
    }

    #[test]
    fn an_added_or_removed_file_is_a_change() {
        let start = Instant::now();
        let mut watch = watch(snap(&[("src/lib.rs", 1, 10)]));

        assert!(!watch.observe(snap(&[("src/lib.rs", 1, 10), ("src/new.rs", 1, 5)]), start));
        assert!(watch.observe(
            snap(&[("src/lib.rs", 1, 10), ("src/new.rs", 1, 5)]),
            start + SETTLE
        ));
        assert!(!watch.observe(snap(&[("src/lib.rs", 1, 10)]), start + SETTLE * 2));
    }

    #[test]
    fn a_same_second_edit_that_changes_the_size_is_seen() {
        let start = Instant::now();
        let mut watch = watch(snap(&[("src/lib.rs", 1, 10)]));

        assert!(!watch.observe(snap(&[("src/lib.rs", 1, 11)]), start));
        assert!(watch.observe(snap(&[("src/lib.rs", 1, 11)]), start + SETTLE));
    }

    #[test]
    fn the_snapshot_covers_sources_manifests_and_bundled_folders_only() {
        let plugin = tempfile::tempdir().unwrap();
        let path = plugin.path();
        for dir in ["src/nested", "assets", "themes", "target/release"] {
            std::fs::create_dir_all(path.join(dir)).unwrap();
        }
        for file in [
            "Cargo.toml",
            "plugin.toml",
            "src/lib.rs",
            "src/nested/mod.rs",
            "assets/icon.png",
            "themes/dark.css",
            "target/release/plug.wasm",
            "README.md",
        ] {
            std::fs::write(path.join(file), "x").unwrap();
        }

        let watched: Vec<String> = snapshot(path)
            .keys()
            .map(|file| {
                file.strip_prefix(path)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect();

        assert_eq!(
            watched,
            [
                "Cargo.toml",
                "assets/icon.png",
                "plugin.toml",
                "src/lib.rs",
                "src/nested/mod.rs",
                "themes/dark.css"
            ]
        );
    }

    #[test]
    fn editing_a_file_changes_the_snapshot_of_the_folder() {
        let plugin = tempfile::tempdir().unwrap();
        std::fs::create_dir(plugin.path().join("src")).unwrap();
        let lib = plugin.path().join("src/lib.rs");
        std::fs::write(&lib, "a").unwrap();
        let before = snapshot(plugin.path());

        std::fs::write(&lib, "ab").unwrap();

        assert_ne!(snapshot(plugin.path()), before);
    }
}
