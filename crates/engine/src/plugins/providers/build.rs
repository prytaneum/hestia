//! Placing what a flavor's `build` produced. The guest writes only under its
//! build area; the host copies each output it names to the place the profile
//! gave that file, so what a plugin builds never lands anywhere the profile did
//! not already name.

use std::path::{Component, Path, PathBuf};

use proto::plugin::BUILD_MOUNT;
use proto::provider::flavor::BuildOutput;

#[derive(Debug, thiserror::Error)]
pub(super) enum Fault {
    #[error("output `{0}` is not a file under the build area")]
    Outside(String),
    #[error("output `{0}` is a symlink or leaves the build area")]
    Escapes(String),
    #[error("output `{0}` is not a regular file")]
    NotFile(String),
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

/// Copies each output to the destination of the target it names, then checks
/// that every expected file exists. Blocking.
pub(super) fn place(
    build_dir: &Path,
    outputs: &[BuildOutput],
    expected: &[Expected],
) -> Result<(), Fault> {
    let root = build_dir.canonicalize().map_err(|source| Fault::Io {
        target: BUILD_MOUNT.to_string(),
        source,
    })?;
    for output in outputs {
        let slot = expected
            .iter()
            .find(|slot| slot.target == output.target)
            .ok_or_else(|| Fault::Target(output.target.clone()))?;
        let source = source_file(build_dir, &root, &output.path)?;
        copy_into(&source, &slot.destination).map_err(|source| Fault::Io {
            target: slot.target.clone(),
            source,
        })?;
    }
    match expected.iter().find(|slot| !slot.destination.is_file()) {
        Some(slot) => Err(Fault::Missing(slot.target.clone())),
        None => Ok(()),
    }
}

/// The regular file the guest path `guest` names under the build area, as a
/// canonical path inside `root`.
fn source_file(build_dir: &Path, root: &Path, guest: &str) -> Result<PathBuf, Fault> {
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
    if !metadata.is_file() {
        return Err(Fault::NotFile(guest.to_string()));
    }
    let real = path
        .canonicalize()
        .map_err(|_| Fault::Absent(guest.to_string()))?;
    if !real.starts_with(root) {
        return Err(Fault::Escapes(guest.to_string()));
    }
    Ok(real)
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
                place(&area.build, &[output(path, target)], &expecting(&area)).is_err(),
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
        );
        assert!(matches!(refused, Err(Fault::Escapes(_))));

        std::fs::create_dir_all(area.build.join("real")).unwrap();
        std::os::unix::fs::symlink(&area.served, area.build.join("dir")).unwrap();
        let through_dir = place(
            &area.build,
            &[output("/build/dir/secret", "server.jar")],
            &expecting(&area),
        );
        assert!(matches!(through_dir, Err(Fault::Escapes(_))));
    }

    #[test]
    fn a_built_file_nothing_produced_is_missing() {
        let area = area();
        let missing = place(&area.build, &[], &expecting(&area));
        assert!(matches!(missing, Err(Fault::Missing(target)) if target == "server.jar"));
        std::fs::write(area.served.join("server.jar"), b"already there").unwrap();
        place(&area.build, &[], &expecting(&area)).unwrap();
    }
}
