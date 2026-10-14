//! Opening a folder or a link with the host's default handler. The plugin's own
//! commands start that handler with the shell's environment, which inside an
//! AppImage is not the host's ([`common::appimage`]).

use std::ffi::OsStr;
use std::path::PathBuf;

/// The schemes `tauri-plugin-opener` opens by default, kept so a link in an
/// untrusted description cannot reach an arbitrary scheme handler.
const URL_SCHEMES: [&str; 4] = ["http", "https", "mailto", "tel"];

#[tauri::command]
pub async fn open_path(path: String) -> Result<(), String> {
    let path = PathBuf::from(path);
    if !path.is_absolute() || !path.exists() {
        return Err(format!("{} does not exist", path.display()));
    }
    hand_off(path.as_os_str())
}

#[tauri::command]
pub async fn open_url(url: String) -> Result<(), String> {
    let url = reqwest::Url::parse(&url).map_err(|e| format!("not a link: {e}"))?;
    if !URL_SCHEMES.contains(&url.scheme()) {
        return Err(format!("{}: links are not opened", url.scheme()));
    }
    hand_off(OsStr::new(url.as_str()))
}

fn hand_off(target: &OsStr) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    let opened = match common::appimage::host_env() {
        Some(env) => host::open(target, &env),
        None => open::that_detached(target),
    };
    #[cfg(not(target_os = "linux"))]
    let opened = open::that_detached(target);

    opened.map_err(|e| {
        tracing::warn!(error = %e, "cannot hand the target to the host");
        e.to_string()
    })
}

#[cfg(target_os = "linux")]
mod host {
    use std::ffi::{OsStr, OsString};
    use std::io;
    use std::os::unix::process::CommandExt;
    use std::process::{Child, Stdio};

    use common::appimage::Env;

    /// Open `target` with each opener the `open` crate knows in turn, started
    /// with `env` in place of the shell's own.
    pub fn open(target: &OsStr, env: &Env) -> io::Result<()> {
        let mut last = io::Error::new(io::ErrorKind::NotFound, "no opener is installed");
        for mut cmd in ::open::commands(target) {
            cmd.env_clear()
                .envs(env.iter().cloned())
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .process_group(0);
            match cmd.spawn() {
                Ok(child) => {
                    reap(child, cmd.get_program().to_os_string());
                    return Ok(());
                }
                Err(e) => last = e,
            }
        }
        Err(last)
    }

    /// Wait for the opener off-thread: some block until the handler they start
    /// exits, and an exit status is the only trace a failed handler leaves.
    fn reap(mut child: Child, program: OsString) {
        std::thread::spawn(move || match child.wait() {
            Ok(status) if !status.success() => {
                tracing::warn!(program = ?program, %status, "the host opener failed");
            }
            Ok(_) => {}
            Err(e) => {
                tracing::warn!(program = ?program, error = %e, "cannot wait for the host opener")
            }
        });
    }
}
