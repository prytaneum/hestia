//! What a plugin's `data` import does: named locks over its own state, so a
//! read-modify-write of a `data/` file is atomic across the calls of one plugin.
//! The sandbox has no file locks, so the host holds them. A lock belongs to the
//! call that took it and is released when the call's store is dropped, whatever
//! ended the call.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};
use tokio::time::{timeout, Instant};

const MAX_NAMES: usize = 64;
const MAX_HELD: usize = 8;
const NAME_LENGTH: std::ops::RangeInclusive<usize> = 1..=64;
const MAX_WAIT: Duration = Duration::from_secs(5);

#[derive(Debug, thiserror::Error)]
pub(crate) enum LockError {
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    Busy(String),
}

/// One plugin's lock names, shared by all its calls.
#[derive(Default)]
pub(crate) struct Locks {
    names: Mutex<HashMap<String, Arc<AsyncMutex<()>>>>,
}

/// The locks one call holds. Dropped with the call's store.
#[derive(Default)]
pub(crate) struct Held(HashMap<String, OwnedMutexGuard<()>>);

impl Locks {
    /// Takes `name` for the call, waiting at most [`MAX_WAIT`] and never past
    /// `deadline`, the end of the call.
    pub(crate) async fn acquire(
        &self,
        held: &mut Held,
        name: &str,
        deadline: Instant,
    ) -> Result<(), LockError> {
        if !NAME_LENGTH.contains(&name.len())
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        {
            return Err(LockError::Invalid(format!(
                "`{name}` is not a lock name: use 1 to 64 of A-Z, a-z, 0-9, - and _"
            )));
        }
        if held.0.contains_key(name) {
            return Err(LockError::Invalid(format!(
                "this call already holds `{name}`"
            )));
        }
        if held.0.len() >= MAX_HELD {
            return Err(LockError::Invalid(format!(
                "a call holds at most {MAX_HELD} locks"
            )));
        }
        let lock = self.named(name)?;
        let wait = MAX_WAIT.min(deadline.saturating_duration_since(Instant::now()));
        match timeout(wait, lock.lock_owned()).await {
            Ok(guard) => {
                held.0.insert(name.to_string(), guard);
                Ok(())
            }
            Err(_) => Err(LockError::Busy(format!("`{name}` is held by another call"))),
        }
    }

    pub(crate) fn release(&self, held: &mut Held, name: &str) -> Result<(), LockError> {
        held.0
            .remove(name)
            .map(drop)
            .ok_or_else(|| LockError::Invalid(format!("this call does not hold `{name}`")))
    }

    fn named(&self, name: &str) -> Result<Arc<AsyncMutex<()>>, LockError> {
        let mut names = self.names.lock().unwrap();
        if let Some(lock) = names.get(name) {
            return Ok(lock.clone());
        }
        if names.len() >= MAX_NAMES {
            return Err(LockError::Invalid(format!(
                "a plugin uses at most {MAX_NAMES} lock names"
            )));
        }
        Ok(names.entry(name.to_string()).or_default().clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn soon() -> Instant {
        Instant::now() + Duration::from_secs(30)
    }

    #[tokio::test(start_paused = true)]
    async fn a_second_call_waits_for_the_first_to_let_go() {
        let locks = Arc::new(Locks::default());
        let mut first = Held::default();
        locks.acquire(&mut first, "counter", soon()).await.unwrap();

        let waiter = tokio::spawn({
            let locks = locks.clone();
            async move {
                let mut second = Held::default();
                locks.acquire(&mut second, "counter", soon()).await
            }
        });
        tokio::time::sleep(Duration::from_secs(1)).await;
        assert!(!waiter.is_finished());

        locks.release(&mut first, "counter").unwrap();
        waiter.await.unwrap().unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn the_wait_is_bounded_and_ends_busy() {
        let locks = Locks::default();
        let mut first = Held::default();
        locks.acquire(&mut first, "counter", soon()).await.unwrap();

        let mut second = Held::default();
        let refused = locks.acquire(&mut second, "counter", soon()).await;
        assert!(matches!(refused, Err(LockError::Busy(_))));
    }

    #[tokio::test(start_paused = true)]
    async fn dropping_a_calls_locks_releases_them() {
        let locks = Locks::default();
        let mut first = Held::default();
        locks.acquire(&mut first, "counter", soon()).await.unwrap();
        drop(first);

        let mut second = Held::default();
        locks.acquire(&mut second, "counter", soon()).await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn names_are_checked_and_a_hold_is_not_reentrant() {
        let locks = Locks::default();
        let mut held = Held::default();
        for bad in ["", "a/b", "a.b", &"x".repeat(65)] {
            let refused = locks.acquire(&mut held, bad, soon()).await;
            assert!(matches!(refused, Err(LockError::Invalid(_))), "{bad}");
        }
        locks.acquire(&mut held, "a", soon()).await.unwrap();
        assert!(matches!(
            locks.acquire(&mut held, "a", soon()).await,
            Err(LockError::Invalid(_))
        ));
        assert!(matches!(
            locks.release(&mut held, "b"),
            Err(LockError::Invalid(_))
        ));
    }

    #[tokio::test(start_paused = true)]
    async fn the_number_of_names_and_of_holds_is_capped() {
        let locks = Locks::default();
        for n in 0..MAX_NAMES {
            let mut held = Held::default();
            locks
                .acquire(&mut held, &format!("n{n}"), soon())
                .await
                .unwrap();
        }
        let mut held = Held::default();
        assert!(matches!(
            locks.acquire(&mut held, "one-more", soon()).await,
            Err(LockError::Invalid(_))
        ));

        for n in 0..MAX_HELD {
            locks
                .acquire(&mut held, &format!("n{n}"), soon())
                .await
                .unwrap();
        }
        assert!(matches!(
            locks.acquire(&mut held, "n20", soon()).await,
            Err(LockError::Invalid(_))
        ));
    }
}
