//! What a launch still has to download, worked out before anything is fetched,
//! and the counter that reports bytes against that one total.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::Result;
use proto::minecraft::{Artifact, ProvisionProgress};

use super::materialize::safe_join;
use super::profile::Library;

/// One object of an asset index.
#[derive(Debug, Clone)]
pub struct AssetObject {
    pub name: String,
    pub hash: String,
    pub size: u64,
}

impl AssetObject {
    pub fn path(&self, objects_root: &Path) -> PathBuf {
        objects_root.join(&self.hash[..2]).join(&self.hash)
    }
}

pub struct PlanInput<'a> {
    pub client: &'a Artifact,
    pub client_jar: &'a Path,
    pub libraries: &'a [Library],
    pub libraries_root: &'a Path,
    pub assets: &'a [AssetObject],
    pub objects_root: &'a Path,
}

/// The files missing at plan time. The downloaders are handed these lists
/// rather than asking the disk again, so the plan and the download cannot
/// disagree about what is missing.
#[derive(Debug, Default)]
pub struct DownloadPlan {
    pub client: bool,
    pub libraries: Vec<Library>,
    pub assets: Vec<AssetObject>,
    pub bytes: u64,
}

/// Libraries not yet on disk, each path once: a modloader profile layered over
/// the base game names some twice.
pub fn missing_libraries(
    libraries: &[Library],
    root: &Path,
    is_present: &impl Fn(&Path, u64) -> bool,
) -> Result<Vec<Library>> {
    let mut seen = HashSet::new();
    let mut missing = Vec::new();
    for library in libraries {
        if seen.insert(library.path.as_str())
            && !is_present(&safe_join(root, &library.path)?, library.artifact.size)
        {
            missing.push(library.clone());
        }
    }
    Ok(missing)
}

pub fn plan_downloads(
    input: &PlanInput<'_>,
    is_present: impl Fn(&Path, u64) -> bool,
) -> Result<DownloadPlan> {
    let client = !is_present(input.client_jar, input.client.size);
    let libraries = missing_libraries(input.libraries, input.libraries_root, &is_present)?;
    let mut seen = HashSet::new();
    let assets: Vec<AssetObject> = input
        .assets
        .iter()
        .filter(|object| {
            seen.insert(object.hash.as_str())
                && !is_present(&object.path(input.objects_root), object.size)
        })
        .cloned()
        .collect();
    let bytes = if client { input.client.size } else { 0 }
        + libraries.iter().map(|l| l.artifact.size).sum::<u64>()
        + assets.iter().map(|a| a.size).sum::<u64>();
    Ok(DownloadPlan {
        client,
        libraries,
        assets,
        bytes,
    })
}

/// Bytes written so far against the planned total. It only moves forward and
/// is stamped on every event of the launch, so a phase with no bytes of its
/// own leaves it where it was.
pub struct Tally {
    total: u64,
    done: AtomicU64,
}

impl Tally {
    pub fn new(total: u64) -> Self {
        Tally {
            total,
            done: AtomicU64::new(0),
        }
    }

    pub fn stamp(&self, progress: &mut ProvisionProgress) {
        progress.overall_total = self.total;
        progress.overall_current = self.done.load(Ordering::Relaxed).min(self.total);
    }
}

/// One file's contribution to a [`Tally`]: fed the file's running byte count,
/// it adds only what is new.
pub struct Meter<'a> {
    tally: Option<&'a Tally>,
    seen: AtomicU64,
}

impl<'a> Meter<'a> {
    pub fn new(tally: Option<&'a Tally>) -> Self {
        Meter {
            tally,
            seen: AtomicU64::new(0),
        }
    }

    /// Record that `bytes` of this file are written; returns how many were new.
    pub fn reach(&self, bytes: u64) -> u64 {
        let delta = bytes.saturating_sub(self.seen.fetch_max(bytes, Ordering::Relaxed));
        if let Some(tally) = self.tally {
            tally.done.fetch_add(delta, Ordering::Relaxed);
        }
        delta
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn library(path: &str, size: u64) -> Library {
        Library {
            name: path.to_string(),
            path: path.to_string(),
            artifact: Artifact {
                size,
                ..Artifact::default()
            },
        }
    }

    fn asset(hash_digit: char, size: u64) -> AssetObject {
        AssetObject {
            name: format!("asset-{hash_digit}"),
            hash: hash_digit.to_string().repeat(40),
            size,
        }
    }

    #[test]
    fn the_plan_totals_only_what_is_missing_or_truncated() {
        let temp = tempfile::tempdir().unwrap();
        let (libs, objects) = (temp.path().join("libs"), temp.path().join("objects"));
        let jar = temp.path().join("client.jar");
        std::fs::create_dir_all(&libs).unwrap();
        std::fs::write(libs.join("good.jar"), vec![0; 10]).unwrap();
        std::fs::write(libs.join("truncated.jar"), vec![0; 3]).unwrap();
        let assets = [asset('a', 5), asset('b', 7), asset('b', 7)];
        let kept = assets[0].path(&objects);
        std::fs::create_dir_all(kept.parent().unwrap()).unwrap();
        std::fs::write(kept, vec![0; 5]).unwrap();

        let client = Artifact {
            size: 100,
            ..Artifact::default()
        };
        let libraries = [
            library("good.jar", 10),
            library("truncated.jar", 20),
            library("absent.jar", 30),
            library("absent.jar", 30),
        ];
        let plan = plan_downloads(
            &PlanInput {
                client: &client,
                client_jar: &jar,
                libraries: &libraries,
                libraries_root: &libs,
                assets: &assets,
                objects_root: &objects,
            },
            super::super::materialize::present,
        )
        .unwrap();

        assert!(plan.client);
        assert_eq!(plan.libraries.len(), 2);
        assert_eq!(plan.assets.len(), 1);
        assert_eq!(plan.bytes, 100 + 20 + 30 + 7);
    }

    #[test]
    fn a_meter_adds_each_byte_once_and_the_tally_never_exceeds_its_total() {
        let tally = Tally::new(10);
        let meter = Meter::new(Some(&tally));
        assert_eq!(meter.reach(4), 4);
        assert_eq!(meter.reach(4), 0);
        assert_eq!(meter.reach(12), 8);

        let mut progress = ProvisionProgress::default();
        tally.stamp(&mut progress);
        assert_eq!((progress.overall_current, progress.overall_total), (10, 10));
    }
}
