use std::path::{Component, Path};

use anyhow::{bail, Result};
use base64::Engine as _;
use proto::error::{ErrorInfo, FileFault};
use proto::plugin::PluginAssetResult;

use super::store::Plugins;

pub const MAX_ASSET_BYTES: u64 = 8 * 1024 * 1024;

const SERVED_DIRS: [&str; 2] = ["assets", "themes"];

const MIME_TYPES: [(&str, &str); 13] = [
    ("css", "text/css"),
    ("woff2", "font/woff2"),
    ("woff", "font/woff"),
    ("ttf", "font/ttf"),
    ("otf", "font/otf"),
    ("png", "image/png"),
    ("jpg", "image/jpeg"),
    ("jpeg", "image/jpeg"),
    ("gif", "image/gif"),
    ("webp", "image/webp"),
    ("avif", "image/avif"),
    ("svg", "image/svg+xml"),
    ("json", "application/json"),
];

fn rejected(path: &str, reason: FileFault) -> anyhow::Error {
    ErrorInfo::FileRejected {
        path: path.to_string(),
        reason,
    }
    .into()
}

fn mime_of(path: &Path) -> Option<&'static str> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    MIME_TYPES
        .iter()
        .find(|(known, _)| *known == extension)
        .map(|(_, mime)| *mime)
}

impl Plugins {
    /// Blocking: a file of an enabled plugin's `package/assets` or
    /// `package/themes`, for the desktop's `hestia-plugin` scheme. `path` is
    /// client input and stays inside those two directories, links included.
    pub fn asset(&self, id: &str, path: &str) -> Result<PluginAssetResult> {
        let record = self
            .get(id)
            .ok_or_else(|| ErrorInfo::PluginNotFound { id: id.to_string() })?;
        if !record.enabled {
            bail!(ErrorInfo::PluginUnavailable { id: id.to_string() });
        }
        let relative = Path::new(path);
        let mut components = relative.components();
        let served = match components.next() {
            Some(Component::Normal(first)) => first
                .to_str()
                .and_then(|name| SERVED_DIRS.into_iter().find(|dir| *dir == name)),
            _ => None,
        };
        let Some(served) = served else {
            bail!(rejected(path, FileFault::OutsideEntry));
        };
        if components.clone().next().is_none()
            || components.any(|component| !matches!(component, Component::Normal(_)))
        {
            bail!(rejected(path, FileFault::OutsideEntry));
        }
        let Some(mime) = mime_of(relative) else {
            bail!(rejected(path, FileFault::UnsupportedType));
        };
        let not_found = || ErrorInfo::FileNotFound {
            path: path.to_string(),
        };
        let package = self.package_dir(id)?;
        let root = package.canonicalize().map_err(|_| not_found())?;
        let target = package
            .join(relative)
            .canonicalize()
            .map_err(|_| not_found())?;
        if !target.starts_with(root.join(served)) {
            tracing::warn!(
                plugin = id,
                path,
                "plugin asset resolves outside its directory"
            );
            bail!(rejected(path, FileFault::OutsideEntry));
        }
        let metadata = std::fs::metadata(&target).map_err(|_| not_found())?;
        if !metadata.is_file() {
            bail!(not_found());
        }
        if metadata.len() > MAX_ASSET_BYTES {
            bail!(rejected(path, FileFault::TooLarge));
        }
        let bytes = std::fs::read(&target).map_err(|_| not_found())?;
        Ok(PluginAssetResult {
            mime: mime.to_string(),
            bytes: base64::engine::general_purpose::STANDARD.encode(bytes),
        })
    }
}

#[cfg(test)]
mod tests {
    use proto::error::{ErrorInfo, FileFault};
    use tempfile::TempDir;

    use super::*;
    use crate::plugins::store::{PluginRecord, PACKAGE};

