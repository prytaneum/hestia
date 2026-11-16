//! Documents set aside because this build could not read them, as the warnings
//! a front-end shows.

use std::path::Path;

use common::schema::notices::{self, Mark, Quarantine};
use proto::warning::WarningInfo;

pub use notices::mark;

/// Everything this daemon has quarantined since it started.
pub fn all() -> Vec<WarningInfo> {
    notices::all().into_iter().map(warning).collect()
}

/// What was set aside since `mark` under `dir`, so a concurrent operation's
/// quarantine is not reported as this one's.
pub fn since_under(mark: Mark, dir: &Path) -> Vec<WarningInfo> {
    let prefix = dir.display().to_string();
    notices::since(mark)
        .into_iter()
        .filter(|quarantine| quarantine.path.starts_with(&prefix))
        .map(warning)
        .collect()
}

fn warning(quarantine: Quarantine) -> WarningInfo {
    WarningInfo::DocumentQuarantined {
        document: quarantine.document,
        path: quarantine.path,
        detail: quarantine.detail,
    }
}
