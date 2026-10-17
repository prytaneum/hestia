//! A batch of changes to an entry's content index, committed once. Open it
//! after the downloads have finished, so the index it holds is current; nothing
//! reaches disk until [`Pool::commit`], which saves the index before deleting any
//! file it no longer names. A crash in between leaves an orphan file, never an
//! index naming a file that is gone.

use std::path::Path;

use anyhow::Result;
use proto::content::InstalledContent;

use super::install;

pub(crate) struct Pool<'a> {
    entry_dir: &'a Path,
    data_dir: &'a Path,
    worlds: Vec<String>,
    items: Vec<InstalledContent>,
    superseded: Vec<InstalledContent>,
    dirty: bool,
}

impl<'a> Pool<'a> {
    pub(crate) fn open(entry_dir: &'a Path, data_dir: &'a Path, worlds: Vec<String>) -> Self {
        Pool {
            entry_dir,
            data_dir,
            worlds,
            items: install::load(entry_dir),
            superseded: Vec::new(),
            dirty: false,
        }
    }

    /// The row `item` would take over: the same kind and either the same project
    /// or the same filename.
    pub(crate) fn holder_of(&self, item: &InstalledContent) -> Option<&InstalledContent> {
        install::replaces(&self.items, item).map(|pos| &self.items[pos])
    }

    /// Mirror `item` into the game and record it, in the row it takes over or
    /// as a new one.
    pub(crate) fn upsert(&mut self, item: InstalledContent) -> Result<()> {
        let position = install::replaces(&self.items, &item);
        self.put(position, item)
    }

    /// Mirror `item` into the game and record it in the row that `old` names.
    pub(crate) fn replace(&mut self, old: &InstalledContent, item: InstalledContent) -> Result<()> {
        let position = self
            .items
            .iter()
            .position(|i| i.kind == old.kind && i.filename == old.filename);
        self.put(position, item)
    }

    /// Take every row `doomed` names out of the index and its files off disk.
    pub(crate) fn drop_where(&mut self, doomed: impl Fn(&InstalledContent) -> bool) {
        let (dropped, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut self.items)
            .into_iter()
            .partition(|item| doomed(item));
        self.dirty |= !dropped.is_empty();
        self.items = kept;
        self.superseded.extend(dropped);
    }

    pub(crate) fn commit(self) -> Result<()> {
        if !self.dirty {
            return Ok(());
        }
        install::save(self.entry_dir, self.items)?;
        for old in &self.superseded {
            install::remove_files(self.entry_dir, self.data_dir, old, &self.worlds);
        }
        Ok(())
    }

    fn put(&mut self, position: Option<usize>, item: InstalledContent) -> Result<()> {
        install::apply_files(self.entry_dir, self.data_dir, &item, &self.worlds)?;
        self.dirty = true;
        match position {
            Some(position) => {
                let old = std::mem::replace(&mut self.items[position], item);
                let renamed = old.filename != self.items[position].filename;
                if renamed {
                    self.superseded.push(old);
                }
            }
            None => self.items.push(item),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use proto::content::ContentKind;

    use super::*;

    fn sodium(filename: &str) -> InstalledContent {
        InstalledContent {
            kind: ContentKind::Mod,
            project_id: "sodium".to_string(),
            filename: filename.to_string(),
            enabled: true,
            ..InstalledContent::default()
        }
    }

    fn write_managed(entry: &Path, item: &InstalledContent) {
        let path = install::managed_path(entry, item).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, item.filename.as_bytes()).unwrap();
    }

    #[test]
    fn the_superseded_file_goes_only_after_the_index_names_its_successor() {
        let dir = tempfile::tempdir().unwrap();
        let entry = dir.path().join("entry");
        let data = entry.join("data");
        let (old, new) = (sodium("sodium-1.jar"), sodium("sodium-2.jar"));
        write_managed(&entry, &old);
        write_managed(&entry, &new);
        install::save(&entry, vec![old.clone()]).unwrap();

        let mut pool = Pool::open(&entry, &data, Vec::new());
        pool.replace(&old, new.clone()).unwrap();
        assert!(
            install::managed_path(&entry, &old).unwrap().is_file(),
            "the old file stays until the commit"
        );
        pool.commit().unwrap();

        let index = install::load(&entry);
        assert_eq!(index.len(), 1);
        assert_eq!(index[0].filename, "sodium-2.jar");
        assert!(!install::managed_path(&entry, &old).unwrap().exists());
        assert!(install::managed_path(&entry, &new).unwrap().is_file());
    }
}
