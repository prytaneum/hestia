use std::time::Duration;

use wasmtime::ResourceLimiter;

pub(super) const TICK: Duration = Duration::from_millis(10);

const TABLE_ELEMENTS: usize = 100_000;

/// The bounds every plugin call runs under.
#[derive(Debug, Clone)]
pub struct Limits {
    pub(super) init_timeout: Duration,
    pub(super) event_timeout: Duration,
    #[cfg(feature = "probe")]
    pub(super) hook_timeout: Duration,
    pub(super) command_timeout: Duration,
    #[cfg(feature = "probe")]
    pub(super) event_burst: f64,
    #[cfg(feature = "probe")]
    pub(super) event_refill_per_second: f64,
    pub(super) cancel_grace: Duration,
    pub(super) notice_burst: f64,
    pub(super) notice_refill_per_second: f64,
    pub(super) memory_bytes: usize,
    pub(super) calls_per_plugin: usize,
    pub(super) calls_total: usize,
    pub(super) traps_to_disable: usize,
    pub(super) trap_window: Duration,
}

impl Default for Limits {
    /// 10 s for `init`, 5 s for an event such as `settings-changed`, 2 s for a
    /// hook, an hour for a command and 5 s more once its job is cancelled,
    /// 256 MiB per memory, 10 notices then one more every 5 s, 4 calls per plugin and 16 overall, three traps in
    /// ten minutes.
    fn default() -> Self {
        Limits {
            init_timeout: Duration::from_secs(10),
            event_timeout: Duration::from_secs(5),
            #[cfg(feature = "probe")]
            hook_timeout: Duration::from_secs(2),
            command_timeout: Duration::from_secs(60 * 60),
            #[cfg(feature = "probe")]
            event_burst: super::wakeups::BURST,
            #[cfg(feature = "probe")]
            event_refill_per_second: super::wakeups::REFILL_PER_SECOND,
            cancel_grace: Duration::from_secs(5),
            notice_burst: super::notify::BURST,
            notice_refill_per_second: super::notify::REFILL_PER_SECOND,
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
            #[cfg(feature = "probe")]
            hook_timeout: budget,
            command_timeout: budget,
            ..self
        }
    }

    /// The deliveries a plugin may hear at once, and how many more it earns each
    /// second.
    #[cfg(feature = "probe")]
    pub fn with_event_budget(self, burst: f64, refill_per_second: f64) -> Self {
        Limits {
            event_burst: burst,
            event_refill_per_second: refill_per_second,
            ..self
        }
    }

    /// The notices a plugin may show at once across its calls, and how many more
    /// it earns each second.
    pub fn with_notice_budget(self, burst: f64, refill_per_second: f64) -> Self {
        Limits {
            notice_burst: burst,
            notice_refill_per_second: refill_per_second,
            ..self
        }
    }

    /// How long a command may keep running after its job is cancelled before the
    /// call is dropped.
    pub fn with_cancel_grace(self, grace: Duration) -> Self {
        Limits {
            cancel_grace: grace,
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
