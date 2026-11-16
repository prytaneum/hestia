use std::sync::atomic::Ordering;

use super::bindings::hestia::kernel::progress;
use super::host::State;

impl progress::Host for State {
    fn report(&mut self, current: u64, total: u64, message: String) {
        let message = &message[..message.floor_char_boundary(1024)];
        match &self.call.job {
            Some(job) => self
                .shared
                .services
                .progress(&self.plugin, job, current, total, message),
            None => {
                tracing::trace!(plugin = %self.plugin, current, total, "progress outside a job")
            }
        }
    }

    fn cancelled(&mut self) -> bool {
        self.call.cancelled.load(Ordering::Relaxed)
    }
}
