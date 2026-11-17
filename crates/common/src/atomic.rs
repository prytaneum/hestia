//! Write-through-a-temp-then-rename, so a failure or crash leaves the previous
//! file intact. Every staged name ends in [`SUFFIX`], which the engine's
//! startup reclaim sweeps.

use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::fsys::{self, Entry};

pub const SUFFIX: &str = ".part";

/// `<file name>.part` beside `path`; appended, never substituted, so `a.dat`
/// and `a.json` cannot share a staging file.
pub fn staged(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(SUFFIX);
    path.with_file_name(name)
}

pub fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    commit(path, bytes, false)
}

/// [`write`], owner-only (see [`fsys::restrict`]). The staged file is made
/// fresh and restricted while still empty, so the contents are never readable
/// by others.
pub fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    commit(path, bytes, true)
}

fn commit(path: &Path, bytes: &[u8], private: bool) -> io::Result<()> {
    let part = staged(path);
    let result = stage(&part, bytes, private).and_then(|()| fsys::rename(&part, path));
    if result.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    result
}

/// A leftover staging file may carry looser permissions, which an open of an
/// existing file would keep, so it is removed and the file created anew.
fn stage(part: &Path, bytes: &[u8], private: bool) -> io::Result<()> {
    match fsys::remove_file(part) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
        _ => {}
    }
    let mut file = OpenOptions::new().write(true).create_new(true).open(part)?;
    if private {
        fsys::restrict(part, Entry::File)?;
    }
    file.write_all(bytes)?;
    file.sync_all()
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
    fn owner_only(path: &Path, mode: u32) -> bool {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .expect("exists")
            .permissions()
            .mode()
            & 0o777
            == mode
    }

    #[cfg(windows)]
    fn owner_only(path: &Path, _: u32) -> bool {
        let shown = std::process::Command::new("icacls")
            .arg(path)
            .output()
            .expect("icacls runs");
        let shown = String::from_utf8_lossy(&shown.stdout).to_string();
        !shown.contains("(I)") && !shown.contains("Users") && !shown.contains("Everyone")
    }

    #[test]
    fn private_files_and_directories_are_owner_only_whatever_was_there_before() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let path = dir.path().join("accounts.json");
        std::fs::write(staged(&path), b"stale").expect("a stale staging file");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(staged(&path), std::fs::Permissions::from_mode(0o644))
                .expect("loosened");
        }
        write_private(&path, b"secret").expect("written");
        assert_eq!(std::fs::read(&path).expect("read back"), b"secret");
        assert!(owner_only(&path, 0o600));
        assert!(!staged(&path).exists());

        let private = dir.path().join("cache").join("plugins");
        fsys::create_private_dir(&private).expect("created");
        assert!(owner_only(&private, 0o700));
        let inside = private.join("entry");
        write_private(&inside, b"x").expect("written inside");
        assert!(owner_only(&inside, 0o600));
    }
}
