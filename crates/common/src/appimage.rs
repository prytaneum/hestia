//! The AppImage runtime's contract, and undoing what its launcher does.
//!
//! The image's launcher prepends its mounted AppDir to `PATH` and
//! `LD_LIBRARY_PATH` and points GTK, GIO and GSettings into it, so the desktop
//! shell finds its bundled libraries. Any other process started with that
//! environment inherits it: a host program loads some of the image's libraries
//! beside its own and dies in the dynamic linker, and the daemon passes it on to
//! every game it launches. Only the shell keeps it.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

const IMAGE: &str = "APPIMAGE";

/// Set by the AppImage runtime and launcher to describe the image itself.
const MARKERS: [&str; 5] = ["APPDIR", IMAGE, "ARGV0", "OWD", "APPIMAGE_GTK_THEME"];

/// Forced by linuxdeploy's GTK hook to values outside the AppDir, so no path
/// filter catches them; left in place they pin the host's GTK apps to X11 and
/// Adwaita.
const FORCED: [&str; 2] = ["GDK_BACKEND", "GTK_THEME"];

pub type Env = Vec<(OsString, OsString)>;

/// The image this process was started from, as the runtime reports it — the
/// running binary sits on a throwaway mount that cannot be traced back to it.
pub fn image() -> Option<PathBuf> {
    std::env::var_os(IMAGE)
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
}

/// The environment a host program should start with, or `None` when this
/// process is not under the image's launcher and its environment already is
/// the host's.
pub fn host_env() -> Option<Env> {
    env(&[])
}

/// [`host_env`] for one of Hestia's own binaries that runs on without the shell,
/// still told which image it came from: self-update replaces that file, and the
/// tray relaunches the shell through it.
pub fn sidecar_env() -> Option<Env> {
    env(&[IMAGE])
}

fn env(keep: &[&str]) -> Option<Env> {
    let appdir = PathBuf::from(std::env::var_os("APPDIR")?);
    appdir
        .is_absolute()
        .then(|| scrub(std::env::vars_os(), &appdir, keep))
}

fn scrub(
    vars: impl IntoIterator<Item = (OsString, OsString)>,
    appdir: &Path,
    keep: &[&str],
) -> Env {
    let dropped = |key: &OsStr| {
        !keep.iter().any(|name| key == *name)
            && MARKERS.iter().chain(&FORCED).any(|name| key == *name)
    };
    vars.into_iter()
        .filter(|(key, _)| !dropped(key))
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

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    const APPDIR: &str = "/tmp/.mount_Hestia1a2b3c";

    fn scrubbed(vars: &[(&str, &str)], keep: &[&str]) -> Vec<(String, String)> {
        let vars = vars.iter().map(|(k, v)| (k.into(), v.into()));
        scrub(vars, Path::new(APPDIR), keep)
            .into_iter()
            .map(|(k, v)| (k.into_string().unwrap(), v.into_string().unwrap()))
            .collect()
    }

    const LAUNCHED: [(&str, &str); 11] = [
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
    ];

    #[test]
    fn the_launcher_environment_is_undone() {
        assert_eq!(
            scrubbed(&LAUNCHED, &[]),
            [
                ("PATH".into(), "/usr/local/bin:/usr/bin".into()),
                ("XDG_DATA_DIRS".into(), "/usr/share:/usr/local/share".into()),
                ("HOME".into(), "/home/u".into()),
            ]
        );
    }

    #[test]
    fn a_sidecar_still_knows_its_image() {
        let env = scrubbed(&LAUNCHED, &[IMAGE]);
        assert!(env.contains(&("APPIMAGE".into(), "/home/u/Hestia.AppImage".into())));
        assert!(!env.iter().any(|(_, value)| value.contains(APPDIR)));
    }

    #[test]
    fn a_value_outside_the_appdir_is_kept_verbatim() {
        let vars = [
            ("DBUS_SESSION_BUS_ADDRESS", "unix:path=/run/user/1000/bus"),
            ("PATH", "/usr/bin::/bin"),
            ("NEIGHBOUR", "/tmp/.mount_Hestia1a2b3cd/usr/lib"),
        ];
        assert_eq!(
            scrubbed(&vars, &[]),
            vars.map(|(k, v)| (k.to_string(), v.to_string()))
        );
    }
}
