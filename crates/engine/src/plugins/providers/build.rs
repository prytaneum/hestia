//! Placing what a flavor's `build` produced. The guest writes only under its
//! build area; the host copies each output it names: a file to the place the
//! profile gave it, a directory tree into one of a fixed set of destinations.
//! What a plugin builds never lands anywhere else.

use std::path::{Component, Path, PathBuf};

use anyhow::Context;
use proto::plugin::BUILD_MOUNT;
use proto::provider::flavor::{BuildOutput, Destination};

use crate::cancel::{Cancel, Cancelled};

const LOCK_FILE: &str = ".lock";
const CANCEL_POLL: std::time::Duration = std::time::Duration::from_millis(50);

/// An exclusive hold on one flavor's build area, released when dropped. It
/// spans processes, so a second build of the flavor under the same data home
/// waits. The lock file is bookkeeping: outputs are placed only by name.
pub(super) struct AreaLock {
    _file: std::fs::File,
}

/// Creates `area` and waits for its lock, giving up with [`Cancelled`] when
/// `cancel` is raised. The wait is a blocking thread, which on cancel finishes
/// acquiring and releases at once.
pub(super) async fn lock_area(area: &Path, cancel: &Cancel) -> anyhow::Result<AreaLock> {
    std::fs::create_dir_all(area).with_context(|| format!("cannot create {}", area.display()))?;
    let path = area.join(LOCK_FILE);
    let mut waiting = tokio::task::spawn_blocking(move || {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)?;
        file.lock()?;
        std::io::Result::Ok(AreaLock { _file: file })
    });
    loop {
        tokio::select! {
            held = &mut waiting => {
                return held?.with_context(|| format!("cannot lock {}", area.display()));
            }
            () = tokio::time::sleep(CANCEL_POLL) => {
                if cancel.is_cancelled() {
                    return Err(Cancelled.into());
                }
            }
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub(super) enum Fault {
    #[error("output `{0}` is not a file under the build area")]
    Outside(String),
    #[error("output `{0}` is a symlink or leaves the build area")]
    Escapes(String),
    #[error("output `{0}` is not a regular file")]
    NotFile(String),
    #[error("output `{0}` is not a directory")]
    NotDirectory(String),
    #[error("output `{0}` holds something that is neither a file nor a directory")]
    Special(String),
    #[error("output `{0}` has no destination here")]
    Destination(String),
    #[error("output target `{0}` is not a relative path")]
    Relative(String),
    #[error("output `{0}` does not exist")]
    Absent(String),
    #[error("output target `{0}` is not a file the profile builds")]
    Target(String),
    #[error("the build did not produce `{0}`")]
    Missing(String),
    #[error("cannot place `{target}`: {source}")]
    Io {
        target: String,
        source: std::io::Error,
    },
}

/// A file the profile names as built and where it must end up.
pub(super) struct Expected {
    pub target: String,
    pub destination: PathBuf,
}

/// The directories a tree output may be placed in; `entry` is absent where the
/// side has no data directory of its own.
pub(super) struct Roots {
    pub entry: Option<PathBuf>,
    pub libraries: PathBuf,
}

impl Roots {
    fn of(&self, destination: Destination) -> Option<&Path> {
        match destination {
            Destination::Entry => self.entry.as_deref(),
            Destination::Libraries => Some(&self.libraries),
        }
    }
}

/// Places each output, then checks that every expected file exists. A file goes
/// to the destination of the target it names; a tree goes under its
/// destination root. Blocking.
pub(super) fn place(
    build_dir: &Path,
    outputs: &[BuildOutput],
    expected: &[Expected],
    roots: &Roots,
) -> Result<(), Fault> {
    let root = build_dir.canonicalize().map_err(|source| Fault::Io {
        target: BUILD_MOUNT.to_string(),
        source,
    })?;
    for output in outputs {
        match output.destination {
            Some(destination) => place_tree(build_dir, &root, output, destination, roots)?,
            None => place_file(build_dir, &root, output, expected)?,
        }
    }
    match expected.iter().find(|slot| !slot.destination.is_file()) {
        Some(slot) => Err(Fault::Missing(slot.target.clone())),
        None => Ok(()),
    }
}

fn place_file(
    build_dir: &Path,
    root: &Path,
    output: &BuildOutput,
    expected: &[Expected],
) -> Result<(), Fault> {
    let slot = expected
        .iter()
        .find(|slot| slot.target == output.target)
        .ok_or_else(|| Fault::Target(output.target.clone()))?;
    let source = source_file(build_dir, root, &output.path)?;
    copy_into(&source, &slot.destination).map_err(|source| Fault::Io {
        target: slot.target.clone(),
        source,
    })
}

fn place_tree(
    build_dir: &Path,
    root: &Path,
    output: &BuildOutput,
    destination: Destination,
    roots: &Roots,
) -> Result<(), Fault> {
    let base = roots
        .of(destination)
        .ok_or_else(|| Fault::Destination(output.path.clone()))?;
    let target = Path::new(&output.target);
    if target.is_absolute()
        || target
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(Fault::Relative(output.target.clone()));
    }
    let source = source_directory(build_dir, root, &output.path)?;
    copy_tree(&source, &base.join(target), &output.path).map_err(|fault| match fault {
        TreeFault::Refused(fault) => fault,
        TreeFault::Io(source) => Fault::Io {
            target: output.target.clone(),
            source,
        },
    })
}

/// The entry the guest path `guest` names under the build area, as a canonical
/// path inside `root`, with its metadata.
fn source_entry(
    build_dir: &Path,
    root: &Path,
    guest: &str,
) -> Result<(PathBuf, std::fs::Metadata), Fault> {
    let outside = || Fault::Outside(guest.to_string());
    let relative = Path::new(guest)
        .strip_prefix(BUILD_MOUNT)
        .map_err(|_| outside())?;
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(outside());
    }
    let path = build_dir.join(relative);
    let metadata =
        std::fs::symlink_metadata(&path).map_err(|_| Fault::Absent(guest.to_string()))?;
    if metadata.file_type().is_symlink() {
        return Err(Fault::Escapes(guest.to_string()));
    }
    let real = path
        .canonicalize()
        .map_err(|_| Fault::Absent(guest.to_string()))?;
    if !real.starts_with(root) {
        return Err(Fault::Escapes(guest.to_string()));
    }
    Ok((real, metadata))
}

