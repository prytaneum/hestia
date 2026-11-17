//! A program that runs is the program that was hashed. Each platform holds the
//! verified file differently (see its module) and builds the command that runs
//! it; the caller keeps the [`Program`] alive until the process has started.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux as platform;

#[cfg(all(unix, not(target_os = "linux")))]
mod fallback;
#[cfg(all(unix, not(target_os = "linux")))]
use fallback as platform;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as platform;

use std::io;
use std::path::Path;

use tokio::process::Command;

pub(super) struct Program {
    held: platform::Held,
    path: String,
}

impl Program {
    /// Blocking: reads the program. `None` when it is not the one with `sha256`.
    pub fn verify(path: &str, sha256: &str) -> io::Result<Option<Program>> {
        let (held, digest) = platform::hold(Path::new(path))?;
        Ok((digest == sha256).then(|| Program {
            held,
            path: path.to_owned(),
        }))
    }

    /// The command that runs the verified program, with an empty environment
    /// but for what the platform cannot run without. `data` is the plugin's own
    /// directory.
    pub fn command(&self, data: &Path) -> Command {
        self.held.command(&self.path, data)
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::Program;
    use crate::grant::digest;

    #[tokio::test]
    async fn what_runs_is_what_was_hashed_even_if_the_file_changes_after() {
        let dir = tempfile::tempdir().unwrap();
        let tool = dir.path().join("tool");
        std::fs::write(&tool, "#!/bin/sh\necho original\n").unwrap();
        std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
        let path = tool.to_str().unwrap();

        let program = Program::verify(path, &digest(&tool).unwrap())
            .unwrap()
            .expect("the digest matches");
        std::fs::write(&tool, "#!/bin/sh\necho swapped\n").unwrap();

        let output = program.command(dir.path()).output().await.unwrap();
        assert_eq!(output.stdout, b"original\n");
        assert!(Program::verify(path, &"0".repeat(64)).unwrap().is_none());
    }
}