    fn install(enabled: bool) -> (TempDir, Plugins) {
        let home = tempfile::tempdir().unwrap();
        let plugins = Plugins::new(home.path().to_path_buf());
        plugins
            .save(&PluginRecord {
                id: "demo".into(),
                enabled,
                ..PluginRecord::default()
            })
            .unwrap();
        let package = home.path().join("demo").join(PACKAGE);
        std::fs::create_dir_all(package.join("assets")).unwrap();
        std::fs::create_dir_all(package.join("themes")).unwrap();
        std::fs::create_dir_all(package.join("src")).unwrap();
        std::fs::write(package.join("assets/icon.png"), b"png").unwrap();
        std::fs::write(package.join("themes/ember.css"), b"a{}").unwrap();
        std::fs::write(package.join("assets/notes.txt"), b"x").unwrap();
        std::fs::write(package.join("plugin.toml"), b"secret").unwrap();
        (home, plugins)
    }

    fn refusal(plugins: &Plugins, id: &str, path: &str) -> ErrorInfo {
        plugins
            .asset(id, path)
            .expect_err("refused")
            .downcast::<ErrorInfo>()
            .expect("typed")
    }

    fn rejected_with(plugins: &Plugins, path: &str, reason: FileFault) {
        match refusal(plugins, "demo", path) {
            ErrorInfo::FileRejected { reason: got, .. } => assert_eq!(got, reason, "{path}"),
            other => panic!("{path}: {other:?}"),
        }
    }

    #[test]
    fn serves_a_file_under_assets_and_themes_with_its_mime() {
        let (_home, plugins) = install(true);
        let icon = plugins.asset("demo", "assets/icon.png").unwrap();
        assert_eq!(icon.mime, "image/png");
        assert_eq!(
            icon.bytes,
            base64::engine::general_purpose::STANDARD.encode(b"png")
        );
        assert_eq!(
            plugins.asset("demo", "themes/ember.css").unwrap().mime,
            "text/css"
        );
    }

    #[test]
    fn traversal_absolute_and_foreign_directories_are_refused() {
        let (_home, plugins) = install(true);
        for path in [
            "assets/../plugin.toml",
            "../plugin.toml",
            "/etc/passwd",
            "plugin.toml",
            "src/lib.css",
            "assets",
            "assets/",
            "",
        ] {
            rejected_with(&plugins, path, FileFault::OutsideEntry);
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_link_out_of_the_served_directory_is_refused() {
        let (home, plugins) = install(true);
        let package = home.path().join("demo").join(PACKAGE);
        let outside = home.path().join("outside.css");
        std::fs::write(&outside, b"a{}").unwrap();
        std::os::unix::fs::symlink(&outside, package.join("assets/leak.css")).unwrap();
        std::os::unix::fs::symlink(package.join("plugin.toml"), package.join("themes/own.css"))
            .unwrap();
        rejected_with(&plugins, "assets/leak.css", FileFault::OutsideEntry);
        rejected_with(&plugins, "themes/own.css", FileFault::OutsideEntry);
    }

    #[test]
    fn a_file_past_the_cap_is_refused() {
        let (home, plugins) = install(true);
        let big = home
            .path()
            .join("demo")
            .join(PACKAGE)
            .join("assets/big.png");
        std::fs::File::create(big)
            .unwrap()
            .set_len(MAX_ASSET_BYTES + 1)
            .unwrap();
        rejected_with(&plugins, "assets/big.png", FileFault::TooLarge);
    }

    #[test]
    fn an_unknown_extension_is_refused() {
        let (_home, plugins) = install(true);
        rejected_with(&plugins, "assets/notes.txt", FileFault::UnsupportedType);
        rejected_with(&plugins, "assets/script.js", FileFault::UnsupportedType);
    }

    #[test]
    fn a_missing_file_is_not_found() {
        let (_home, plugins) = install(true);
        assert!(matches!(
            refusal(&plugins, "demo", "assets/none.png"),
            ErrorInfo::FileNotFound { .. }
        ));
    }

    #[test]
    fn a_disabled_or_unknown_plugin_serves_nothing() {
        let (_home, plugins) = install(false);
        assert!(matches!(
            refusal(&plugins, "demo", "assets/icon.png"),
            ErrorInfo::PluginUnavailable { .. }
        ));
        assert!(matches!(
            refusal(&plugins, "ghost", "assets/icon.png"),
            ErrorInfo::PluginNotFound { .. }
        ));
    }
}