/// The regular file the guest path `guest` names, canonical and inside `root`.
fn source_file(build_dir: &Path, root: &Path, guest: &str) -> Result<PathBuf, Fault> {
    let (real, metadata) = source_entry(build_dir, root, guest)?;
    metadata
        .is_file()
        .then_some(real)
        .ok_or_else(|| Fault::NotFile(guest.to_string()))
}

/// The directory the guest path `guest` names, canonical and inside `root`.
fn source_directory(build_dir: &Path, root: &Path, guest: &str) -> Result<PathBuf, Fault> {
    let (real, metadata) = source_entry(build_dir, root, guest)?;
    metadata
        .is_dir()
        .then_some(real)
        .ok_or_else(|| Fault::NotDirectory(guest.to_string()))
}

enum TreeFault {
    Refused(Fault),
    Io(std::io::Error),
}

impl From<std::io::Error> for TreeFault {
    fn from(error: std::io::Error) -> Self {
        TreeFault::Io(error)
    }
}

/// Copies `source` into `destination` file by file. A symlink or a special file
/// anywhere in the tree refuses the whole output; a file already at its place
/// is left as it is, so placing a tree twice, or over a directory that holds
/// the user's own files, changes nothing that exists.
fn copy_tree(source: &Path, destination: &Path, guest: &str) -> Result<(), TreeFault> {
    std::fs::create_dir_all(destination)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let into = destination.join(entry.file_name());
        if kind.is_symlink() {
            return Err(TreeFault::Refused(Fault::Escapes(guest.to_string())));
        } else if kind.is_dir() {
            copy_tree(&entry.path(), &into, guest)?;
        } else if kind.is_file() {
            copy_new(&entry.path(), &into)?;
        } else {
            return Err(TreeFault::Refused(Fault::Special(guest.to_string())));
        }
    }
    Ok(())
}

