//! An entry's icon is read where the daemon keeps it, in the entry's own
//! directory, so the webview has to be let at each one before its `asset:` URL
//! resolves. Only the icon file is let through: the directory around it holds
//! logs an old client writes its session id into.

use std::path::{Path, PathBuf};

use tauri::{AppHandle, Manager};

const STEM: &str = "icon";
const EXTENSIONS: [&str; 4] = ["png", "jpg", "webp", "gif"];
const ENTRY_ROOTS: [&str; 2] = ["servers", "instances"];

#[tauri::command]
pub fn icons_allow(app: AppHandle, paths: Vec<String>) -> Result<(), String> {
    let home = common::paths::data_home(None);
    for path in paths {
        let path = PathBuf::from(path);
        let Some(icon) = entry_icon(&home, &path) else {
            return Err(format!("{} is not an entry icon", path.display()));
        };
        app.asset_protocol_scope()
            .allow_file(&icon)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// The canonical `<home>/<servers|instances>/<entry>/icon.<ext>` `path` names,
/// resolved first so a link cannot pass for one.
fn entry_icon(home: &Path, path: &Path) -> Option<PathBuf> {
    let path = path.canonicalize().ok()?;
    let home = home.canonicalize().ok()?;
    let named = path.file_stem().is_some_and(|stem| stem == STEM)
        && path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| EXTENSIONS.contains(&ext));
    let placed = path
        .parent()
        .and_then(Path::parent)
        .is_some_and(|root| ENTRY_ROOTS.iter().any(|name| root == home.join(name)));
    (named && placed).then_some(path)
}

#[cfg(test)]
mod tests {
    use super::entry_icon;

    #[test]
    fn only_an_icon_at_an_entry_root_is_let_through() {
        let home = tempfile::tempdir().unwrap();
        let entry = home.path().join("instances/cozy");
        std::fs::create_dir_all(entry.join("logs")).unwrap();
        for file in ["icon.png", "icon.json", "logs/icon.png"] {
            std::fs::write(entry.join(file), b"").unwrap();
        }
        assert!(entry_icon(home.path(), &entry.join("icon.png")).is_some());
        assert!(entry_icon(home.path(), &entry.join("icon.json")).is_none());
        assert!(entry_icon(home.path(), &entry.join("logs/icon.png")).is_none());
        assert!(entry_icon(home.path(), &entry.join("logs/../icon.png")).is_some());
    }
}
