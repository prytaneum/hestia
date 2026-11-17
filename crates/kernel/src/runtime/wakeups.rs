use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

/// A burst of this many wake-ups, then one more each second.
pub(super) const BURST: f64 = 30.0;
pub(super) const REFILL_PER_SECOND: f64 = 1.0;

/// What a plugin may start on its own. One instance counts each job it starts
/// and each schedule tick, so a plugin cannot loop itself awake; another counts
/// the events it hears, so a flood of them never starves those.
pub(super) struct Wakeups {
    burst: f64,
    refill_per_second: f64,
    buckets: Mutex<HashMap<String, (f64, Instant)>>,
}

impl Default for Wakeups {
    fn default() -> Self {
        Wakeups::new(BURST, REFILL_PER_SECOND)
    }
}

impl Wakeups {
    pub fn new(burst: f64, refill_per_second: f64) -> Self {
        Wakeups {
            burst,
            refill_per_second,
            buckets: Mutex::default(),
        }
    }

    pub fn spend(&self, plugin: &str) -> bool {
        self.take(plugin).is_ok()
    }

    /// Spends one, or answers how long until one is refilled.
    pub fn take(&self, plugin: &str) -> Result<(), Duration> {
        let now = Instant::now();
        let mut buckets = self.buckets.lock().unwrap_or_else(PoisonError::into_inner);
        let (tokens, last) = buckets
            .entry(plugin.to_owned())
            .or_insert((self.burst, now));
        let refilled = *tokens + now.duration_since(*last).as_secs_f64() * self.refill_per_second;
        *tokens = refilled.min(self.burst);
        *last = now;
        match *tokens >= 1.0 {
            true => {
                *tokens -= 1.0;
                Ok(())
            }
            false => Err(Duration::from_secs_f64(
                (1.0 - *tokens) / self.refill_per_second,
            )),
        }
    }
}
