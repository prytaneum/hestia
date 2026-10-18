//! Shared helpers for the disk-backed record stores (`servers`, `instances`):
//! each entry is a directory holding a JSON record, listing scans the parent —
//! the disk is the registry. [`Store`] is that lifecycle written once: lookup,
//! register, rename, remove, and a read-modify-write that holds one write lock,
//! so concurrent mutators never persist from a stale read.

use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{bail, Context, Result};
use proto::error::{EntryKind, ErrorInfo};
use uuid::Uuid;

use crate::schema::{self, Document};

/// `YYYYMMDD-HHMMSS` in UTC (Howard Hinnant's civil-from-days algorithm; no
/// date-time dependency for one format). What names a backup archive and an
/// exported instance — anything the disk registry sorts by time.
pub(crate) fn utc_stamp(unix: i64) -> String {
    let days = unix.div_euclid(86400);
    let secs = unix.rem_euclid(86400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}{month:02}{day:02}-{:02}{:02}{:02}",
        secs / 3600,
        (secs % 3600) / 60,
        secs % 60
    )
}

fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// A stable, opaque entry id (UUIDv7 hex): the entry's internal key — process
/// key, port/in-flight claims, process records — never a path component, so a
/// rename never touches it. Stays `[0-9a-f]`, never `_` (the `<id>_<seq>`
/// session-key scheme reserves it).
pub(crate) fn allocate_id(taken: impl Fn(&str) -> bool) -> Result<String> {
    for _ in 0..8 {
        let id = Uuid::now_v7().simple().to_string();
        if !taken(&id) {
            return Ok(id);
        }
    }
    bail!("could not allocate a unique entry id");
}

/// An entry's directory name: its display name slugged (unique via
/// [`name_taken`]), so a rename moves the directory. Falls back to the id for a
/// name with no sluggable characters, which create forbids.
pub(crate) fn dir_name(id: &str, name: &str) -> String {
    proto::naming::slugify(name).unwrap_or_else(|| id.to_string())
}

/// True when `name` collides with an existing entry's display name once both
/// are slugged — two entries must not reduce to the same slug, or a bare-name
/// reference would be ambiguous (`Modded` and `modded` are the same entry).
pub(crate) fn name_taken<'a>(name: &str, existing: impl IntoIterator<Item = &'a str>) -> bool {
    let Ok(slug) = slugify(name) else {
        return false;
    };
    existing
        .into_iter()
        .any(|other| slugify(other).map(|s| s == slug).unwrap_or(false))
}

/// Reduce a display name to a filesystem-safe slug (the shared rule lives in
/// `proto::naming`); it names the entry's directory ([`dir_name`]).
pub(crate) fn slugify(name: &str) -> Result<String> {
    match proto::naming::slugify(name) {
        Some(slug) => Ok(slug),
        None => bail!("name '{name}' has no usable characters"),
    }
}

/// Where a record lives: its directory plus the file name the document type
/// itself declares, so the two can never be paired wrongly at a call site.
pub(crate) fn record_path<T: Document>(dir: &Path) -> PathBuf {
    dir.join(T::NAME)
}

pub(crate) fn read_record<T: Document>(dir: &Path) -> Option<T> {
    schema::load(&record_path::<T>(dir))
}

pub(crate) fn write_record<T: Document>(dir: &Path, record: &T) -> Result<()> {
    schema::save(&record_path::<T>(dir), record)
}

/// Every record under `dir` (one subdirectory per entry, skipping any that has
/// no readable record — an unreadable one is set aside and reported by
/// [`schema::load`] rather than passed off as absent).
pub(crate) fn scan<T: Document>(dir: &Path) -> Vec<T> {
    let mut records = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                if let Some(record) = read_record(&entry.path()) {
                    records.push(record);
                }
            }
        }
    }
    records
}

/// A record the [`Store`] keeps: one entry directory named from its display name.
pub(crate) trait Entry: Document + Clone {
    const KIND: EntryKind;

    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn set_name(&mut self, name: String);
}

/// The record lifecycle shared by servers and instances. Every mutator takes
/// the one write lock, so a read-modify-write cannot lose a concurrent update.
pub(crate) struct Store<R: Entry> {
    dir: PathBuf,
    writes: Mutex<()>,
    records: PhantomData<fn() -> R>,
}

