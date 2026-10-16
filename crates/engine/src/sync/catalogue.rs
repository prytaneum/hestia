//! The syncable units, the file each one is, and the version each needs.

use proto::sync::SyncUnit;

pub const ALL: &[SyncUnit] = &[
    SyncUnit::Options,
    SyncUnit::Servers,
    SyncUnit::Commands,
    SyncUnit::Hotbars,
    SyncUnit::Screenshots,
    SyncUnit::ResourcePacks,
];

pub const OPTIONS: &str = "options.txt";

/// `None` for a unit that shares no file: nothing is copied, so there is no
/// shared copy and no agreement to settle against.
pub fn file(unit: SyncUnit) -> Option<&'static str> {
    match unit {
        SyncUnit::Options => Some(OPTIONS),
        SyncUnit::Servers => Some("servers.dat"),
        SyncUnit::Commands => Some("command_history.txt"),
        SyncUnit::Hotbars => Some("hotbar.nbt"),
        SyncUnit::Screenshots | SyncUnit::ResourcePacks => None,
    }
}

/// Hotbars and the command history do not exist before their version; the
/// options catalogue translates every spelling, so options have no floor.
fn requires(unit: SyncUnit) -> Option<(u64, u64, u64)> {
    match unit {
        SyncUnit::Options => None,
        SyncUnit::Hotbars => Some((1, 12, 0)),
        SyncUnit::Commands => Some((1, 20, 2)),
        SyncUnit::Servers | SyncUnit::Screenshots | SyncUnit::ResourcePacks => None,
    }
}

pub fn requirement(unit: SyncUnit) -> String {
    match requires(unit) {
        Some((major, minor, 0)) => format!("{major}.{minor}"),
        Some((major, minor, patch)) => format!("{major}.{minor}.{patch}"),
        None => String::new(),
    }
}

/// An id that is not a release triple is a snapshot, which is modern.
pub fn supports(unit: SyncUnit, game_version: &str) -> bool {
    let Some(floor) = requires(unit) else {
        return true;
    };
    crate::version::parse(game_version).is_none_or(|version| version >= floor)
}

/// 1.20.5 replaced item tags with components; neither era reads the other.
const COMPONENT_ITEMS_SINCE: (u64, u64, u64) = (1, 20, 5);

pub fn era(unit: SyncUnit, game_version: &str) -> &'static str {
    if unit != SyncUnit::Hotbars {
        return "";
    }
    match crate::version::parse(game_version) {
        Some(version) if version < COMPONENT_ITEMS_SINCE => "legacy",
        _ => "components",
    }
}

/// Keys that describe the file, the machine or a moment, so a copy is wrong.
pub const NEVER_SHARED_KEYS: &[&str] = &[
    "version",
    "resourcePacks",
    "incompatibleResourcePacks",
    "lastServer",
    "soundDevice",
    "fullscreenResolution",
    "overrideWidth",
    "overrideHeight",
    "startedCleanly",
    "tutorialStep",
    "joinedFirstServer",
    "onboardAccessibility",
    "showInventoryAchievementHint",
    "skipRealms32bitWarning",
    "skipFriendsListPromo",
    "hideBundleTutorial",
    "skin",
    "difficulty",
];
