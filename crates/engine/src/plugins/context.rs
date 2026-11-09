//! What one call is lent beyond the plugin's own sandbox: a run's progress and
//! cancel, and directories mounted for that call only. Every call gets a fresh
//! store, so nothing here outlives it.

use std::path::{Component, Path, PathBuf};

use super::imports::CommandRun;

/// A host directory a call sees at `guest`.
#[derive(Debug, Clone)]
pub struct Mount {
    pub host: PathBuf,
    pub guest: &'static str,
    pub writable: bool,
}

/// The extras of one call.
#[derive(Default)]
pub struct CallContext {
    pub run: Option<CommandRun>,
    pub mounts: Vec<Mount>,
}

impl CallContext {
    pub fn running(run: CommandRun) -> Self {
        CallContext {
            run: Some(run),
            mounts: Vec::new(),
        }
    }
}

/// The host directory of `guest`, which must be an existing directory at or
/// under a writable mount. The result is canonical and checked against the
/// canonical mount root, so a symlink the guest planted cannot lead out.
pub(crate) fn writable_dir(mounts: &[Mount], guest: &str) -> Result<PathBuf, String> {
    let denied = || format!("`{guest}` is not a directory the call may work in");
    let guest = Path::new(guest);
    for mount in mounts.iter().filter(|mount| mount.writable) {
        let Ok(rest) = guest.strip_prefix(mount.guest) else {
            continue;
        };
        if rest
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
        {
            return Err(denied());
        }
        let root = mount.host.canonicalize().map_err(|_| denied())?;
        let dir = root.join(rest).canonicalize().map_err(|_| denied())?;
        return if dir.starts_with(&root) && dir.is_dir() {
            Ok(dir)
        } else {
            Err(denied())
        };
    }
    Err(denied())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mounts(dir: &Path) -> Vec<Mount> {
        vec![
            Mount {
                host: dir.join("data"),
                guest: "/data",
                writable: true,
            },
            Mount {
                host: dir.join("vanilla"),
                guest: "/vanilla",
                writable: false,
            },
        ]
    }

    #[test]
    fn a_working_directory_stays_inside_a_writable_mount() {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(home.path().join("data/sub")).unwrap();
        std::fs::create_dir_all(home.path().join("vanilla")).unwrap();
        let mounts = mounts(home.path());

        assert!(writable_dir(&mounts, "/data").is_ok());
        assert!(writable_dir(&mounts, "/data/sub").is_ok());
        for refused in [
            "/vanilla",
            "/data/missing",
            "/data/../vanilla",
            "/dataX",
            "/etc",
            "data",
            "",
        ] {
            assert!(writable_dir(&mounts, refused).is_err(), "{refused}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_cannot_lead_out_of_the_mount() {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(home.path().join("data")).unwrap();
        std::fs::create_dir_all(home.path().join("outside")).unwrap();
        std::os::unix::fs::symlink(home.path().join("outside"), home.path().join("data/escape"))
            .unwrap();
        assert!(writable_dir(&mounts(home.path()), "/data/escape").is_err());
    }
}
