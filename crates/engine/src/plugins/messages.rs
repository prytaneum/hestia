//! A plugin's message catalogs: `messages/<locale>.json` in its package, one
//! flat object of texts by key per language. The same lookup serves the app
//! (for the manifest's own texts, through `plugin.messages`) and the plugin (its
//! `messages.catalog` import), so a text reads the same in both.
//!
//! A locale resolves from the general to the specific: the `en` file, overlaid
//! by the language's (`pt`), overlaid by the exact locale's (`pt-BR`), so a key
//! the locale lacks reads in the nearest language that has it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use proto::plugin::is_locale_tag;

use super::store::Plugins;

const DIR: &str = "messages";
const FALLBACK: &str = "en";
pub const MAX_CATALOG_BYTES: u64 = 256 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum MessagesError {
    #[error("`{0}` is not a locale tag like `en` or `pt-BR`")]
    BadLocale(String),
    #[error(transparent)]
    Store(#[from] anyhow::Error),
}

/// The catalogs of one plugin's package.
#[derive(Debug, Clone)]
pub(crate) struct Catalogs {
    dir: PathBuf,
}

impl Catalogs {
    pub(crate) fn of_package(package: &Path) -> Self {
        Catalogs {
            dir: package.join(DIR),
        }
    }

    /// Blocking. A catalog that cannot be read or parsed is skipped with a
    /// warning, never an error: a broken translation must not break a page.
    pub(crate) fn resolve(&self, locale: &str) -> Result<BTreeMap<String, String>, MessagesError> {
        if !is_locale_tag(locale) {
            return Err(MessagesError::BadLocale(locale.to_string()));
        }
        let language = locale.split('-').next().unwrap_or(locale);
        let mut merged = BTreeMap::new();
        for tag in [FALLBACK, language, locale] {
            merged.extend(self.read(tag));
        }
        Ok(merged)
    }

    fn read(&self, tag: &str) -> BTreeMap<String, String> {
        let path = self.dir.join(format!("{tag}.json"));
        let Ok(metadata) = std::fs::metadata(&path) else {
            return BTreeMap::new();
        };
        if !metadata.is_file() || metadata.len() > MAX_CATALOG_BYTES {
            tracing::warn!(%tag, "plugin message catalog skipped: not a file or too large");
            return BTreeMap::new();
        }
        let parsed = std::fs::read_to_string(&path)
            .map_err(|error| error.to_string())
            .and_then(|text| {
                serde_json::from_str::<BTreeMap<String, String>>(&text)
                    .map_err(|error| error.to_string())
            });
        parsed.unwrap_or_else(|error| {
            tracing::warn!(%tag, %error, "plugin message catalog skipped");
            BTreeMap::new()
        })
    }
}

impl Plugins {
    /// Blocking: the catalog of `id` for `locale`, empty when it ships none.
    pub fn messages(
        &self,
        id: &str,
        locale: &str,
    ) -> Result<BTreeMap<String, String>, MessagesError> {
        Catalogs::of_package(&self.package_dir(id)?).resolve(locale)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn package(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(DIR)).unwrap();
        for (name, text) in files {
            std::fs::write(dir.path().join(DIR).join(name), text).unwrap();
        }
        dir
    }

    fn resolve(dir: &tempfile::TempDir, locale: &str) -> BTreeMap<String, String> {
        Catalogs::of_package(dir.path()).resolve(locale).unwrap()
    }

    #[test]
    fn the_exact_locale_overlays_its_language_which_overlays_english() {
        let dir = package(&[
            ("en.json", r#"{"a":"en a","b":"en b","c":"en c"}"#),
            ("pt.json", r#"{"b":"pt b","c":"pt c"}"#),
            ("pt-BR.json", r#"{"c":"br c"}"#),
        ]);
        let merged = resolve(&dir, "pt-BR");
        assert_eq!(
            [&merged["a"], &merged["b"], &merged["c"]],
            ["en a", "pt b", "br c"]
        );
        assert_eq!(resolve(&dir, "fr")["a"], "en a");
    }

    #[test]
    fn a_broken_or_oversized_catalog_is_skipped() {
        let big = format!(r#"{{"k":"{}"}}"#, "x".repeat(MAX_CATALOG_BYTES as usize));
        let dir = package(&[
            ("en.json", r#"{"a":"fine"}"#),
            ("pt.json", "not json"),
            ("pt-BR.json", &big),
        ]);
        assert_eq!(resolve(&dir, "pt-BR").len(), 1);
    }

    #[test]
    fn a_locale_cannot_name_a_path() {
        let dir = package(&[]);
        for locale in ["../x", "en/../..", "", "e", "pt_BR", "en-"] {
            assert!(
                matches!(
                    Catalogs::of_package(dir.path()).resolve(locale),
                    Err(MessagesError::BadLocale(_))
                ),
                "{locale}"
            );
        }
    }
}
