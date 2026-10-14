//! Handing a path or URL back to the host desktop from inside an AppImage.
//!
//! The image's launcher prepends its mounted AppDir to `PATH` and
//! `LD_LIBRARY_PATH` and points GTK, GIO and GSettings into it, so the shell
//! finds its bundled libraries. A host program started with that environment —
//! `xdg-open` and the file manager it launches — loads some of the image's
//! libraries beside its own and dies in the dynamic linker. The shell keeps that
//! environment; only what it hands to the host is started without it.

use std::ffi::{OsStr, OsString};
use std::io;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};

/// Set by the AppImage runtime and launcher to describe the image itself.
const MARKERS: [&str; 5] = ["APPDIR", "APPIMAGE", "ARGV0", "OWD", "APPIMAGE_GTK_THEME"];

/// Forced by linuxdeploy's GTK hook to values outside the AppDir, so no path
/// filter catches them; left in place they pin the host's GTK apps to X11 and
/// Adwaita.
const FORCED: [&str; 2] = ["GDK_BACKEND", "GTK_THEME"];

pub type Env = Vec<(OsString, OsString)>;

/// The environment a host program should start with, or `None` outside an
/// AppImage, where the shell's own environment already is the host's.
pub fn host_env() -> Option<Env> {
    let appdir = PathBuf::from(std::env::var_os("APPDIR")?);
    appdir
        .is_absolute()
        .then(|| scrub(std::env::vars_os(), &appdir))
}

/// Open `target` with the host's default handler, trying each opener the `open`
/// crate knows in turn.
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
        Err(e) => tracing::warn!(program = ?program, error = %e, "cannot wait for the host opener"),
    });
}

fn scrub(vars: impl IntoIterator<Item = (OsString, OsString)>, appdir: &Path) -> Env {
    vars.into_iter()
        .filter(|(key, _)| !MARKERS.iter().chain(&FORCED).any(|name| key == name))
        .filter_map(|(key, value)| Some((key, outside(&value, appdir)?)))
        .collect()
}

/// `value` without its entries under `appdir`, or `None` when nothing is left.
/// A value with no such entry is kept verbatim, whatever its shape.
fn outside(value: &OsStr, appdir: &Path) -> Option<OsString> {
    let entries: Vec<PathBuf> = std::env::split_paths(value).collect();
    if !entries.iter().any(|entry| entry.starts_with(appdir)) {
        return Some(value.to_os_string());
    }
    let kept: Vec<PathBuf> = entries
        .into_iter()
        .filter(|entry| !entry.as_os_str().is_empty() && !entry.starts_with(appdir))
        .collect();
    if kept.is_empty() {
        return None;
    }
    std::env::join_paths(kept).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const APPDIR: &str = "/tmp/.mount_Hestia1a2b3c";

    fn scrubbed(vars: &[(&str, &str)]) -> Vec<(String, String)> {
        let vars = vars.iter().map(|(k, v)| (k.into(), v.into()));
        scrub(vars, Path::new(APPDIR))
            .into_iter()
            .map(|(k, v)| (k.into_string().unwrap(), v.into_string().unwrap()))
            .collect()
    }

    #[test]
    fn the_launcher_environment_is_undone() {
        let env = scrubbed(&[
            ("APPDIR", APPDIR),
            ("APPIMAGE", "/home/u/Hestia.AppImage"),
            ("OWD", "/home/u"),
            ("GDK_BACKEND", "x11"),
            ("GTK_THEME", "Adwaita:dark"),
            (
                "PATH",
                "/tmp/.mount_Hestia1a2b3c/usr/bin/:/tmp/.mount_Hestia1a2b3c/usr/sbin/:/usr/local/bin:/usr/bin",
            ),
            (
                "LD_LIBRARY_PATH",
                "/tmp/.mount_Hestia1a2b3c/usr/lib/:/tmp/.mount_Hestia1a2b3c/lib/:",
            ),
            ("GTK_PATH", "/tmp/.mount_Hestia1a2b3c//usr/lib/gtk-3.0"),
            ("GTK_DATA_PREFIX", APPDIR),
            (
                "XDG_DATA_DIRS",
                "/tmp/.mount_Hestia1a2b3c/usr/share/:/tmp/.mount_Hestia1a2b3c/usr/share:/usr/share:/usr/local/share",
            ),
            ("HOME", "/home/u"),
        ]);

        assert_eq!(
            env,
            [
                ("PATH".into(), "/usr/local/bin:/usr/bin".into()),
                ("XDG_DATA_DIRS".into(), "/usr/share:/usr/local/share".into()),
                ("HOME".into(), "/home/u".into()),
            ]
        );
    }

    #[test]
    fn a_value_outside_the_appdir_is_kept_verbatim() {
        let vars = [
            ("DBUS_SESSION_BUS_ADDRESS", "unix:path=/run/user/1000/bus"),
            ("PATH", "/usr/bin::/bin"),
            ("NEIGHBOUR", "/tmp/.mount_Hestia1a2b3cd/usr/lib"),
        ];
        assert_eq!(
            scrubbed(&vars),
            vars.map(|(k, v)| (k.to_string(), v.to_string()))
        );
    }
}
