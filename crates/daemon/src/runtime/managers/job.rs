//! The plumbing every worker manager shares: event publishing, job ids, and the
//! in-flight key set that admits one job per entry at a time.

use std::borrow::Borrow;
use std::collections::HashSet;
use std::hash::Hash;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use ipc::protocol::Event;
use proto::java::{JavaInstallPhase, JavaInstallProgress};
use proto::job::{Job, JobEvent, JobState};
use proto::minecraft::{ProvisionPhase, ProvisionProgress};

/// Below this ratio delta a progress update is dropped, matching Modrinth's
/// `emit_loading` (0.5%): a phase of thousands of tiny units (an instance's
/// assets) otherwise emits an event per unit and floods the socket.
const PROGRESS_EPSILON: f64 = 0.005;

/// A progress payload the coalescer can throttle: a phase discriminant (a
/// change forces a forward so the label switches promptly) and an overall
/// `0.0..=1.0` completion ratio.
pub(super) trait Coalescible {
    type Phase: Copy + PartialEq + Send;
    fn phase(&self) -> Self::Phase;
    fn ratio(&self) -> f64;
}

impl Coalescible for ProvisionProgress {
    type Phase = ProvisionPhase;
    fn phase(&self) -> ProvisionPhase {
        self.phase
    }
    fn ratio(&self) -> f64 {
        ProvisionProgress::ratio(self)
    }
}

impl Coalescible for JavaInstallProgress {
    type Phase = JavaInstallPhase;
    fn phase(&self) -> JavaInstallPhase {
        self.phase
    }
    fn ratio(&self) -> f64 {
        JavaInstallProgress::ratio(self)
    }
}

impl Coalescible for proto::download::DownloadProgress {
    type Phase = ();
    fn phase(&self) {}
    fn ratio(&self) -> f64 {
        match self.total {
            0 => 0.0,
            total => self.downloaded as f64 / total as f64,
        }
    }
}

/// Coalesce a high-frequency progress stream so a phase made of thousands of
/// tiny units (or a per-chunk download) can't flood every subscribed front-end
/// (the desktop re-renders per event; the freeze this fixes). An update is
/// forwarded only when its phase changes, its overall ratio advances past
/// `PROGRESS_EPSILON`, or it is terminal — so the bar still lands on 100% and
/// the label still switches promptly, while the intermediate ticks are dropped.
/// Mirrors the CLI, which throttles at its render layer instead.
pub(super) fn coalesce_progress<P, F>(emit: F) -> impl Fn(&P) + Send + Sync
where
    P: Coalescible,
    F: Fn(&P) + Send + Sync,
{
    let state = Mutex::new(None::<(P::Phase, f64)>);
    move |p: &P| {
        let ratio = p.ratio();
        let mut last = state.lock().unwrap();
        let forward = match *last {
            Some((phase, sent)) => {
                phase != p.phase() || ratio >= 1.0 || (ratio - sent).abs() > PROGRESS_EPSILON
            }
            None => true,
        };
        if forward {
            *last = Some((p.phase(), ratio));
            drop(last);
            emit(p);
        }
    }
}

fn job_event<C: Job>(id: &str, state: JobState<&C::Progress, &C::Done>) -> Event {
    let event = JobEvent {
        id: id.to_string(),
        channel: C::CHANNEL.to_string(),
        state,
    };
    Event {
        topic: proto::job::TOPIC.to_string(),
        payload: serde_json::to_value(&event).unwrap_or_default(),
    }
}

fn generate_id(prefix: &str) -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed) + 1;
    format!("{prefix}-{}-{}", std::process::id(), n)
}

/// The caller's job id, or a generated one when it left the id empty.
pub(super) fn job_id(id: String, prefix: &str) -> String {
    if id.is_empty() {
        generate_id(prefix)
    } else {
        id
    }
}

/// The keys whose job is still running. A key admits one job at a time.
pub(super) struct InFlight<K> {
    active: Arc<Mutex<HashSet<K>>>,
}

