//! The screenshots the instances have taken, read where they already are.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use proto::screenshot::Screenshot;
use proto::sync::SyncUnit;

use crate::engine::Engine;
use crate::instances::InstanceRecord;

const DIR: &str = "screenshots";

impl Engine {
    pub fn screenshots(&self, reference: &str) -> Result<Vec<Screenshot>> {
        let records = match reference.trim() {
            "" => self
                .instances
                .list()
                .into_iter()
                .filter(|record| self.shares_screenshots(record))
                .collect(),
            reference => vec![self.instances.require(reference)?],
        };
        let mut taken: Vec<Screenshot> = records
            .iter()
            .flat_map(|record| self.taken_by(record))
            .collect();
        taken.sort_by(|a, b| b.taken_unix.cmp(&a.taken_unix).then(a.file.cmp(&b.file)));
        Ok(taken)
    }

    pub fn delete_screenshot(&self, reference: &str, file: &str) -> Result<()> {
        let record = self.instances.require(reference)?;
        let path = self.screenshot_dir(&record).join(safe_name(file)?);
        if !path.is_file() {
            bail!("no screenshot '{file}' in '{}'", record.name);
        }
        std::fs::remove_file(&path).with_context(|| format!("cannot remove {}", path.display()))
    }

    fn shares_screenshots(&self, record: &InstanceRecord) -> bool {
        self.sync.enabled(SyncUnit::Screenshots) && record.sharing.shares(SyncUnit::Screenshots)
    }

    fn screenshot_dir(&self, record: &InstanceRecord) -> PathBuf {
        self.instances.data_dir(record).join(DIR)
    }

    fn taken_by(&self, record: &InstanceRecord) -> Vec<Screenshot> {
        let dir = self.screenshot_dir(record);
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return Vec::new();
        };
        entries
            .flatten()
            .filter(|entry| is_image(&entry.path()))
            .map(|entry| {
                let meta = entry.metadata().ok();
                Screenshot {
                    instance: record.id.clone(),
                    instance_name: record.name.clone(),
                    file: entry.file_name().to_string_lossy().into_owned(),
                    path: entry.path(),
                    taken_unix: meta
                        .as_ref()
                        .and_then(common::modified_unix)
                        .unwrap_or_default(),
                    bytes: meta.map(|meta| meta.len()).unwrap_or_default(),
                }
            })
            .collect()
    }
}

fn is_image(path: &Path) -> bool {
    path.is_file()
        && path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("png"))
}

/// A name from a client is a file in one directory, never a path into another.
fn safe_name(file: &str) -> Result<&str> {
    let name = file.trim();
    if name.is_empty() || name.contains(['/', '\\']) || name.contains("..") {
        bail!("'{file}' is not a screenshot name");
    }
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_that_climbs_out_of_the_folder_is_refused() {
        assert!(safe_name("shot.png").is_ok());
        for name in ["", "../shot.png", "a/b.png", "..\\b.png"] {
            assert!(safe_name(name).is_err(), "{name}");
        }
    }
}