impl<R: Entry> Store<R> {
    pub fn new(dir: PathBuf) -> Self {
        Store {
            dir,
            writes: Mutex::new(()),
            records: PhantomData,
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn entry_dir(&self, record: &R) -> PathBuf {
        self.dir().join(dir_name(record.id(), record.name()))
    }

    pub fn list(&self) -> Vec<R> {
        let mut records: Vec<R> = scan(self.dir());
        records.sort_by(|a, b| a.name().cmp(b.name()));
        records
    }

    /// Find one entry by id or name (any spelling that slugs the same).
    pub fn get(&self, reference: &str) -> Option<R> {
        self.list()
            .into_iter()
            .find(|r| proto::naming::reference_matches(reference, r.id(), r.name()))
    }

    pub fn require(&self, reference: &str) -> Result<R> {
        self.get(reference)
            .with_context(|| format!("unknown {}: {reference}", R::KIND))
    }

    /// Add a new entry under a fresh id; `build` receives that id and returns
    /// the record to persist. Runs under the write lock, so it is where a claim
    /// that must not race (a port) belongs.
    pub fn register(&self, name: &str, build: impl FnOnce(String) -> Result<R>) -> Result<R> {
        let _writes = self.writes.lock().unwrap();
        if name_taken(name, self.list().iter().map(|r| r.name())) {
            bail!(ErrorInfo::AlreadyExists {
                entry: R::KIND,
                name: name.to_string()
            });
        }
        slugify(name)?;
        let id = allocate_id(|id| self.get(id).is_some())?;
        let record = build(id)?;
        let dir = self.entry_dir(&record);
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("cannot create {}", dir.display()))?;
        write_record(&dir, &record)?;
        Ok(record)
    }

    /// Rewrite the display name and move the directory to the new slug. The id
    /// is stable, so everything keyed by it is untouched.
    pub fn rename(&self, reference: &str, new_name: &str) -> Result<R> {
        let _writes = self.writes.lock().unwrap();
        let mut record = self.require(reference)?;
        if name_taken(
            new_name,
            self.list()
                .iter()
                .filter(|r| r.id() != record.id())
                .map(|r| r.name()),
        ) {
            bail!(ErrorInfo::AlreadyExists {
                entry: R::KIND,
                name: new_name.to_string()
            });
        }
        slugify(new_name)?;
        let old_dir = self.entry_dir(&record);
        record.set_name(new_name.to_string());
        let new_dir = self.entry_dir(&record);
        if new_dir != old_dir && old_dir.exists() {
            std::fs::rename(&old_dir, &new_dir).with_context(|| {
                format!("cannot move {} to {}", old_dir.display(), new_dir.display())
            })?;
        }
        write_record(&new_dir, &record)?;
        tracing::info!(kind = %R::KIND, id = %record.id(), name = %new_name, "entry renamed");
        Ok(record)
    }

    /// Delete the entry's directory, everything in it. False when none matches.
    pub fn remove(&self, reference: &str) -> Result<bool> {
        let _writes = self.writes.lock().unwrap();
        let Some(record) = self.get(reference) else {
            return Ok(false);
        };
        let dir = self.entry_dir(&record);
        std::fs::remove_dir_all(&dir)
            .with_context(|| format!("cannot remove {}", dir.display()))?;
        tracing::info!(kind = %R::KIND, id = %record.id(), "entry removed");
        Ok(true)
    }

    /// Read, change and persist one record under the write lock; nothing is
    /// written when `change` fails. `change` must not call back into a locking
    /// method of this store.
    pub fn mutate<T>(
        &self,
        reference: &str,
        change: impl FnOnce(&mut R) -> Result<T>,
    ) -> Result<T> {
        let _writes = self.writes.lock().unwrap();
        let mut record = self.require(reference)?;
        let outcome = change(&mut record)?;
        write_record(&self.entry_dir(&record), &record)?;
        Ok(outcome)
    }
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};

    use super::*;

    #[derive(Serialize, Deserialize, Default, Clone)]
    struct Note {
        id: String,
        name: String,
        count: u32,
    }

    impl Document for Note {
        const NAME: &'static str = "note.json";
    }

    impl Entry for Note {
        const KIND: EntryKind = EntryKind::Server;

        fn id(&self) -> &str {
            &self.id
        }

        fn name(&self) -> &str {
            &self.name
        }

        fn set_name(&mut self, name: String) {
            self.name = name;
        }
    }

    #[test]
    fn a_failed_mutation_persists_nothing() {
        let home = tempfile::tempdir().expect("temp dir");
        let store = Store::<Note>::new(home.path().to_path_buf());
        let note = store
            .register("alpha", |id| {
                Ok(Note {
                    id,
                    name: "alpha".into(),
                    count: 0,
                })
            })
            .unwrap();

        let failed: Result<()> = store.mutate("alpha", |n| {
            n.count = 9;
            bail!("nope")
        });
        assert!(failed.is_err());
        assert_eq!(store.require(&note.id).unwrap().count, 0);
    }

    #[test]
    fn stamps_are_utc_civil_dates() {
        assert_eq!(utc_stamp(0), "19700101-000000");
        assert_eq!(utc_stamp(1_751_852_045), "20250707-013405");
    }
}