/// Stages beside `destination` and renames, unless something is already there.
fn copy_new(source: &Path, destination: &Path) -> std::io::Result<()> {
    if std::fs::symlink_metadata(destination).is_ok() {
        return Ok(());
    }
    stage(source, destination)
}

/// Stages beside `destination` and renames, so a failure leaves nothing half
/// written. A destination already the same length is left alone: a client
/// install runs on every launch, and the libraries it feeds are named by version.
///
/// shortcut: length, not a hash, decides "already placed"; hash it when a built
/// file can change without changing size under one name.
pub(super) fn copy_into(source: &Path, destination: &Path) -> std::io::Result<()> {
    let length = std::fs::metadata(source)?.len();
    if std::fs::metadata(destination).is_ok_and(|placed| placed.is_file() && placed.len() == length)
    {
        return Ok(());
    }
    stage(source, destination)
}

fn stage(source: &Path, destination: &Path) -> std::io::Result<()> {
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let name = destination
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let partial = destination.with_file_name(format!(".{name}.part"));
    let staged =
        std::fs::copy(source, &partial).and_then(|_| std::fs::rename(&partial, destination));
    if staged.is_err() {
        let _ = std::fs::remove_file(&partial);
    }
    staged
}

#[cfg(test)]
mod tests {
    use super::*;

    fn output(path: &str, target: &str) -> BuildOutput {
        BuildOutput {
            path: path.into(),
            target: target.into(),
            destination: None,
        }
    }

    fn tree(path: &str, target: &str, destination: Destination) -> BuildOutput {
        BuildOutput {
            destination: Some(destination),
            ..output(path, target)
        }
    }

    fn roots(area: &Area) -> Roots {
        Roots {
            entry: Some(area.served.clone()),
            libraries: area.served.join("libraries"),
        }
    }

    struct Area {
        _home: tempfile::TempDir,
        build: PathBuf,
        served: PathBuf,
    }

    fn area() -> Area {
        let home = tempfile::tempdir().unwrap();
        let build = home.path().join("build");
        let served = home.path().join("served");
        std::fs::create_dir_all(&build).unwrap();
        std::fs::create_dir_all(&served).unwrap();
        Area {
            _home: home,
            build,
            served,
        }
    }

    fn expecting(area: &Area) -> Vec<Expected> {
        vec![Expected {
            target: "server.jar".into(),
            destination: area.served.join("server.jar"),
        }]
    }

    #[test]
    fn an_output_is_copied_to_the_place_the_profile_named() {
        let area = area();
        std::fs::write(area.build.join("out.jar"), b"jar").unwrap();
        place(
            &area.build,
            &[output("/build/out.jar", "server.jar")],
            &expecting(&area),
            &roots(&area),
        )
        .unwrap();
        assert_eq!(
            std::fs::read(area.served.join("server.jar")).unwrap(),
            b"jar"
        );
        assert!(!area.served.join(".server.jar.part").exists());
    }

