//! Schema versioning for the documents hestia owns on disk.
//!
//! Every user-owned document — the settings, the accounts, an entry's record,
//! its content index, the skin library — carries the version of the schema it
//! was written with, in a top-level
//! [`FIELD`]. A build that opens one therefore knows, before it decodes
//! anything, whether it is looking at a shape it understands or a shape from a
//! build newer than itself.
//!
//! The stamp is **per document, not per data home**. A `server.json` restored
//! from a backup, an entry directory copied between machines, and the record
//! travelling inside an exported archive are all the same problem, and only a
//! self-describing document answers it — a single version file at the root
//! would speak for files it never saw written.
//!
//! # What is deliberately not versioned
//!
//! Derived state — process records and tombstones,
//! installed-runtime records, the download cache. A document nothing is lost by
//! discarding does not need a migration path; it needs deleting and
//! regenerating, which is what already happens when one fails to read. Desktop
//! preferences are excluded too: they are schema-less by design and the
//! front-end owns their keys.

pub mod notices;

use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;

use crate::registry;

/// The stamped field, on the document's top-level object.
pub(crate) const FIELD: &str = "schemaVersion";

/// The version an unstamped document is read as.
pub(crate) const BASELINE: u32 = 1;

/// The suffix a document that could not be read is renamed with.
const ASIDE: &str = "unreadable";

/// A document hestia persists and must be able to read back across versions.
///
/// Implementors serialize to a JSON **object** — that is where the stamp goes —
/// and tolerate unknown fields, which serde's derive does by default.
pub(crate) trait Document: Serialize + DeserializeOwned {
    /// What this document is called, for logs, warnings and the name of a
    /// quarantined copy. Its file name where it has a fixed one.
    const NAME: &'static str;

    /// The schema this build writes.
    const VERSION: u32 = BASELINE;
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum SchemaError {
    #[error(
        "{document} was written by a newer hestia (schema {found}; this build reads up to \
         {supported})"
    )]
    FromTheFuture {
        document: &'static str,
        found: u32,
        supported: u32,
    },
    #[error("{document} declares a schema version that is not a whole number 1 or above")]
    BadVersion { document: &'static str },
    #[error("{document} is not valid JSON: {source}")]
    Malformed {
        document: &'static str,
        source: serde_json::Error,
    },
    #[error("{document} does not match schema {version}: {source}")]
    Invalid {
        document: &'static str,
        version: u32,
        source: serde_json::Error,
    },
    #[error("{document} does not serialize to a JSON object, so it cannot be stamped")]
    NotAnObject { document: &'static str },
}

/// Decode a stored JSON value, refusing one from a newer build. Pure: no file is
/// read, written or set aside, so a caller that owns a document from somewhere
/// other than the data home — an archive — applies its own policy to the
/// failure.
pub(crate) fn decode<T: Document>(mut value: Value) -> Result<T, SchemaError> {
    let stored = stamp::<T>(&value)?;
    if stored > T::VERSION {
        return Err(SchemaError::FromTheFuture {
            document: T::NAME,
            found: stored,
            supported: T::VERSION,
        });
    }

    if let Some(object) = value.as_object_mut() {
        object.remove(FIELD);
    }

    serde_json::from_value(value).map_err(|source| SchemaError::Invalid {
        document: T::NAME,
        version: T::VERSION,
        source,
    })
}

/// Serialize a document and stamp it with the schema this build writes.
pub(crate) fn encode<T: Document>(document: &T) -> Result<Value, SchemaError> {
    let mut value = serde_json::to_value(document).map_err(|source| SchemaError::Invalid {
        document: T::NAME,
        version: T::VERSION,
        source,
    })?;
    let Some(object) = value.as_object_mut() else {
        return Err(SchemaError::NotAnObject { document: T::NAME });
    };
    object.insert(FIELD.to_string(), Value::from(T::VERSION));
    Ok(value)
}

/// Read a document, or `None` when there is none to read.
///
/// A file that cannot be read *as this document* — malformed or from a newer
/// build — is renamed aside rather than treated as absent, because the caller's
/// next write would otherwise land on top of it. The quarantine is recorded as a
/// [`notices`] warning so somebody other than the log finds out.
pub(crate) fn load<T: Document>(path: &Path) -> Option<T> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => {
            // Not a schema problem and not something a rename would survive:
            // report it and leave the file exactly as it is.
            tracing::warn!(path = %path.display(), "cannot read {}: {e}", T::NAME);
            return None;
        }
    };

    let value = match serde_json::from_str::<Value>(&text) {
        Ok(value) => value,
        Err(source) => {
            return set_aside::<T>(
                path,
                SchemaError::Malformed {
                    document: T::NAME,
                    source,
                },
            )
        }
    };

    match decode::<T>(value) {
        Ok(document) => Some(document),
        Err(error) => set_aside::<T>(path, error),
    }
}

