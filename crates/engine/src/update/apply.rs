//! Installing a downloaded artifact over the running build.

use std::path::Path;

use anyhow::{bail, Result};
use proto::update::UpdateInstall;

#[derive(Debug)]
pub struct Applied {
    pub relaunches: bool,
}

pub fn apply(artifact: &Path, install: UpdateInstall) -> Result<Applied> {
    if install == UpdateInstall::Unmanaged {
        bail!(
            "this copy of hestia was not placed by an installer, so it cannot \
             replace itself — unpack the new archive over it instead"
        );
    }
    tracing::info!(?install, ?artifact, "applying update");
    platform::apply(artifact, install)
}

#[cfg(windows)]
mod platform {
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    use anyhow::{bail, Result};
    use proto::update::UpdateInstall;
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOW;

    use super::Applied;

    /// `/P` shows only a progress page, `/UPDATE` reuses the recorded install
    /// directory, mode and component choices. The installer stops the daemon
    /// and the tray, replaces them, and restarts whatever was running.
    pub fn apply(artifact: &Path, _install: UpdateInstall) -> Result<Applied> {
        launch(artifact, "/P /UPDATE")?;
        Ok(Applied { relaunches: true })
    }

    /// `ShellExecuteW`, not `Command::spawn`: the setup is manifested
    /// `highestAvailable`, and `CreateProcess` refuses that outright with
    /// ERROR_ELEVATION_REQUIRED rather than raising the UAC prompt.
    fn launch(artifact: &Path, parameters: &str) -> Result<()> {
        let file = wide(artifact.as_os_str());
        let parameters = wide(parameters.as_ref());
        let verb = wide("open".as_ref());

        let result = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                verb.as_ptr(),
                file.as_ptr(),
                parameters.as_ptr(),
                std::ptr::null(),
                SW_SHOW,
            )
        };
        if result as isize <= 32 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() == Some(ERROR_CANCELLED) {
                bail!("the request for administrator rights was declined");
            }
            return Err(anyhow::Error::new(error).context("cannot start the installer"));
        }
        Ok(())
    }

    const ERROR_CANCELLED: i32 = 1223;

    fn wide(text: &std::ffi::OsStr) -> Vec<u16> {
        text.encode_wide().chain(std::iter::once(0)).collect()
    }
}

#[cfg(not(windows))]
mod platform {
    use std::ffi::OsStr;
    use std::path::Path;
    use std::process::{Command, Stdio};

    use anyhow::{anyhow, bail, Context, Result};
    use proto::update::UpdateInstall;

    use super::Applied;

    pub fn apply(artifact: &Path, install: UpdateInstall) -> Result<Applied> {
        match install {
            UpdateInstall::AppImage => replace_appimage(artifact),
            UpdateInstall::Deb => escalate("dpkg", &["-i".as_ref(), artifact.as_os_str()]),
            UpdateInstall::Rpm => escalate("rpm", &["-U".as_ref(), artifact.as_os_str()]),
            UpdateInstall::Nsis | UpdateInstall::Unmanaged => {
                bail!("this install cannot be updated in place on linux")
            }
        }
        .map(|()| Applied { relaunches: false })
    }

    fn replace_appimage(artifact: &Path) -> Result<()> {
        let target = common::appimage::image()
            .ok_or_else(|| anyhow!("$APPIMAGE is unset, so there is no image to replace"))?;

        // Staged beside the target because a rename is atomic only within one
        // filesystem, and committed by rename because the running process has
        // the old image mounted — writing over it in place would truncate the
        // filesystem out from under it.
        let staged = target.with_extension("AppImage.new");
        std::fs::copy(artifact, &staged).context("cannot stage the new AppImage")?;
        let committed = make_executable(&staged).and_then(|()| {
            std::fs::rename(&staged, &target).context("cannot replace the running AppImage")
        });
        if committed.is_err() {
            let _ = std::fs::remove_file(&staged);
        }
        committed
    }

    fn make_executable(path: &Path) -> Result<()> {
        use std::os::unix::fs::PermissionsExt;

        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
            .context("cannot make the new AppImage executable")
    }

    /// `pkexec` is tried unconditionally rather than gated on `$DISPLAY`: it is
    /// the one step that can prompt, and a text polkit agent would have
    /// answered. The daemon has no terminal, so when nothing can even ask, the
    /// command is handed back for a caller that does have one.
    fn escalate(program: &str, args: &[&OsStr]) -> Result<()> {
        if is_root() {
            return run(program, args);
        }
        match run("pkexec", &prepend(&[program.as_ref()], args)) {
            Ok(()) => return Ok(()),
            Err(e) if declined(&e) => return Err(e),
            Err(e) => tracing::debug!(error = %e, "pkexec could not elevate; trying sudo"),
        }
        if run("sudo", &prepend(&["-n".as_ref(), program.as_ref()], args)).is_ok() {
            return Ok(());
        }
        bail!(proto::error::ErrorInfo::ElevationRequired {
            command: describe(program, args),
        })
    }

    fn run(program: &str, args: &[&OsStr]) -> Result<()> {
        let status = Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .status()
            .with_context(|| format!("cannot run {program}"))?;
        if !status.success() {
            return Err(anyhow::Error::new(ExitStatus {
                program: program.to_string(),
                code: status.code(),
            }));
        }
        Ok(())
    }

    #[derive(Debug)]
    struct ExitStatus {
        program: String,
        code: Option<i32>,
    }

    /// pkexec answers 126 for a dismissed prompt and 127 for no agent at all.
    /// Asking again through sudo after a refusal would be asking twice.
    fn declined(error: &anyhow::Error) -> bool {
        error
            .downcast_ref::<ExitStatus>()
            .is_some_and(|e| e.code == Some(126))
    }

    impl std::fmt::Display for ExitStatus {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self.code {
                Some(126) => write!(f, "the request for administrator rights was declined"),
                Some(code) => write!(f, "{} exited with status {code}", self.program),
                None => write!(f, "{} was terminated by a signal", self.program),
            }
        }
    }

    impl std::error::Error for ExitStatus {}

    fn prepend<'a>(first: &[&'a OsStr], rest: &[&'a OsStr]) -> Vec<&'a OsStr> {
        first.iter().chain(rest.iter()).copied().collect()
    }

    fn describe(program: &str, args: &[&OsStr]) -> String {
        let mut parts = vec!["sudo".to_string(), program.to_string()];
        parts.extend(args.iter().map(|a| a.to_string_lossy().into_owned()));
        parts.join(" ")
    }

    fn is_root() -> bool {
        unsafe { libc::getuid() == 0 }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn the_handback_command_is_runnable_as_written() {
            let command = describe("dpkg", &["-i".as_ref(), "/tmp/hestia.deb".as_ref()]);
            assert_eq!(command, "sudo dpkg -i /tmp/hestia.deb");
        }

        #[test]
        fn a_dismissed_prompt_is_distinguished_from_a_missing_agent() {
            let dismissed = anyhow::Error::new(ExitStatus {
                program: "pkexec".into(),
                code: Some(126),
            });
            let absent = anyhow::Error::new(ExitStatus {
                program: "pkexec".into(),
                code: Some(127),
            });
            assert!(declined(&dismissed));
            assert!(!declined(&absent));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unmanaged_build_refuses_before_touching_anything() {
        let err = apply(Path::new("/nonexistent"), UpdateInstall::Unmanaged).unwrap_err();
        assert!(
            err.to_string().contains("not placed by an installer"),
            "{err}"
        );
    }
}
