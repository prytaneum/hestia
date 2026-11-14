//! The plugin's message catalogs, for the text of a page in the user's
//! language. The package carries `messages/<locale>.json`, a flat object of
//! texts by key with `{name}` placeholders; the host picks the language, falling
//! back from the exact locale to its language to `en`. A page reads the locale
//! from [`PageContext::locale`](proto::plugin::PageContext).
//!
//! The manifest's own texts (a page title, a setting's label and help) are
//! translated by the app from the keys `page.<id>.title`, `setting.<key>.label`,
//! `setting.<key>.help` and `setting.<key>.placeholder` of the same catalog.
//!
//! ```no_run
//! use hestia::messages::Messages;
//!
//! let messages = Messages::load(Some("pt-BR"))?;
//! let title = messages.format("backups.created", &[("name", "smp")]);
//! # Ok::<(), hestia::Error>(())
//! ```

use std::collections::BTreeMap;

use crate::bindings::hestia::plugin::messages as host;
use crate::Error;

/// One locale's catalog.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Messages(BTreeMap<String, String>);

impl Messages {
    /// Asks the host for the catalog of `locale`; `None` is `en`.
    pub fn load(locale: Option<&str>) -> Result<Messages, Error> {
        let json = host::catalog(locale.unwrap_or("en"))?;
        Ok(Messages(serde_json::from_str(&json)?))
    }

    /// The text of `key`, or the key itself when no catalog has it, so a missing
    /// message is visible instead of blank.
    pub fn get<'a>(&'a self, key: &'a str) -> &'a str {
        self.0.get(key).map_or(key, String::as_str)
    }

    /// [`get`](Messages::get) with each `{name}` replaced by its value.
    ///
    /// ```
    /// use hestia::messages::Messages;
    ///
    /// let messages = Messages::from([("hi", "Hello {name}")]);
    /// assert_eq!(messages.format("hi", &[("name", "Ana")]), "Hello Ana");
    /// assert_eq!(messages.format("missing", &[]), "missing");
    /// ```
    pub fn format(&self, key: &str, args: &[(&str, &str)]) -> String {
        args.iter()
            .fold(self.get(key).to_string(), |text, (name, value)| {
                text.replace(&format!("{{{name}}}"), value)
            })
    }
}

impl<const N: usize> From<[(&str, &str); N]> for Messages {
    fn from(entries: [(&str, &str); N]) -> Self {
        Messages(
            entries
                .into_iter()
                .map(|(key, text)| (key.to_string(), text.to_string()))
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_key_reads_as_itself() {
        let messages = Messages::from([("a", "A")]);
        assert_eq!(messages.get("a"), "A");
        assert_eq!(messages.get("b"), "b");
    }

    #[test]
    fn every_placeholder_is_replaced() {
        let messages = Messages::from([("k", "{a} and {a} and {b}")]);
        assert_eq!(
            messages.format("k", &[("a", "1"), ("b", "2")]),
            "1 and 1 and 2"
        );
    }
}
