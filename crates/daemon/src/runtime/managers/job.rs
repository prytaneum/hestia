//! The plumbing every job family shares: event publishing, job ids, and the
//! [`Jobs`] value each family hangs its `start_*` function off.

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use ipc::protocol::Event;
use proto::java::{JavaInstallPhase, JavaInstallProgress};
use proto::job::{Job, JobEvent, JobState};
use proto::minecraft::{ProvisionPhase, ProvisionProgress};

use crate::runtime::{Cancellations, Claim, Claims, ProcessSupervisor};

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
        subject: None,
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

/// What one job differs by besides its contract. Spawning, coalescing,
/// cancellation, terminal classification, event construction and logging are
/// the runner's.
pub(super) struct Spec {
    /// The caller's job id; empty generates one from `prefix`.
    pub id: String,
    pub prefix: &'static str,
    /// What the job holds for as long as it runs. `None` admits any number of
    /// this job at once.
    pub claim: Option<Claim>,
}

/// Every job family, one `start_*` function each (in the family's own file),
/// over the collaborators they share.
pub struct Jobs {
    engine: Arc<engine::Engine>,
    hub: Arc<crate::runtime::EventHub>,
    cancellations: Cancellations,
    pub(super) claims: Claims,
    pub(super) processes: Arc<ProcessSupervisor>,
    /// Session ids reserved between seq allocation and the supervisor accepting
    /// them, so two concurrent launches of one instance can't collide on a seq.
    pub(super) reserved: Arc<Mutex<HashSet<String>>>,
}

impl Jobs {
    pub fn new(
        engine: Arc<engine::Engine>,
        hub: Arc<crate::runtime::EventHub>,
        claims: Claims,
        processes: Arc<ProcessSupervisor>,
        cancellations: Cancellations,
    ) -> Self {
        Jobs {
            engine,
            hub,
            cancellations,
            claims,
            processes,
            reserved: Arc::default(),
        }
    }

    /// Run `work` off-thread, publishing its progress and terminal outcome.
    /// Returns the job id.
    pub(super) fn run<C, W>(&self, spec: Spec, work: W) -> String
    where
        C: Job,
        C::Progress: Coalescible + Send + Sync + 'static,
        C::Done: Send + 'static,
        W: for<'a> FnOnce(&'a engine::Engine, Reporter<'a, C::Progress>) -> Work<'a, C::Done>
            + Send
            + 'static,
    {
        let id = job_id(spec.id, spec.prefix);
        let claim = spec.claim;
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
        id
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
        jobs: Jobs,
        events: tokio::sync::mpsc::UnboundedReceiver<String>,
        _home: tempfile::TempDir,
    }

    fn harness() -> Harness {
        let home = tempfile::tempdir().expect("temp home");
        let engine = Arc::new(engine::Engine::new(Some(home.path())));
        let hub = Arc::new(crate::runtime::EventHub::default());
        let (tx, events) = tokio::sync::mpsc::unbounded_channel();
        hub.subscribe(1, tx, None);
        let processes = engine.processes().clone();
        Harness {
            jobs: Jobs::new(
                engine,
                hub,
                Claims::default(),
                processes,
                Cancellations::default(),
            ),
            events,
            _home: home,
        }
    }

    fn start<W>(jobs: &Jobs, work: W) -> String
    where
        W: for<'a> FnOnce(
                &'a engine::Engine,
                Reporter<'a, stand_in::Progress>,
            ) -> Work<'a, stand_in::Done>
            + Send
            + 'static,
    {
        let spec = Spec {
            id: String::new(),
            prefix: "test",
            claim: None,
        };
        jobs.run::<stand_in::Test, _>(spec, work)
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
    async fn progress_is_coalesced_and_the_job_settles_on_done() {
        let mut harness = harness();
        start(&harness.jobs, |_, reporter| {
            Box::pin(async move {
                let report = reporter.checked();
                // A flood of sub-epsilon ticks, as an assets pass emits.
                for done in 0..=1000 {
                    report(&stand_in::Progress { done })?;
                }
                Ok(stand_in::Done {})
            })
        });

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
        start(&harness.jobs, |_, reporter| {
            Box::pin(async move {
                reporter.cancel().cancel();
                reporter.cancel().check()?;
                Ok(stand_in::Done {})
            })
        });

        assert_eq!(topics(&mut harness.events).await, vec!["cancelled"]);
    }

    #[tokio::test]
    async fn a_failed_job_settles_on_its_error_topic() {
        let mut harness = harness();
        start(&harness.jobs, |_, _| {
            Box::pin(async { Err(anyhow::anyhow!("no")) })
        });

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
