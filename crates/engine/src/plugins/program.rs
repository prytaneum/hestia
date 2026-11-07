//! How a declared program becomes a path the host will execute: a `PATH` name
//! pinned when the grant is approved, or a data-directory file copied into a
//! host-owned directory and verified there
//! ([0101](../../../../docs/decisions/0101-plugins-spawn-only-declared-programs.md)).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use proto::plugin::{Manifest, Program};
use sha2::Sha256;

use crate::checksum::digest_file;

#[cfg(windows)]
const EXE_SUFFIX: &str = ".exe";
#[cfg(not(windows))]
const EXE_SUFFIX: &str = "";

/// One copy at a time, so two calls staging the same program cannot interleave.
static STAGING: Mutex<()> = Mutex::new(());

#[derive(Debug, thiserror::Error)]
pub enum StageError {
    #[error("the program declares no sha256 for this platform ({0})")]
    NoHashForTarget(String),
    #[error("the program file `{0}` is missing from the plugin's data")]
    Missing(String),
    #[error("the program file does not match its declared sha256")]
    HashMismatch,
    #[error("cannot prepare the program: {0}")]
    Io(#[from] std::io::Error),
}

/// The Rust target triple this build runs on, the key of a program's `sha256`.
pub fn target_triple() -> String {
    let arch = std::env::consts::ARCH;
    match std::env::consts::OS {
        "linux" if cfg!(target_env = "musl") => format!("{arch}-unknown-linux-musl"),
        "linux" => format!("{arch}-unknown-linux-gnu"),
        "windows" if cfg!(target_env = "gnu") => format!("{arch}-pc-windows-gnu"),
        "windows" => format!("{arch}-pc-windows-msvc"),
        "macos" => format!("{arch}-apple-darwin"),
        os => format!("{arch}-unknown-{os}"),
    }
}

/// The first executable named `name` in a `PATH` directory. A relative
/// directory is skipped: it would resolve against wherever the daemon runs.
pub fn resolve_on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join(format!("{name}{EXE_SUFFIX}")))
        .find(|candidate| is_executable(candidate))
}

/// The absolute path each `PATH` program of `manifest` resolves to now, at
/// approval: the only path that program will ever run from. One not found is
/// left out, so it cannot run until the plugin is approved again.
pub fn pin_programs(plugin: &str, manifest: &Manifest) -> BTreeMap<String, PathBuf> {
    let mut pinned = BTreeMap::new();
    for program in manifest.programs.iter().filter(|p| !p.in_data_dir()) {
        match resolve_on_path(&program.command) {
            Some(path) => {
                tracing::info!(plugin, program = %program.id, path = %path.display(), "program pinned");
                pinned.insert(program.id.clone(), path);
            }
            None => tracing::warn!(
                plugin,
                program = %program.id,
                "program not found on PATH; it cannot run until the plugin is installed again"
            ),
        }
    }
    pinned
}

/// Whether `path` is a regular file the host may run.
pub fn is_executable(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        metadata.is_file()
    }
}

/// The host-owned copy of a data-directory program, verified against the hash
/// declared for `triple`. The copy is made from the plugin's `data/` only when
/// the host-owned one is absent or differs, then hashed *after* the copy and
/// renamed into place: the guest cannot write `bin_dir`, so what is checked is
/// what runs, and later edits to `data/` change nothing.
///
/// Blocking.
pub fn stage(
    data_dir: &Path,
    bin_dir: &Path,
    program: &Program,
    triple: &str,
) -> Result<PathBuf, StageError> {
    let expected = program
        .sha256
        .get(triple)
        .ok_or_else(|| StageError::NoHashForTarget(triple.to_string()))?;
    let _staging = STAGING.lock().unwrap();
    let target = bin_dir.join(format!("{}{EXE_SUFFIX}", program.id));
    if digest_file::<Sha256>(&target).is_ok_and(|actual| actual == *expected) {
        return Ok(target);
    }

    let source = data_file(data_dir, &program.command)
        .ok_or_else(|| StageError::Missing(program.command.clone()))?;
    std::fs::create_dir_all(bin_dir)?;
    let partial = bin_dir.join(format!(".{}.part", program.id));
    std::fs::copy(&source, &partial)?;
    let verified = digest_file::<Sha256>(&partial).map(|actual| actual == *expected);
    if !matches!(verified, Ok(true)) {
        let _ = std::fs::remove_file(&partial);
        return Err(verified.map_or_else(StageError::Io, |_| StageError::HashMismatch));
    }
    make_executable(&partial)?;
    std::fs::rename(&partial, &target)?;
    Ok(target)
}

/// A regular file at `relative` inside `data_dir`; on Windows the same name
/// with `.exe` is tried too, so one manifest serves both platforms. A symlink
/// is refused: it would make the host read outside `data/`.
fn data_file(data_dir: &Path, relative: &str) -> Option<PathBuf> {
    let plain = data_dir.join(relative);
    let mut candidates = vec![plain.clone()];
    if cfg!(windows) {
        candidates.push(plain.with_extension("exe"));
    }
    candidates.into_iter().find(|candidate| {
        std::fs::symlink_metadata(candidate).is_ok_and(|metadata| metadata.is_file())
    })
}

#[cfg(unix)]
fn make_executable(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> std::io::Result<()> {
    Ok(())
}
