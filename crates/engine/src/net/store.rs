//! The last good copy of a catalogue response, so a read that only needs to
//! know what versions exist still answers when the network is gone.
//!
//! Keyed by URL, kept under `meta/` with the rest of the derived files — every
//! entry is re-downloadable, and reclaiming the directory costs nothing but the
//! next fetch. The root is process-global for the same reason the client is:
//! the catalogue readers are provider impls that hold no aggregate reference.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use sha2::{Digest, Sha256};

static ROOT: OnceLock<PathBuf> = OnceLock::new();

/// Point the store at the data home. Set once at engine construction; the first
/// caller wins, since a daemon's data home never moves.
pub fn set_root(dir: PathBuf) {
    let _ = ROOT.set(dir);
}

pub fn save(url: &str, body: &str) {
    if let Some(dir) = ROOT.get() {
        save_in(dir, url, body);
    }
}

pub fn load(url: &str) -> Option<String> {
    load_in(ROOT.get()?, url)
}

fn entry_path(dir: &Path, url: &str) -> PathBuf {
    let digest: String = Sha256::digest(url.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    dir.join(format!("{digest}.json"))
}

fn save_in(dir: &Path, url: &str, body: &str) {
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    // Written through a temp and renamed, so a reader never sees a half-file
    // and a crash mid-write leaves the previous good copy in place.
    let path = entry_path(dir, url);
    let staging = path.with_extension("part");
    if std::fs::write(&staging, body).is_ok() && std::fs::rename(&staging, &path).is_err() {
        let _ = std::fs::remove_file(&staging);
    }
}

fn load_in(dir: &Path, url: &str) -> Option<String> {
    std::fs::read_to_string(entry_path(dir, url)).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_saved_body_reads_back_and_an_unseen_url_does_not() {
        let dir = tempfile::tempdir().expect("a temp dir");

        save_in(dir.path(), "https://example.test/versions", r#"{"v":[]}"#);
        assert_eq!(
            load_in(dir.path(), "https://example.test/versions").as_deref(),
            Some(r#"{"v":[]}"#)
        );
        assert!(load_in(dir.path(), "https://example.test/never-fetched").is_none());
    }

    #[test]
    fn a_second_save_replaces_the_first_and_leaves_no_temp_behind() {
        let dir = tempfile::tempdir().expect("a temp dir");

        save_in(dir.path(), "https://example.test/catalogue", "old");
        save_in(dir.path(), "https://example.test/catalogue", "new");
        assert_eq!(
            load_in(dir.path(), "https://example.test/catalogue").as_deref(),
            Some("new")
        );

        let leftovers = std::fs::read_dir(dir.path())
            .expect("the dir exists")
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "part"))
            .count();
        assert_eq!(leftovers, 0, "a .part survived the rename");
    }

    #[test]
    fn two_urls_never_share_an_entry() {
        let dir = tempfile::tempdir().expect("a temp dir");
        assert_ne!(
            entry_path(dir.path(), "https://example.test/a"),
            entry_path(dir.path(), "https://example.test/b")
        );
    }
}