    #[test]
    fn an_output_outside_the_build_area_or_aimed_at_another_file_is_refused() {
        let area = area();
        std::fs::write(area.build.join("out.jar"), b"jar").unwrap();
        for (path, target) in [
            ("/data/out.jar", "server.jar"),
            ("/build/../out.jar", "server.jar"),
            ("/build", "server.jar"),
            ("out.jar", "server.jar"),
            ("/build/out.jar", "evil.jar"),
        ] {
            assert!(
                place(
                    &area.build,
                    &[output(path, target)],
                    &expecting(&area),
                    &roots(&area),
                )
                .is_err(),
                "{path} -> {target}"
            );
        }
        assert!(!area.served.join("server.jar").exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_output_is_refused() {
        let area = area();
        let secret = area.served.join("secret");
        std::fs::write(&secret, b"secret").unwrap();
        std::os::unix::fs::symlink(&secret, area.build.join("out.jar")).unwrap();
        let refused = place(
            &area.build,
            &[output("/build/out.jar", "server.jar")],
            &expecting(&area),
            &roots(&area),
        );
        assert!(matches!(refused, Err(Fault::Escapes(_))));

        std::fs::create_dir_all(area.build.join("real")).unwrap();
        std::os::unix::fs::symlink(&area.served, area.build.join("dir")).unwrap();
        let through_dir = place(
            &area.build,
            &[output("/build/dir/secret", "server.jar")],
            &expecting(&area),
            &roots(&area),
        );
        assert!(matches!(through_dir, Err(Fault::Escapes(_))));
    }

    #[test]
    fn a_built_file_nothing_produced_is_missing() {
        let area = area();
        let missing = place(&area.build, &[], &expecting(&area), &roots(&area));
        assert!(matches!(missing, Err(Fault::Missing(target)) if target == "server.jar"));
        std::fs::write(area.served.join("server.jar"), b"already there").unwrap();
        place(&area.build, &[], &expecting(&area), &roots(&area)).unwrap();
    }

    fn write(path: PathBuf, contents: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }

    #[test]
    fn a_tree_is_placed_under_its_destination_and_target() {
        let area = area();
        write(area.build.join("out/net/loader/1/loader-1.jar"), "jar");
        write(area.build.join("out/unix_args.txt"), "args");
        place(
            &area.build,
            &[
                tree("/build/out", "libraries", Destination::Entry),
                tree("/build/out/net", "", Destination::Libraries),
            ],
            &[],
            &roots(&area),
        )
        .unwrap();
        assert_eq!(
            std::fs::read(area.served.join("libraries/unix_args.txt")).unwrap(),
            b"args"
        );
        assert_eq!(
            std::fs::read(area.served.join("libraries/net/loader/1/loader-1.jar")).unwrap(),
            b"jar"
        );
        assert!(!area.served.join("libraries/unix_args.txt.part").exists());
    }

    #[test]
    fn a_tree_never_replaces_a_file_that_exists() {
        let area = area();
        write(area.build.join("out/server.properties"), "built");
        write(area.build.join("out/fresh.txt"), "built");
        write(area.served.join("server.properties"), "the user's");
        let placed = || {
            place(
                &area.build,
                &[tree("/build/out", "", Destination::Entry)],
                &[],
                &roots(&area),
            )
        };
        placed().unwrap();
        placed().unwrap();
        assert_eq!(
            std::fs::read(area.served.join("server.properties")).unwrap(),
            b"the user's"
        );
        assert_eq!(
            std::fs::read(area.served.join("fresh.txt")).unwrap(),
            b"built"
        );
    }

    #[test]
    fn a_tree_output_that_is_misaimed_is_refused() {
        let area = area();
        write(area.build.join("out/a.txt"), "a");
        write(area.build.join("file.txt"), "f");
        let no_entry = Roots {
            entry: None,
            ..roots(&area)
        };
        let refused =
            |output, roots: &Roots| place(&area.build, &[output], &[], roots).unwrap_err();

        assert!(matches!(
            refused(tree("/build/out", "", Destination::Entry), &no_entry),
            Fault::Destination(_)
        ));
        for target in ["../up", "/abs", "a/../../b"] {
            assert!(
                matches!(
                    refused(
                        tree("/build/out", target, Destination::Entry),
                        &roots(&area)
                    ),
                    Fault::Relative(_)
                ),
                "{target}"
            );
        }
        assert!(matches!(
            refused(
                tree("/build/file.txt", "", Destination::Entry),
                &roots(&area)
            ),
            Fault::NotDirectory(_)
        ));
        assert!(matches!(
            refused(tree("/build/../out", "", Destination::Entry), &roots(&area)),
            Fault::Outside(_)
        ));
        assert!(matches!(
            refused(output("/build/out", "server.jar"), &roots(&area)),
            Fault::Target(_)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn a_tree_holding_a_symlink_is_refused_whole() {
        let area = area();
        let secret = area.served.join("secret");
        write(secret.clone(), "secret");
        write(area.build.join("out/a.txt"), "a");
        std::os::unix::fs::symlink(&secret, area.build.join("out/link")).unwrap();
        let refused = place(
            &area.build,
            &[tree("/build/out", "t", Destination::Entry)],
            &[],
            &roots(&area),
        );
        assert!(matches!(refused, Err(Fault::Escapes(_))));
    }
}
