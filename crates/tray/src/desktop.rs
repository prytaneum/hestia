//! Launching the desktop shell from the tray. Spawned detached — the desktop
//! app outlives the tray. A second launch while it is already running is
//! absorbed by the shell's own single-instance handling (GApplication
//! re-focuses the existing window rather than opening another), so the tray
//! need not track it.

use std::process::{Command, Stdio};

pub fn launch() {
    spawn(&[], "desktop launched", "cannot launch the desktop");
}

/// Signal a running desktop shell to close (a no-op if none is running).
pub fn quit() {
    spawn(
        &[common::app::DESKTOP_QUIT_ARG],
        "desktop quit signalled",
        "cannot signal the desktop to quit",
    );
}

fn spawn(args: &[&str], ok_msg: &str, err_msg: &str) {
    // Inside an AppImage the shell runs only through the image's launcher,
    // which sets up the libraries and working directory its webview needs.
    let program = common::appimage::image()
        .or_else(|| common::paths::sibling_binary(common::app::DESKTOP_BIN));
    let Some(program) = program else {
        tracing::warn!("desktop binary not found in this layout");
        return;
    };

    let mut cmd = Command::new(&program);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: setsid is async-signal-safe and valid in the forked child.
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        cmd.creation_flags(DETACHED_PROCESS);
    }

    match cmd.spawn() {
        Ok(child) => tracing::info!(pid = child.id(), "{ok_msg}"),
        Err(e) => tracing::warn!("{err_msg}: {e}"),
    }
}
