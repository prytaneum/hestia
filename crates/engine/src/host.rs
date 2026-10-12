//! Facts about the machine the daemon runs on.

use std::sync::OnceLock;

use sysinfo::{MemoryRefreshKind, System};

/// Installed physical memory in bytes, or `None` where the platform reports
/// none — the signal to skip any limit derived from it.
pub fn total_memory() -> Option<u64> {
    static TOTAL: OnceLock<Option<u64>> = OnceLock::new();
    *TOTAL.get_or_init(|| {
        let mut system = System::new();
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
