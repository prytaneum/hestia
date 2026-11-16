use std::time::Duration;

use wasmtime::ResourceLimiter;

pub(super) const TICK: Duration = Duration::from_millis(10);

const TABLE_ELEMENTS: usize = 100_000;

/// The bounds every plugin call runs under.
#[derive(Debug, Clone)]
pub struct Limits {
    pub(super) init_timeout: Duration,
    pub(super) event_timeout: Duration,
    pub(super) memory_bytes: usize,
    pub(super) calls_per_plugin: usize,
    pub(super) calls_total: usize,
    pub(super) traps_to_disable: usize,
    pub(super) trap_window: Duration,
}

impl Default for Limits {
    /// 10 s for `init`, 5 s for an event such as `settings-changed`, 256 MiB per memory, 4 calls per plugin and 16 overall,
    /// three traps in ten minutes.
    fn default() -> Self {
        Limits {
            init_timeout: Duration::from_secs(10),
            event_timeout: Duration::from_secs(5),
            memory_bytes: 256 << 20,
            calls_per_plugin: 4,
            calls_total: 16,
            traps_to_disable: 3,
            trap_window: Duration::from_secs(10 * 60),
        }
    }
}

impl Limits {
    /// The wall-clock budget of every call kind, including any wait for a call
    /// slot and excluding any wait on a person.
    pub fn with_timeout(self, budget: Duration) -> Self {
        Limits {
            init_timeout: budget,
            event_timeout: budget,
            ..self
        }
    }
}

/// A refused growth is reported to the guest as a failed `memory.grow`; the flag
/// lets the host tell that trap from any other.
pub(super) struct MemoryCap {
    max: usize,
    pub exceeded: bool,
}

impl MemoryCap {
    pub fn new(max: usize) -> Self {
        MemoryCap {
            max,
            exceeded: false,
        }
    }
}

impl ResourceLimiter for MemoryCap {
    fn memory_growing(
        &mut self,
        _current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        let allowed = desired <= self.max;
        self.exceeded |= !allowed;
        Ok(allowed)
    }

    fn table_growing(
        &mut self,
        _current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        Ok(desired <= TABLE_ELEMENTS)
    }
}
