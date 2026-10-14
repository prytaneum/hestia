//! Fetching a batch of files side by side under one progress extent.

use std::collections::HashSet;
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};

use futures_util::StreamExt;
use proto::minecraft::{ProvisionPhase, ProvisionProgress};

use super::phase_progress;
use crate::cancel::Job;

const CONCURRENT_DOWNLOADS: usize = 8;

/// One progress extent across files that download concurrently: the bytes of
/// the whole batch when every size is known up front, completed files
/// otherwise.
pub(super) struct Batch<'a> {
    job: &'a Job<'a>,
    phase: ProvisionPhase,
    bytes: u64,
    files: u64,
    fetched: AtomicU64,
    finished: AtomicU64,
}

impl<'a> Batch<'a> {
    pub(super) fn new(
        job: &'a Job<'a>,
        phase: ProvisionPhase,
        sizes: impl IntoIterator<Item = u64>,
    ) -> Self {
        let sizes: Vec<u64> = sizes.into_iter().collect();
        let bytes = match sizes.contains(&0) {
            true => 0,
            false => sizes.iter().sum(),
        };
        Batch {
            job,
            phase,
            bytes,
            files: sizes.len() as u64,
            fetched: AtomicU64::new(0),
            finished: AtomicU64::new(0),
        }
    }

    /// One file's part of the batch, settled when the returned share drops.
    pub(super) fn share(&self, size: u64) -> Share<'_> {
        Share {
            batch: self,
            size,
            seen: AtomicU64::new(0),
        }
    }

    fn report(&self) {
        let mut progress = phase_progress(self.phase);
        if self.bytes > 0 {
            progress.current = self.fetched.load(Ordering::Relaxed).min(self.bytes);
            progress.total = self.bytes;
        } else {
            progress.item = (self.finished.load(Ordering::Relaxed) + 1).min(self.files);
            progress.items = self.files;
        }
        self.job.report(&progress);
    }
}

pub(super) struct Share<'a> {
    batch: &'a Batch<'a>,
    size: u64,
    seen: AtomicU64,
}

impl Share<'_> {
    pub(super) fn relay(&self, progress: &ProvisionProgress) {
        let before = self.seen.swap(progress.current, Ordering::Relaxed);
        self.batch
            .fetched
            .fetch_add(progress.current.saturating_sub(before), Ordering::Relaxed);
        self.batch.report();
    }
}

impl Drop for Share<'_> {
    // A file already on disk reports no bytes of its own, and a failed one
    // stops short: either way its share is settled, so the bar still ends.
    fn drop(&mut self) {
        let unreported = self.size.saturating_sub(*self.seen.get_mut());
        self.batch.fetched.fetch_add(unreported, Ordering::Relaxed);
        self.batch.finished.fetch_add(1, Ordering::Relaxed);
        self.batch.report();
    }
}

/// Outcomes arrive as they finish, not in the order given.
pub(super) async fn gather<F: Future>(fetches: impl IntoIterator<Item = F>) -> Vec<F::Output> {
    futures_util::stream::iter(fetches)
        .buffer_unordered(CONCURRENT_DOWNLOADS)
        .collect()
        .await
}

/// Fetch side by side, outcomes in the order given. Two fetches writing one
/// `target` would race on its staging file, so the later one waits its turn and
/// replaces the first.
pub(super) async fn gather_ordered<T, F, Fut>(
    items: Vec<T>,
    target: impl Fn(&T) -> Option<String>,
    fetch: F,
) -> Vec<Fut::Output>
where
    F: Fn(T) -> Fut,
    Fut: Future,
{
    let mut targets = HashSet::new();
    let (apart, repeats): (Vec<_>, Vec<_>) = items
        .into_iter()
        .enumerate()
        .partition(|(_, item)| target(item).is_none_or(|name| targets.insert(name)));
    let fetch = |(order, item): (usize, T)| {
        let fetched = fetch(item);
        async move { (order, fetched.await) }
    };
    let mut outcomes = gather(apart.into_iter().map(&fetch)).await;
    for repeat in repeats {
        outcomes.push(fetch(repeat).await);
    }
    outcomes.sort_by_key(|(order, _)| *order);
    outcomes.into_iter().map(|(_, outcome)| outcome).collect()
}
