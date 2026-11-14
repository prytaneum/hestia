//! Facts about the machine the daemon runs on.

use std::sync::OnceLock;

use proto::host::HostInfoResult;
use sysinfo::{MemoryRefreshKind, System};

/// The facts about this machine a caller may always be told: operating system,
/// architecture and path separator in the vocabulary a plugin is queried with.
pub fn info() -> HostInfoResult {
    let target = crate::minecraft::rules::host_target(None);
    HostInfoResult {
        total_memory_bytes: total_memory(),
        os: target.os,
        arch: target.arch,
        path_separator: target.path_separator,
    }
}

/// Installed physical memory in bytes, or `None` where the platform reports
/// none — the signal to skip any limit derived from it.
pub fn total_memory() -> Option<u64> {
    static TOTAL: OnceLock<Option<u64>> = OnceLock::new();
    *TOTAL.get_or_init(|| {
        let mut system = match System::new() {
            Ok(system) => system,
            Err(error) => {
                tracing::warn!(%error, "cannot read installed memory; memory is left uncapped");
                return None;
            }
        };
        system.refresh_memory_specifics(MemoryRefreshKind::nothing().with_ram());
        let total = system.total_memory();
        if total == 0 {
            tracing::warn!("the platform reports no installed memory; memory is left uncapped");
            return None;
        }
        tracing::debug!(total_bytes = total, "read installed memory");
        Some(total)
    })
}