impl<K: Eq + Hash + Clone> InFlight<K> {
    pub(super) fn new() -> Self {
        InFlight {
            active: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    pub(super) fn contains<Q>(&self, key: &Q) -> bool
    where
        K: Borrow<Q>,
        Q: Eq + Hash + ?Sized,
    {
        self.active.lock().unwrap().contains(key)
    }

    /// Take `key`, or `None` when a job already holds it. The claim releases on
    /// drop, so a job that panics never wedges its key.
    pub(super) fn claim(&self, key: K) -> Option<Claim<K>> {
        if !self.active.lock().unwrap().insert(key.clone()) {
            return None;
        }
        Some(Claim {
            active: self.active.clone(),
            key,
        })
    }
}

pub(super) struct Claim<K: Eq + Hash> {
    active: Arc<Mutex<HashSet<K>>>,
    key: K,
}

impl<K: Eq + Hash> Drop for Claim<K> {
    fn drop(&mut self) {
        self.active.lock().unwrap().remove(&self.key);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use proto::java::{JavaInstallPhase, JavaInstallProgress};

    use super::*;

    fn downloading(current: u64, total: u64) -> JavaInstallProgress {
        JavaInstallProgress {
            phase: JavaInstallPhase::Downloading,
            current,
            total,
        }
    }

    #[test]
    fn coalesces_sub_epsilon_java_ticks() {
        let emitted = AtomicUsize::new(0);
        let forward = coalesce_progress(|_: &JavaInstallProgress| {
            emitted.fetch_add(1, Ordering::SeqCst);
        });

        // A per-chunk download flood: one tick per 0.1% of a large archive.
        for i in 0..=1000u64 {
            forward(&downloading(i, 1000));
        }

        // Far fewer than the 1001 ticks: forwarded only past each 0.5% step
        // (~200) plus the first and the terminal 100%.
        let count = emitted.load(Ordering::SeqCst);
        assert!(count > 0, "the first tick and completion must forward");
        assert!(
            count <= 210,
            "sub-epsilon ticks must be dropped, got {count}"
        );
    }

    /// A job invented for the tests: the runner never looks inside a payload,
    /// so one stand-in contract exercises every path.
    mod stand_in {
        use serde::{Deserialize, Serialize};

        #[derive(Serialize, Deserialize, Clone)]
        pub struct Progress {
            pub done: u64,
        }
        impl super::super::Coalescible for Progress {
            type Phase = ();
            fn phase(&self) {}
            fn ratio(&self) -> f64 {
                self.done as f64 / 1000.0
            }
        }

        #[derive(Serialize, Deserialize)]
        pub struct Done {}

        pub struct Test;
        impl proto::Contract for Test {
            const CHANNEL: &'static str = "test.start";
            type Params = proto::Empty;
            type Result = proto::job::JobStarted;
        }
        impl proto::job::Job for Test {
            type Progress = Progress;
            type Done = Done;
        }
    }

    struct Harness {
        runner: Runner<String>,
        events: tokio::sync::mpsc::UnboundedReceiver<String>,
        _home: tempfile::TempDir,
    }

    fn harness() -> Harness {
        let home = tempfile::tempdir().expect("temp home");
        let engine = Arc::new(engine::Engine::new(Some(home.path())));
        let hub = Arc::new(crate::runtime::EventHub::default());
        let (tx, events) = tokio::sync::mpsc::unbounded_channel();
        hub.subscribe(1, tx, None);
        Harness {
            runner: Runner::new(engine, hub, Cancellations::new()),
            events,
            _home: home,
        }
    }

    /// The spec for the stand-in job, with `key` varied per test —
    /// everything else the runner owns.
    fn spec(key: Option<String>) -> Spec<String> {
        Spec {
            id: String::new(),
            prefix: "test",
            key,
        }
    }

    fn start<W>(runner: &Runner<String>, key: Option<String>, work: W) -> Option<String>
    where
        W: for<'a> FnOnce(
                &'a engine::Engine,
                Reporter<'a, stand_in::Progress>,
            ) -> Work<'a, stand_in::Done>
            + Send
            + 'static,
    {
        runner.start::<stand_in::Test, _>(spec(key), work)
    }

    /// The state of every job event published so far, drained.
    async fn topics(events: &mut tokio::sync::mpsc::UnboundedReceiver<String>) -> Vec<String> {
        let mut seen = Vec::new();
        while let Ok(Some(frame)) =
            tokio::time::timeout(Duration::from_millis(500), events.recv()).await
        {
            let value: serde_json::Value = serde_json::from_str(&frame).expect("a frame");
            assert_eq!(value["event"], "job");
            assert_eq!(value["payload"]["channel"], "test.start");
            seen.push(
                value["payload"]["state"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
            );
        }
        seen
    }

    #[tokio::test]
    async fn a_key_admits_one_job_at_a_time() {
        let harness = harness();
        let (gate, held) = tokio::sync::oneshot::channel::<()>();

        let first = start(&harness.runner, Some("entry".into()), move |_, _| {
            Box::pin(async move {
                let _ = held.await;
                Ok(stand_in::Done {})
            })
        });
        assert!(first.is_some(), "the first job takes the key");

        let second = start(&harness.runner, Some("entry".into()), |_, _| {
            Box::pin(async { Ok(stand_in::Done {}) })
        });
        assert!(second.is_none(), "the second is refused while it is held");
        assert!(harness.runner.in_flight("entry"));

        let _ = gate.send(());
    }

    #[tokio::test]
    async fn a_finished_job_releases_its_key() {
        let mut harness = harness();
        start(&harness.runner, Some("entry".into()), |_, _| {
            Box::pin(async { Ok(stand_in::Done {}) })
        })
        .expect("started");
        topics(&mut harness.events).await;

        assert!(!harness.runner.in_flight("entry"));
        assert!(start(&harness.runner, Some("entry".into()), |_, _| {
            Box::pin(async { Ok(stand_in::Done {}) })
        })
        .is_some());
    }

    #[tokio::test]
    async fn a_keyless_job_never_refuses() {
        let harness = harness();
        for _ in 0..3 {
            assert!(start(&harness.runner, None, |_, _| Box::pin(async {
                tokio::task::yield_now().await;
                Ok(stand_in::Done {})
            }))
            .is_some());
        }
    }

    #[tokio::test]
    async fn progress_is_coalesced_and_the_job_settles_on_done() {
        let mut harness = harness();
        start(&harness.runner, None, |_, reporter| {
            Box::pin(async move {
                let report = reporter.checked();
                // A flood of sub-epsilon ticks, as an assets pass emits.
                for done in 0..=1000 {
                    report(&stand_in::Progress { done })?;
                }
                Ok(stand_in::Done {})
            })
        })
        .expect("started");

        let seen = topics(&mut harness.events).await;
        assert_eq!(seen.last().map(String::as_str), Some("done"));
        let progress = seen.iter().filter(|t| *t == "progress").count();
        assert!(
            (1..=210).contains(&progress),
            "every job is coalesced by the runner, got {progress} of 1001 ticks"
        );
    }

    #[tokio::test]
    async fn a_cancelled_job_settles_apart_from_a_failure() {
        let mut harness = harness();
        start(&harness.runner, None, |_, reporter| {
            Box::pin(async move {
                reporter.cancel().cancel();
                reporter.cancel().check()?;
                Ok(stand_in::Done {})
            })
        })
        .expect("started");

        assert_eq!(topics(&mut harness.events).await, vec!["cancelled"]);
    }

    #[tokio::test]
    async fn a_failed_job_settles_on_its_error_topic() {
        let mut harness = harness();
        start(&harness.runner, None, |_, _| {
            Box::pin(async { Err(anyhow::anyhow!("no")) })
        })
        .expect("started");

        assert_eq!(topics(&mut harness.events).await, vec!["failed"]);
    }

    #[test]
    fn forwards_on_phase_change() {
        let emitted = AtomicUsize::new(0);
        let forward = coalesce_progress(|_: &JavaInstallProgress| {
            emitted.fetch_add(1, Ordering::SeqCst);
        });

        forward(&JavaInstallProgress {
            phase: JavaInstallPhase::Resolving,
            current: 0,
            total: 0,
        });
        // Same zero ratio, new phase: a phase switch always forwards.
        forward(&downloading(0, 0));
        forward(&JavaInstallProgress {
            phase: JavaInstallPhase::Extracting,
            current: 0,
            total: 0,
        });

        assert_eq!(emitted.load(Ordering::SeqCst), 3);
    }
}

/// Every cancellable job currently running, keyed by its job id — the same id
/// its progress and terminal events carry, so `job.cancel` needs nothing else to
/// find it.
///
/// The registration guard releases on drop, exactly as [`Claim`] does, so a job
/// that panics leaves no token behind for a later job to inherit.
#[derive(Clone, Default)]
pub struct Cancellations {
    running: Arc<Mutex<std::collections::HashMap<String, engine::Cancel>>>,
}

impl Cancellations {
    pub fn new() -> Self {
        Cancellations::default()
    }

    /// Register `id` and hand back its token plus the guard that unregisters it.
    pub(super) fn register(&self, id: &str) -> (engine::Cancel, Registered) {
        let cancel = engine::Cancel::new();
        self.running
            .lock()
            .unwrap()
            .insert(id.to_string(), cancel.clone());
        (
            cancel,
            Registered {
                running: self.running.clone(),
                id: id.to_string(),
            },
        )
    }

    /// Raise `id`'s cancel flag. False when no such job is running — it already
    /// finished, or never existed; a normal race, not an error.
    pub fn cancel(&self, id: &str) -> bool {
        match self.running.lock().unwrap().get(id) {
            Some(cancel) => {
                cancel.cancel();
                tracing::info!(job = %id, "job cancellation requested");
                true
            }
            None => {
                tracing::debug!(job = %id, "nothing to cancel");
                false
            }
        }
    }
}

pub(super) struct Registered {
    running: Arc<Mutex<std::collections::HashMap<String, engine::Cancel>>>,
    id: String,
}

impl Drop for Registered {
    fn drop(&mut self) {
        self.running.lock().unwrap().remove(&self.id);
    }
}

/// What a job reports through and is cancelled by. The reporter is already
/// coalesced, so a job never decides how often to publish.
pub(super) struct Reporter<'a, P> {
    publish: &'a (dyn Fn(&P) + Send + Sync),
    cancel: &'a engine::Cancel,
}

impl<'a, P> Reporter<'a, P> {
    pub(super) fn cancel(&self) -> &'a engine::Cancel {
        self.cancel
    }

    /// A progress callback that checkpoints, for an engine call whose reporter
    /// is fallible (a download stops between chunks).
    pub(super) fn checked(&self) -> impl Fn(&P) -> anyhow::Result<()> + Send + Sync + '_ {
        move |progress: &P| {
            self.cancel.check()?;
            (self.publish)(progress);
            Ok(())
        }
    }
}

