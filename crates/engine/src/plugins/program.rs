//! How a declared program becomes a path the host will execute: a `PATH` name
//! pinned (path and content hash) when the grant is approved, a managed Java of
//! a major the call picks, or a data-directory file copied into a host-owned
//! directory and verified there
//! ([0101](../../../../docs/decisions/0101-plugins-spawn-only-declared-programs.md),
//! [0118](../../../../docs/decisions/0118-a-programs-identity-is-separate-from-its-invocations.md)).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use proto::minecraft::Requirement;
use proto::plugin::{Grant, Manifest, Program, ProgramPin};
use sha2::Sha256;

use crate::checksum::digest_file;
use crate::minecraft::Prerequisite;

#[cfg(windows)]
const EXE_SUFFIX: &str = ".exe";
#[cfg(not(windows))]
const EXE_SUFFIX: &str = "";

/// The Java runtimes Hestia manages, as a plugin's `java = <major>` programs
/// see them. Handed to the plugin runtime at load, so the plugin code holds no
/// engine.
#[async_trait::async_trait]
pub trait ManagedJava: Send + Sync {
    /// The executable of the managed runtime of `major`, installing it first
    /// when it is missing.
    async fn executable(&self, major: u32) -> anyhow::Result<PathBuf>;
}

/// Stands in until a host lends real runtimes: every lookup fails.
pub(crate) struct NoManagedJava;

#[async_trait::async_trait]
impl ManagedJava for NoManagedJava {
    async fn executable(&self, major: u32) -> anyhow::Result<PathBuf> {
        anyhow::bail!("this host manages no Java runtimes (asked for Java {major})")
    }
}

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

/// Program id to where its `PATH` name was resolved and the hash of the file.
pub type Pins = BTreeMap<String, ProgramPin>;

/// How a `PATH` name becomes a pin; a plugin store holds one so tests need not
/// read the real `PATH`.
pub type Resolver = fn(&str) -> Option<ProgramPin>;

/// Resolves `name` on `PATH` and hashes the file found there.
pub fn pin_on_path(name: &str) -> Option<ProgramPin> {
    let path = resolve_on_path(name)?;
    match digest_file::<Sha256>(&path) {
        Ok(sha256) => Some(ProgramPin {
            path: path.to_string_lossy().into_owned(),
            sha256,
        }),
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "cannot hash a program found on PATH");
            None
        }
    }
}

/// The digest a user approves and a record stores: the grant together with the
/// path and hash each `PATH` program is pinned to. Preview, install, update and
/// load all derive it here, so none of them can disagree about what was approved.
pub fn approval_digest(grant: &Grant, pins: &Pins) -> String {
    grant.approval_digest(pins)
}

fn path_programs(manifest: &Manifest) -> impl Iterator<Item = &Program> {
    manifest.programs.iter().filter(|p| p.on_path())
}

/// The ids of programs `manifest` declares that `installed` pinned and `fresh`
/// would pin elsewhere, to a file with other content, or not at all.
pub fn changed_pins(installed: &Pins, fresh: &Pins, manifest: &Manifest) -> Vec<String> {
    path_programs(manifest)
        .filter_map(|program| {
            let kept = installed.get(&program.id)?;
            (fresh.get(&program.id) != Some(kept)).then(|| program.id.clone())
        })
        .collect()
}

/// What an update without a new approval keeps: the installed pins of the
/// programs it still declares. A program not pinned before stays unpinned.
pub fn carried_pins(installed: &Pins, manifest: &Manifest) -> Pins {
    path_programs(manifest)
        .filter_map(|program| {
            let kept = installed.get(&program.id)?;
            Some((program.id.clone(), kept.clone()))
        })
        .collect()
}