/// Write a document, stamped, through a temp file renamed into place — so a
/// failure or a crash leaves the previous version intact rather than a
/// half-written one.
pub(crate) fn save<T: Document>(path: &Path, document: &T) -> anyhow::Result<()> {
    write(path, document, false)
}

/// [`save`], with the file owner-only where permissions exist. For anything
/// holding a token: the mode is set on the temp file *before* it is renamed
/// into place, so the document is never briefly world-readable.
pub(crate) fn save_private<T: Document>(path: &Path, document: &T) -> anyhow::Result<()> {
    write(path, document, true)
}

fn write<T: Document>(path: &Path, document: &T, private: bool) -> anyhow::Result<()> {
    use anyhow::Context;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("cannot create {}", parent.display()))?;
    }
    let value = encode(document)?;
    let text = serde_json::to_string_pretty(&value).context("the document serializes")?;

    let part = sibling(path, "part");
    let result = (|| -> std::io::Result<()> {
        std::fs::write(&part, format!("{text}\n"))?;
        if private {
            restrict(&part)?;
        }
        std::fs::rename(&part, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    result.with_context(|| format!("cannot write {} at {}", T::NAME, path.display()))
}

#[cfg(unix)]
fn restrict(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn restrict(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

fn stamp<T: Document>(value: &Value) -> Result<u32, SchemaError> {
    match value.get(FIELD) {
        None => Ok(BASELINE),
        Some(Value::Number(number)) => number
            .as_u64()
            .filter(|found| (u64::from(BASELINE)..=u64::from(u32::MAX)).contains(found))
            .map(|found| found as u32)
            .ok_or(SchemaError::BadVersion { document: T::NAME }),
        Some(_) => Err(SchemaError::BadVersion { document: T::NAME }),
    }
}

/// Move an unreadable document out of the way and record why.
///
/// A rename that itself fails means the directory is not writable, in which case
/// the write this was protecting against cannot happen either — so there is
/// nothing further to do but say so loudly.
fn set_aside<T: Document>(path: &Path, error: SchemaError) -> Option<T> {
    let destination = aside_path(path);
    match std::fs::rename(path, &destination) {
        Ok(()) => {
            tracing::warn!(
                path = %path.display(),
                kept = %destination.display(),
                "{error}; it was set aside and this build is starting from defaults"
            );
            notices::record(proto::warning::WarningInfo::DocumentQuarantined {
                document: T::NAME.to_string(),
                path: destination.display().to_string(),
                detail: error.to_string(),
            });
        }
        Err(e) => {
            tracing::error!(
                path = %path.display(),
                "{error}; it could not be set aside either ({e})"
            );
            notices::record(proto::warning::WarningInfo::DocumentQuarantined {
                document: T::NAME.to_string(),
                path: path.display().to_string(),
                detail: error.to_string(),
            });
        }
    }
    None
}

/// `<file>.unreadable-<stamp>`, disambiguated if one already exists — two
/// quarantines of the same file within a second must not overwrite each other,
/// which would defeat the point of keeping them.
fn aside_path(path: &Path) -> PathBuf {
    let stamp = registry::utc_stamp(registry::now_unix());
    let first = sibling(path, &format!("{ASIDE}-{stamp}"));
    if !first.exists() {
        return first;
    }
    for attempt in 1..u32::MAX {
        let next = sibling(path, &format!("{ASIDE}-{stamp}-{attempt}"));
        if !next.exists() {
            return next;
        }
    }
    first
}

/// `<path>.<suffix>` — appended, never substituted, so `config.json` becomes
/// `config.json.part` rather than `config.part`.
fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".{suffix}"));
    path.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Serialize, Deserialize, Default, Debug, PartialEq)]
    #[serde(default, rename_all = "camelCase")]
    struct Doc {
        name: String,
        count: u32,
    }

    impl Document for Doc {
        const NAME: &'static str = "doc.json";
    }

    fn temp() -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix("hestia-schema-")
            .tempdir()
            .expect("temp dir")
    }

    #[test]
    fn a_saved_document_is_stamped_and_reads_back() {
        let dir = temp();
        let path = dir.path().join("doc.json");
        let doc = Doc {
            name: "one".to_string(),
            count: 2,
        };

        save(&path, &doc).unwrap();

        let text = std::fs::read_to_string(&path).unwrap();
        let raw: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(raw[FIELD], Value::from(1));
        assert_eq!(load::<Doc>(&path), Some(doc));
    }

    #[test]
    fn a_missing_document_is_absent_not_quarantined() {
        let dir = temp();
        assert!(load::<Doc>(&dir.path().join("nothing.json")).is_none());
    }

    #[test]
    fn an_unstamped_document_is_read_as_the_baseline() {
        let dir = temp();
        let path = dir.path().join("doc.json");
        std::fs::write(&path, r#"{"name":"plain"}"#).unwrap();

        assert_eq!(load::<Doc>(&path).expect("loads").name, "plain");
    }

    #[test]
    fn a_document_from_the_future_is_set_aside_never_decoded() {
        let dir = temp();
        let path = dir.path().join("doc.json");
        std::fs::write(&path, r#"{"schemaVersion":9,"name":"tomorrow"}"#).unwrap();

        assert!(load::<Doc>(&path).is_none());

        assert!(!path.exists(), "the original is not left to be overwritten");
        let kept = quarantined(dir.path());
        assert!(kept.contains("tomorrow"), "the content survived in {kept}");
    }

    #[test]
    fn malformed_json_is_set_aside() {
        let dir = temp();
        let path = dir.path().join("doc.json");
        std::fs::write(&path, "{ not json").unwrap();

        assert!(load::<Doc>(&path).is_none());
        assert!(quarantined(dir.path()).contains("not json"));
    }

    #[test]
    fn a_document_that_does_not_match_its_declared_schema_is_set_aside() {
        let dir = temp();
        let path = dir.path().join("doc.json");
        std::fs::write(&path, r#"{"schemaVersion":1,"count":"not a number"}"#).unwrap();

        assert!(load::<Doc>(&path).is_none());
        assert!(quarantined(dir.path()).contains("not a number"));
    }

    #[test]
    fn a_non_numeric_stamp_is_set_aside_rather_than_assumed() {
        let dir = temp();
        let path = dir.path().join("doc.json");
        std::fs::write(&path, r#"{"schemaVersion":"1","name":"odd"}"#).unwrap();

        assert!(load::<Doc>(&path).is_none());
        assert!(quarantined(dir.path()).contains("odd"));
    }

    #[test]
    fn a_quarantine_is_recorded_where_a_front_end_can_see_it() {
        let dir = temp();
        let path = dir.path().join("doc.json");
        let mark = notices::mark();
        std::fs::write(&path, "{ broken").unwrap();

        load::<Doc>(&path);

        let mine: Vec<_> = notices::since(mark)
            .into_iter()
            .filter(|w| {
                matches!(w, proto::warning::WarningInfo::DocumentQuarantined { path, .. }
                if path.starts_with(&dir.path().display().to_string()))
            })
            .collect();
        assert_eq!(mine.len(), 1, "got {mine:?}");
    }

    #[test]
    fn two_quarantines_of_one_file_do_not_overwrite_each_other() {
        let dir = temp();
        let path = dir.path().join("doc.json");
        std::fs::write(&path, "{ first").unwrap();
        load::<Doc>(&path);
        std::fs::write(&path, "{ second").unwrap();
        load::<Doc>(&path);

        let kept: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().contains(ASIDE))
            .collect();
        assert_eq!(kept.len(), 2);
    }

    #[test]
    fn a_write_leaves_no_temp_behind() {
        let dir = temp();
        save(&dir.path().join("doc.json"), &Doc::default()).unwrap();

        let names: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["doc.json"]);
    }

    fn quarantined(dir: &Path) -> String {
        let entry = std::fs::read_dir(dir)
            .unwrap()
            .flatten()
            .find(|e| e.file_name().to_string_lossy().contains(ASIDE))
            .expect("a quarantined copy");
        std::fs::read_to_string(entry.path()).unwrap()
    }
}
