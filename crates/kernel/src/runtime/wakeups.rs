use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

/// A burst of this many wake-ups, then one more each second.
const BURST: f64 = 30.0;
const REFILL_PER_SECOND: f64 = 1.0;

/// What a plugin may start on its own. One instance counts each job it starts
/// and each schedule tick, so a plugin cannot loop itself awake; another counts
/// the events it hears, so a flood of them never starves those.
#[derive(Default)]
pub(super) struct Wakeups {
    buckets: Mutex<HashMap<String, (f64, Instant)>>,
}

impl Wakeups {
    pub fn spend(&self, plugin: &str) -> bool {
        self.take(plugin).is_ok()
    }

    /// Spends one, or answers how long until one is refilled.
    pub fn take(&self, plugin: &str) -> Result<(), Duration> {
        let now = Instant::now();
        let mut buckets = self.buckets.lock().unwrap_or_else(PoisonError::into_inner);
        let (tokens, last) = buckets.entry(plugin.to_owned()).or_insert((BURST, now));
        let refilled = *tokens + now.duration_since(*last).as_secs_f64() * REFILL_PER_SECOND;
        *tokens = refilled.min(BURST);
        *last = now;
        match *tokens >= 1.0 {
            true => {
                *tokens -= 1.0;
                Ok(())
            }
            false => Err(Duration::from_secs_f64((1.0 - *tokens) / REFILL_PER_SECOND)),
        }
    }
}