impl<'a> Reporter<'a, ProvisionProgress> {
    pub(super) fn job(&self) -> engine::Job<'a> {
        engine::Job::new(self.publish, self.cancel)
    }
}

type Publish<P> = Box<dyn Fn(&P) + Send + Sync>;
type Work<'a, T> =
    std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<T>> + Send + 'a>>;

/// What one job differs by besides its contract. Claiming, spawning,
/// coalescing, cancellation, terminal classification, event construction and
/// logging are the runner's.
pub(super) struct Spec<K> {
    /// The caller's job id; empty generates one from `prefix`.
    pub id: String,
    pub prefix: &'static str,
    /// The in-flight key. `None` admits any number of this job at once.
    pub key: Option<K>,
}

/// The plumbing every worker manager runs its jobs through.
pub(super) struct Runner<K> {
    engine: Arc<engine::Engine>,
    hub: Arc<crate::runtime::EventHub>,
    cancellations: Cancellations,
    active: InFlight<K>,
}

impl<K: Eq + Hash + Clone + Send + std::fmt::Debug + 'static> Runner<K> {
    pub(super) fn new(
        engine: Arc<engine::Engine>,
        hub: Arc<crate::runtime::EventHub>,
        cancellations: Cancellations,
    ) -> Self {
        Runner {
            engine,
            hub,
            cancellations,
            active: InFlight::new(),
        }
    }

    pub(super) fn in_flight<Q>(&self, key: &Q) -> bool
    where
        K: Borrow<Q>,
        Q: Eq + Hash + ?Sized,
    {
        self.active.contains(key)
    }

    /// Run `work` off-thread, publishing its progress and terminal outcome.
    /// Returns the job id, or `None` when the key is already claimed.
    pub(super) fn start<C, W>(&self, spec: Spec<K>, work: W) -> Option<String>
    where
        C: Job,
        C::Progress: Coalescible + Send + Sync + 'static,
        C::Done: Send + 'static,
        W: for<'a> FnOnce(&'a engine::Engine, Reporter<'a, C::Progress>) -> Work<'a, C::Done>
            + Send
            + 'static,
    {
        let id = job_id(spec.id, spec.prefix);
        let claim = match spec.key {
            Some(key) => match self.active.claim(key.clone()) {
                Some(claim) => Some(claim),
                None => {
                    tracing::debug!(key = ?key, kind = spec.prefix, "job already in flight");
                    return None;
                }
            },
            None => None,
        };

        let engine = self.engine.clone();
        let hub = self.hub.clone();
        let cancellations = self.cancellations.clone();
        let (job_id, kind) = (id.clone(), spec.prefix);
        tracing::info!(job = %id, kind, "job started");

        tokio::spawn(async move {
            let _claim = claim;
            let (cancel, _registered) = cancellations.register(&job_id);

            let publish_hub = hub.clone();
            let publish_id = job_id.clone();
            let publish: Publish<C::Progress> =
                Box::new(coalesce_progress(move |progress: &C::Progress| {
                    publish_hub.publish(&job_event::<C>(&publish_id, JobState::Progress(progress)));
                }));

            let outcome = work(
                &engine,
                Reporter {
                    publish: publish.as_ref(),
                    cancel: &cancel,
                },
            )
            .await;

            let event = match outcome {
                Ok(done) => {
                    tracing::info!(job = %job_id, kind, "job done");
                    job_event::<C>(&job_id, JobState::Done(&done))
                }
                Err(e) if engine::is_cancelled(&e) => {
                    tracing::info!(job = %job_id, kind, "job cancelled");
                    job_event::<C>(&job_id, JobState::Cancelled)
                }
                Err(e) => {
                    tracing::error!(job = %job_id, kind, error = format!("{e:#}"), "job failed");
                    job_event::<C>(
                        &job_id,
                        JobState::Failed {
                            error: crate::runtime::engine_error(e),
                        },
                    )
                }
            };
            hub.publish(&event);
        });
        Some(id)
    }
}
