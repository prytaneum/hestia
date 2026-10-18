//! The three-way settle: each side is asked whether it moved since the
//! baseline, and the clock only breaks a tie. A missing side is never an edit.

use std::fs;
use std::path::Path;
use std::time::SystemTime;

use anyhow::{Context, Result};

pub fn one<'a, T: PartialEq>(
    base: Option<&T>,
    stored: Option<&'a T>,
    local: Option<&'a T>,
    data_newer: bool,
) -> Option<&'a T> {
    match (stored, local) {
        (Some(s), Some(d)) if s == d => Some(s),
        (Some(s), Some(d)) => match base {
            Some(b) if b == s => Some(d),
            Some(b) if b == d => Some(s),
            _ => Some(if data_newer { d } else { s }),
        },
        (Some(s), None) => Some(s),
        (None, other) => other,
    }
}

/// Records the instance's content as the agreement, so the next pass reads
/// every disagreement as the shared copy's change.
pub fn defer_to_store(baseline: &Path, data: &Path) -> Result<()> {
    write_if_changed(baseline, &read(data).unwrap_or_default())
}

/// The stamp is the tiebreak between two edited sides, so a copy carries its
/// source's rather than describing itself.
pub fn copy_file(from: &Path, to: &Path) -> Result<()> {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("cannot create {}", parent.display()))?;
    }
    fs::copy(from, to)
        .with_context(|| format!("cannot copy {} to {}", from.display(), to.display()))?;
    if let Some(time) = mtime(from) {
        fs::File::options()
            .write(true)
            .open(to)
            .and_then(|file| file.set_modified(time))
            .with_context(|| format!("cannot stamp {}", to.display()))?;
    }
    Ok(())
}

/// A rewrite that changes nothing would still stamp the file, and that stamp is
/// what every other instance's tiebreak reads.
pub fn write_if_changed(path: &Path, contents: &[u8]) -> Result<()> {
    if read(path).is_some_and(|current| current == contents) {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("cannot create {}", parent.display()))?;
    }
    common::atomic::write(path, contents)
        .with_context(|| format!("cannot write {}", path.display()))
}

pub fn read(path: &Path) -> Option<Vec<u8>> {
    fs::read(path).ok()
}

pub fn newer(a: &Path, b: &Path) -> bool {
    match (mtime(a), mtime(b)) {
        (Some(ta), Some(tb)) => ta >= tb,
        (Some(_), None) => true,
        _ => false,
    }
}

pub fn mtime(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|m| m.modified()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(text: &str) -> Option<String> {
        Some(text.to_string())
    }

    #[test]
    fn one_side_changing_wins_over_a_newer_file() {
        let base = value("2");
        let stored = value("2");
        let local = value("4");
        assert_eq!(
            one(base.as_ref(), stored.as_ref(), local.as_ref(), false),
            local.as_ref()
        );
    }

    #[test]
    fn both_changing_falls_back_to_the_clock() {
        let base = value("2");
        let stored = value("3");
        let local = value("4");
        assert_eq!(
            one(base.as_ref(), stored.as_ref(), local.as_ref(), true),
            local.as_ref()
        );
        assert_eq!(
            one(base.as_ref(), stored.as_ref(), local.as_ref(), false),
            stored.as_ref()
        );
    }

    #[test]
    fn what_only_one_side_knows_is_kept() {
        let only_store = value("on");
        assert_eq!(
            one(None, only_store.as_ref(), None, true),
            only_store.as_ref()
        );
        let only_data = value("off");
        assert_eq!(
            one(None, None, only_data.as_ref(), false),
            only_data.as_ref()
        );
        assert_eq!(one::<String>(None, None, None, true), None);
    }
}
