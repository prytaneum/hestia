//! Per-user data-directory resolution — the single source of truth for "where
//! Hestia's data lives", linked by the daemon (via the engine) and every client.

use std::fs;
use std::path::{Path, PathBuf};

fn env_path(name: &str) -> Option<PathBuf> {
    match std::env::var_os(name) {
        Some(v) if !v.is_empty() => Some(PathBuf::from(v)),
        _ => None,
    }
}

/// The root of the layout holding `dir`. A shipped build splits its binaries
/// between the root (shell, tray) and `bin/` (CLI, daemon); `deps/` is where
/// cargo puts test binaries, which must agree with the ones they exercise.
fn layout_root(dir: &Path) -> &Path {
    use std::ffi::OsStr;

    match dir.file_name() {
        Some(name) if name == OsStr::new("bin") || name == OsStr::new("deps") => {
            dir.parent().unwrap_or(dir)
        }
        _ => dir,
    }
}

/// The layout root of the running build — the directory a shipped install is
/// rooted at, stepping out of `bin/` when that is where this binary sits.
pub fn install_root() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    Some(layout_root(exe.parent()?).to_path_buf())
}

/// Locate another binary of this build: beside the running one, at the layout
/// root, or in the root's `bin/`. `names` are tried in order.
pub fn sibling_binary(names: &[&str]) -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?.to_path_buf();
    let root = layout_root(&dir).to_path_buf();
    names.iter().find_map(|name| {
        [dir.join(name), root.join(name), root.join("bin").join(name)]
            .into_iter()
            .find(|candidate| candidate.is_file())
    })
}

/// A `data/` directory inside the build's own layout — the portable archives
/// and every debug build. Anchored on the layout root, never the executable's
/// own directory, so every binary of one build agrees on one home.
#[cfg(any(feature = "portable", debug_assertions))]
fn contained_data_home() -> Option<PathBuf> {
    Some(install_root()?.join("data"))
}

/// A short, stable tag for a path. A per-install name must fit inside a unix
/// socket path (~108 bytes) and a pipe name, so it carries a hash rather than
/// the path. FNV-1a, not `DefaultHasher`, whose output is unstable across Rust
/// releases.
pub fn path_tag(path: &Path) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in path.to_string_lossy().as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// Which install the running build is, for anything that must not be shared
/// with a different one — the daemon endpoint above all. A build carrying its
/// own data directory (the portable archives, every debug build) is tagged by
/// that directory; `None` is a build using the per-user platform directory,
/// where there is nothing to tell apart.
#[cfg(any(feature = "portable", debug_assertions))]
pub fn install_scope() -> Option<String> {
    install_root().map(|root| scope_of(&root))
}

/// Canonical: Windows reports the exe path in whatever case and form it was launched by.
#[cfg(any(feature = "portable", debug_assertions))]
fn scope_of(root: &Path) -> String {
    let root = fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    path_tag(&root.join("data"))
}

#[cfg(not(any(feature = "portable", debug_assertions)))]
pub fn install_scope() -> Option<String> {
    None
}

/// The platform default data directory. Portable and debug builds keep their
/// data inside the build's own layout, so neither ever touches the real
/// per-user directory.
fn platform_data_home() -> PathBuf {
    #[cfg(any(feature = "portable", debug_assertions))]
    {
        if let Some(dir) = contained_data_home() {
            return dir;
        }
    }
    #[cfg(windows)]
    {
        if let Some(appdata) = env_path("APPDATA") {
            return appdata.join("Hestia");
        }
        if let Some(profile) = env_path("USERPROFILE") {
            return profile.join("AppData").join("Roaming").join("Hestia");
        }
    }
    #[cfg(not(windows))]
    {
        if let Some(home) = env_path("HOME") {
            return home.join(".hestia");
        }
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

/// The fixed anchor directory: never redirected. Holds the persisted-home pointer
/// and is the default data directory when nothing else is configured.
pub fn anchor_dir() -> PathBuf {
    platform_data_home()
}

fn pointer_file() -> PathBuf {
    platform_data_home().join("home")
}

fn read_pointer() -> Option<PathBuf> {
    let contents = fs::read_to_string(pointer_file()).ok()?;
    for line in contents.lines() {
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            return Some(PathBuf::from(trimmed));
        }
    }
    None
}

/// Resolve the data directory. Precedence: `override_dir` → `$HESTIA_HOME` → the
/// persisted-home pointer → the platform default.
pub fn data_home(override_dir: Option<&Path>) -> PathBuf {
    if let Some(dir) = override_dir {
        if !dir.as_os_str().is_empty() {
            return dir.to_path_buf();
        }
    }
    if let Some(env) = env_path("HESTIA_HOME") {
        return env;
    }
    if let Some(pointer) = read_pointer() {
        return pointer;
    }
    platform_data_home()
}

/// Persist `dir` as the default data directory for future runs. An empty path
/// removes the pointer, reverting to the platform default.
pub fn set_persisted_home(dir: &Path) -> std::io::Result<()> {
    let pointer = pointer_file();
    if dir.as_os_str().is_empty() {
        match fs::remove_file(&pointer) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    } else {
        if let Some(parent) = pointer.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&pointer, format!("{}\n", dir.display()))
    }
}

/// The config file within the resolved data directory.
pub fn config_path(override_dir: Option<&Path>) -> PathBuf {
    data_home(override_dir).join("config.json")
}

/// The directory holding Hestia's own logs, within the resolved data directory.
pub fn log_dir(override_dir: Option<&Path>) -> PathBuf {
    data_home(override_dir).join("logs")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_binary_of_one_layout_agrees_on_the_root() {
        let root = Path::new("/opt/hestia");
        // The shell and the tray sit at the root, the CLI and the daemon below
        // it, and cargo's test binaries a level down from the profile dir.
        for dir in [root, &root.join("bin"), &root.join("deps")] {
            assert_eq!(layout_root(dir), root, "{}", dir.display());
        }
    }

    #[test]
    fn an_ordinary_directory_name_is_the_root_itself() {
        let dir = Path::new("/opt/hestia/binaries");
        assert_eq!(layout_root(dir), dir);
    }

    #[test]
    fn a_contained_build_is_scoped_by_the_root_of_the_data_home_it_resolves() {
        let root = anchor_dir().parent().unwrap().to_path_buf();
        assert_eq!(install_scope(), Some(scope_of(&root)));
    }

    #[test]
    fn every_spelling_of_one_root_is_one_scope() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("bin")).unwrap();
        let detour = root.path().join("bin").join("..");
        assert_eq!(scope_of(root.path()), scope_of(&detour));
    }
}
