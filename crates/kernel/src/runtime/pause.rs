use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use tokio::time::Instant;

/// Time a call spends waiting on a person, which its budget does not count.
#[derive(Default)]
pub(super) struct Pause {
    clock: Mutex<Clock>,
}

#[derive(Default)]
struct Clock {
    since: Option<Instant>,
    total: Duration,
}

/// Keeps the call's budget paused while held.
pub(super) struct Waiting {
    pause: Arc<Pause>,
}

impl Pause {
    pub fn wait(self: &Arc<Self>) -> Waiting {
        self.clock().since = Some(Instant::now());
        Waiting {
            pause: self.clone(),
        }
    }

    /// The paused time so far, counting a wait still under way.
    pub fn paused(&self) -> Duration {
        let clock = self.clock();
        clock.total + clock.since.map_or(Duration::ZERO, |since| since.elapsed())
    }

    fn clock(&self) -> std::sync::MutexGuard<'_, Clock> {
        self.clock.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Drop for Waiting {
    fn drop(&mut self) {
        let mut clock = self.pause.clock();
        if let Some(since) = clock.since.take() {
            clock.total += since.elapsed();
        }
    }
}
