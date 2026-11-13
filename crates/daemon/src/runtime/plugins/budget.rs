//! How often the host may wake a plugin by itself.
//!
//! Every host-initiated call, a schedule tick or a delivered event, spends one
//! token from the plugin's bucket; the bucket refills at a steady rate. A burst
//! (a connection spewing frames, a handful of sign-ins in a row) is absorbed, a
//! sustained flood is not. Calls somebody asked for, a command, a page, a
//! provider operation, a hook, never come here.
//!
//! Time is passed in, so the limits are tested without waiting.

use std::time::{Duration, Instant};

/// Calls a plugin may be woken for in one burst.
pub const BURST: u32 = 30;
/// The steady rate after the burst: one call per period.
pub const REFILL_EVERY: Duration = Duration::from_secs(1);
/// How often the log says a plugin is being held back.
pub const WARN_EVERY: Duration = Duration::from_secs(60);

/// What woke the plugin, which decides what a refusal does to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wake {
    /// A schedule tick: a refused one is skipped, as if the machine had slept.
    Tick,
    /// A delivered event: a refused one waits, and the queue drops its oldest.
    Event,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Verdict {
    Run,
    Skip,
    Wait(Duration),
}

pub struct Budget {
    capacity: u32,
    every: Duration,
    tokens: u32,
    refilled_at: Instant,
    warned_at: Option<Instant>,
}

impl Budget {
    pub fn new(now: Instant) -> Self {
        Budget::with(BURST, REFILL_EVERY, now)
    }

    fn with(capacity: u32, every: Duration, now: Instant) -> Self {
        Budget {
            capacity,
            every,
            tokens: capacity,
            refilled_at: now,
            warned_at: None,
        }
    }

    pub fn judge(&mut self, wake: Wake, now: Instant) -> Verdict {
        self.refill(now);
        if self.tokens > 0 {
            self.tokens -= 1;
            return Verdict::Run;
        }
        match wake {
            Wake::Tick => Verdict::Skip,
            Wake::Event => Verdict::Wait(self.every.saturating_sub(now - self.refilled_at)),
        }
    }

    /// Says whether the log is due to mention a wake-up that was held back.
    pub fn note_throttled(&mut self, now: Instant) -> bool {
        let due = self
            .warned_at
            .is_none_or(|warned| now.duration_since(warned) >= WARN_EVERY);
        if due {
            self.warned_at = Some(now);
        }
        due
    }

    fn refill(&mut self, now: Instant) {
        let elapsed = now.saturating_duration_since(self.refilled_at);
        let periods = elapsed.as_nanos() / self.every.as_nanos();
        if periods == 0 {
            return;
        }
        let gained = u32::try_from(periods).unwrap_or(u32::MAX);
        self.tokens = self.tokens.saturating_add(gained).min(self.capacity);
        if self.tokens == self.capacity {
            self.refilled_at = now;
        } else {
            self.refilled_at += self.every * gained;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECOND: Duration = Duration::from_secs(1);

    fn spend(budget: &mut Budget, count: u32, now: Instant) {
        for _ in 0..count {
            assert_eq!(budget.judge(Wake::Event, now), Verdict::Run);
        }
    }

    #[test]
    fn a_burst_is_absorbed_and_the_next_call_is_refused() {
        let start = Instant::now();
        let mut budget = Budget::new(start);
        spend(&mut budget, BURST, start);
        assert!(matches!(budget.judge(Wake::Event, start), Verdict::Wait(_)));
    }

    #[test]
    fn it_refills_one_token_per_period_up_to_the_burst() {
        let start = Instant::now();
        let mut budget = Budget::new(start);
        spend(&mut budget, BURST, start);

        spend(&mut budget, 1, start + SECOND);
        assert!(matches!(
            budget.judge(Wake::Event, start + SECOND),
            Verdict::Wait(_)
        ));
        spend(&mut budget, 3, start + SECOND * 4);

        let later = start + SECOND * 10_000;
        spend(&mut budget, BURST, later);
        assert!(matches!(budget.judge(Wake::Event, later), Verdict::Wait(_)));
    }

    #[test]
    fn a_refused_event_waits_for_the_next_token() {
        let start = Instant::now();
        let mut budget = Budget::new(start);
        spend(&mut budget, BURST, start);
        let halfway = start + SECOND / 2;
        assert_eq!(
            budget.judge(Wake::Event, halfway),
            Verdict::Wait(SECOND / 2)
        );
        assert_eq!(budget.judge(Wake::Event, start + SECOND), Verdict::Run);
    }

    #[test]
    fn a_refused_tick_is_skipped_and_never_owed() {
        let start = Instant::now();
        let mut budget = Budget::new(start);
        spend(&mut budget, BURST, start);
        assert_eq!(budget.judge(Wake::Tick, start), Verdict::Skip);
        assert_eq!(budget.judge(Wake::Tick, start + SECOND / 2), Verdict::Skip);
        assert_eq!(budget.judge(Wake::Tick, start + SECOND), Verdict::Run);
        assert_eq!(budget.judge(Wake::Tick, start + SECOND), Verdict::Skip);
    }

    #[test]
    fn the_warning_is_rate_limited() {
        let start = Instant::now();
        let mut budget = Budget::new(start);
        assert!(budget.note_throttled(start));
        assert!(!budget.note_throttled(start + WARN_EVERY / 2));
        assert!(budget.note_throttled(start + WARN_EVERY));
    }
}
