//! Write-through-a-temp-then-rename, so a failure or crash leaves the previous
//! file intact. Every staged name ends in [`SUFFIX`], which the engine's
//! startup reclaim sweeps.

use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

pub const SUFFIX: &str = ".part";

/// `<file name>.part` beside `path`; appended, never substituted, so `a.dat`
/// and `a.json` cannot share a staging file.
pub fn staged(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(SUFFIX);
    path.with_file_name(name)
}

pub fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    commit(path, bytes, OpenOptions::new())
}

/// [`write`], owner-only where permissions exist. The mode is applied when the
/// staged file is created, so the contents are never readable by others.
pub fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let options = OpenOptions::new();
    #[cfg(unix)]
    let options = {
        let mut options = options;
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        options
    };
    commit(path, bytes, options)
}

fn commit(path: &Path, bytes: &[u8], mut options: OpenOptions) -> io::Result<()> {
    let part = staged(path);
    let result = options
        .create(true)
        .write(true)
        .truncate(true)
        .open(&part)
        .and_then(|mut file| file.write_all(bytes))
        .and_then(|()| std::fs::rename(&part, path));
    if result.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn staged_appends_to_the_full_name() {
        assert_eq!(staged(Path::new("d/a.json")), Path::new("d/a.json.part"));
        assert_ne!(staged(Path::new("a.dat")), staged(Path::new("a.json")));
    }

    #[cfg(unix)]
    #[test]
    fn a_private_write_is_owner_only_and_leaves_no_part() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("a temp dir");
        let path = dir.path().join("accounts.json");
        write_private(&path, b"secret").expect("written");
        let mode = std::fs::metadata(&path)
            .expect("exists")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
        assert!(!staged(&path).exists());
    }
}
