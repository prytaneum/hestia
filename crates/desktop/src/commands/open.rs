//! Files the shell hands the app.
//!
//! Installing hestia claims `.hestia`, so double-clicking an instance archive
//! launches the app with that path as an argument — or, when a window is
//! already open, hands it to the running instance instead (single-instance).
//! Both routes land here, and the webview learns about it the same way in
//! either case: it takes whatever is pending at boot, and listens for the
//! event afterwards.

use std::sync::Mutex;

use tauri::{Emitter, Manager, State};

/// The topic the webview listens on for a file opened while it was running.
pub const ARCHIVE_OPENED: &str = "archive-opened";

/// A path the shell was given before the webview was ready to hear about it.
/// Taken exactly once — a second read is a fresh boot's business, not a replay
/// of the last one.
#[derive(Default)]
pub struct PendingArchive(Mutex<Option<String>>);

impl PendingArchive {
    pub fn set(&self, path: String) {
        *self.0.lock().unwrap() = Some(path);
    }

    fn take(&self) -> Option<String> {
        self.0.lock().unwrap().take()
    }
}

/// The archive path this launch carries, if any. Anything that is not an
/// existing file is ignored: the argument list also holds flags, and a path
/// that has since been moved is not worth an error the user cannot act on.
pub fn archive_from(argv: &[String]) -> Option<String> {
    argv.iter()
        .skip(1)
        .find(|arg| !arg.starts_with('-') && std::path::Path::new(arg).is_file())
        .cloned()
}

/// Route an opened file to the webview, or hold it until the webview asks.
pub fn deliver(app: &tauri::AppHandle, path: String) {
    tracing::info!(path, "an archive was opened with the app");
    let pending: State<'_, PendingArchive> = app.state();
    match app.emit(ARCHIVE_OPENED, &path) {
        Ok(()) => {}
        Err(e) => {
            tracing::warn!(error = %e, "cannot deliver the opened archive; holding it");
            pending.set(path);
        }
    }
}

/// The archive the app was opened with, cleared as it is read.
#[tauri::command]
pub fn pending_archive(pending: State<'_, PendingArchive>) -> Option<String> {
    pending.take()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(args: &[&str]) -> Vec<String> {
        args.iter().map(|a| a.to_string()).collect()
    }

    #[test]
    fn the_executable_and_flags_are_not_the_file() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let path = file.path().to_string_lossy().into_owned();

        assert_eq!(
            archive_from(&argv(&["hestia-desktop", "--quit", &path])),
            Some(path.clone())
        );
        assert_eq!(archive_from(&argv(&["hestia-desktop"])), None);
        assert_eq!(archive_from(&argv(&["hestia-desktop", "--quit"])), None);
        assert_eq!(
            archive_from(&argv(&["hestia-desktop", "/nowhere/gone.hestia"])),
            None,
            "a path that no longer exists is not worth an error"
        );
        assert_eq!(
            archive_from(&argv(&[&path])),
            None,
            "the first argument is the executable"
        );
    }
}