/// The pin each `PATH` program of `manifest` resolves to now, at approval: the
/// only file that program will ever run. One not found is left out, unpinned:
/// it cannot run until the plugin is approved again with the program present.
pub fn pin_programs(plugin: &str, manifest: &Manifest, resolve: Resolver) -> Pins {
    let mut pinned = BTreeMap::new();
    for program in path_programs(manifest) {
        match resolve(&program.command) {
            Some(pin) => {
                tracing::info!(plugin, program = %program.id, path = %pin.path, "program pinned");
                pinned.insert(program.id.clone(), pin);
            }
            None => tracing::warn!(
                plugin,
                program = %program.id,
                "program not found on PATH; it cannot run until the plugin is approved again with it installed"
            ),
        }
    }
    pinned
}

/// What the user is told is missing for `program`: its declared requirement,
/// or its command when it declares none.
pub fn requirement_of(program: &Program) -> Requirement {
    program.requires.clone().unwrap_or_else(|| Requirement {
        name: program.identity_label(),
        url: String::new(),
    })
}

/// What a flavor that requires the programs `ids` needs on this computer. Only
/// a `PATH` program can be unmet: a managed Java is installed on demand and a
/// data file is checked when it is staged, so those name nothing to check. An
/// id the plugin is not granted to run is an error, since the flavor could
/// never run it.
pub fn prerequisites(grant: &Grant, ids: &[String]) -> Result<Vec<Prerequisite>, String> {
    let mut required = Vec::new();
    for id in ids {
        let program = grant.program(id).ok_or_else(|| {
            format!("requires `{id}`, which is not a program the plugin is granted to run")
        })?;
        if program.on_path() {
            let requirement = requirement_of(program);
            required.push(Prerequisite {
                command: program.command.clone().into(),
                name: requirement.name.into(),
                url: requirement.url.into(),
            });
        }
    }
    Ok(required)
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use proto::plugin::{Permission, PermissionScope, ProgramJava};

    use super::*;
    use crate::minecraft::unmet_by;

    fn git() -> Program {
        Program {
            id: "git".into(),
            command: "git".into(),
            args: vec!["**".into()],
            requires: Some(Requirement {
                name: "Git".into(),
                url: "https://git-scm.com/downloads".into(),
            }),
            ..Program::default()
        }
    }

    fn granting(programs: Vec<Program>) -> Grant {
        Grant {
            permissions: BTreeMap::from([(Permission::ProcessSpawn, PermissionScope::default())]),
            programs: programs
                .into_iter()
                .map(|program| (program.id.clone(), program))
                .collect(),
            ..Grant::default()
        }
    }

    #[test]
    fn a_flavor_requires_the_path_programs_it_names() {
        let java = Program {
            id: "jvm".into(),
            command: String::new(),
            java: Some(ProgramJava::Major(21)),
            ..Program::default()
        };
        let grant = granting(vec![git(), java]);

        let required = prerequisites(&grant, &["git".into(), "jvm".into()]).unwrap();

        assert_eq!(required.len(), 1, "a managed Java is installed on demand");
        assert_eq!(required[0].command, "git");
        assert_eq!(required[0].name, "Git");
        assert_eq!(required[0].url, "https://git-scm.com/downloads");
        assert!(prerequisites(&grant, &["nope".into()]).is_err());
        assert!(prerequisites(&Grant::default(), &["git".into()]).is_err());
    }

    #[test]
    fn a_requirement_is_met_or_missing_by_what_the_resolver_finds() {
        let required = prerequisites(&granting(vec![git()]), &["git".into()]).unwrap();

        assert!(unmet_by(&required, |_| true).is_empty());
        let missing = unmet_by(&required, |_| false);
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].name, "Git");
        assert_eq!(missing[0].url, "https://git-scm.com/downloads");
    }

    #[test]
    fn a_program_without_a_declared_requirement_is_named_by_its_command() {
        let bare = Program {
            requires: None,
            ..git()
        };
        let requirement = requirement_of(&bare);
        assert_eq!(requirement.name, "git");
        assert_eq!(requirement.url, "");
    }
}
