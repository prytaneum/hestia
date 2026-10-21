//! Minecraft provider vocabulary shared by both sides of the socket. A *flavor*
//! is a distribution (vanilla, fabric, …); a provider lists the game *versions*
//! it supports. Servers and instances (clients) share this vocabulary; their
//! contracts live in the `server` and `instance` modules.

use serde::{Deserialize, Serialize};

use crate::content::ContentKind;
use crate::download::Checksum;
use crate::keys::KeyRule;

/// A distribution offered by a domain: the first level of the `available`
/// selector (`vanilla`, `fabric`, …).
///
/// Everything a front-end needs to render the choice travels with it — the
/// blurb explaining what it is and the content kinds an entry of it would
/// accept — so shipping a flavor is a daemon-side change alone. A front-end
/// that kept its own table of either would drift the moment one was added,
/// which is the same no-drift rule `ServerInfo::accepts` follows.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct Flavor {
    pub id: String,
    pub name: String,
    /// One line saying what this distribution is and what it costs to run,
    /// authored beside the provider. English; a front-end with its own
    /// translation for a known id may prefer it.
    pub summary: String,
    pub accepts: Vec<ContentKind>,
    /// What this flavor needs installed that is **not** on this machine; empty
    /// when it is ready to use. Resolved as the catalogue is built, so a
    /// front-end warns before the user commits rather than after.
    pub requires: Vec<Requirement>,
}

/// Something a flavor needs on the machine that Hestia cannot install itself —
/// named as the user would recognise it, with where to get it. A front-end
/// should never have to know what a given flavor depends on.
#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct Requirement {
    pub name: String,
    pub url: String,
}

/// One key/value setting, shared by the server and instance `config` channels
/// and their create-time settings list.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ConfigEntry {
    pub key: String,
    pub value: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum VersionKind {
    #[default]
    Release,
    Snapshot,
    OldBeta,
    OldAlpha,
}

/// A game version a flavor can target.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct GameVersion {
    pub id: String,
    pub kind: VersionKind,
    pub stable: bool,
}

/// A single downloadable file, the shared shape for every artifact in a profile.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct Artifact {
    pub url: String,
    pub filename: String,
    pub size: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checksum: Option<Checksum>,
}

/// Whether moving `from` → `to` is a downgrade, judged by their positions in a
/// provider's newest-first version list — the catalogue is the ordering ground
/// truth, so no version-string parsing can drift from upstream. `None` when
/// either version is not listed.
pub fn downgrade_between(versions: &[GameVersion], from: &str, to: &str) -> Option<bool> {
    let position = |id: &str| versions.iter().position(|v| v.id == id);
    Some(position(to)? > position(from)?)
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct FlavorsResult {
    pub flavors: Vec<Flavor>,
    pub rules: Vec<KeyRule>,
}

/// Where a provisioning job (server create, instance launch preparation) is.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum ProvisionPhase {
    #[default]
    Resolving,
    Backup,
    Java,
    Server,
    Client,
    Libraries,
    Assets,
    Content,
    /// Writing a modpack's own game-directory files (its `overrides/`).
    Overrides,
    /// Writing an instance's files into an export archive.
    Archive,
    /// Reading an imported archive's files back onto disk.
    Extract,
}

/// Progress for a provisioning job. `current`/`total` are bytes for a
/// single-artifact phase and completed/total counts for `Libraries`/`Assets`;
/// a phase with unknown extent reports `0/0`. A batch of files fetched side by
/// side (a content install, a modpack) reports the bytes of the whole batch;
/// when a file's size is not known up front it counts files instead, as `item`
/// (1-based) of `items`. Both are zero otherwise.
///
/// A launch plans its downloads up front and reports `overallCurrent` of
/// `overallTotal` bytes across every phase; the pair only moves forward and
/// stays at its last value through a phase that has no bytes. `0/0` means the
/// job has no plan, and `current`/`total` are all there is.
#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct ProvisionProgress {
    pub phase: ProvisionPhase,
    pub current: u64,
    pub total: u64,
    pub detail: String,
    pub item: u64,
    pub items: u64,
    pub overall_current: u64,
    pub overall_total: u64,
}

impl ProvisionProgress {
    /// Overall completion in `0.0..=1.0`. A multi-unit phase (`items > 0`)
    /// fills monotonically across the whole batch — completed units plus the
    /// current unit's fraction — so cached or instant units still advance the
    /// bar instead of snapping back on each per-unit reset. Shared by the CLI
    /// gauge, the daemon's progress coalescing, and the desktop's mirror.
    pub fn ratio(&self) -> f64 {
        if self.overall_total > 0 {
            return (self.overall_current as f64 / self.overall_total as f64).clamp(0.0, 1.0);
        }
        let unit = if self.total > 0 {
            (self.current as f64 / self.total as f64).clamp(0.0, 1.0)
        } else {
            0.0
        };
        if self.items > 0 {
            ((self.item.saturating_sub(1) as f64 + unit) / self.items as f64).clamp(0.0, 1.0)
        } else {
            unit
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn versions(ids: &[&str]) -> Vec<GameVersion> {
        ids.iter()
            .map(|id| GameVersion {
                id: id.to_string(),
                ..GameVersion::default()
            })
            .collect()
    }

    #[test]
    fn downgrade_follows_list_order() {
        let list = versions(&["1.21.1", "1.21", "1.20.4"]);
        assert_eq!(downgrade_between(&list, "1.20.4", "1.21.1"), Some(false));
        assert_eq!(downgrade_between(&list, "1.21.1", "1.20.4"), Some(true));
        assert_eq!(downgrade_between(&list, "1.21", "1.21"), Some(false));
    }

    #[test]
    fn a_planned_total_outranks_the_phase_extent() {
        let progress = ProvisionProgress {
            current: 1,
            total: 1,
            item: 3,
            items: 3,
            overall_current: 25,
            overall_total: 100,
            ..ProvisionProgress::default()
        };
        assert_eq!(progress.ratio(), 0.25);
    }
}
