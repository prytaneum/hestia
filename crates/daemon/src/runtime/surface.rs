//! The daemon's half of `kernel::Services`: where a plugin's files live and
//! the front-ends that draw what it shows. The message catalogue comes with
//! the plugin install, which has the manifest.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock, PoisonError, Weak};

use kernel::{Answer, GuestError, NoticeLevel as Level, Services};
use proto::error::ErrorInfo;
use proto::plugin::{
    NoticeLevel, PluginLink, PluginNotice, PluginQuestion, PluginQuestionClosed, PluginRunProgress,
};
use tokio::sync::{oneshot, watch};

use super::{EventHub, Runtime};

struct Waiting {
    plugin: String,
    answer: oneshot::Sender<bool>,
}

pub struct Surface {
    hub: Arc<EventHub>,
    data_root: PathBuf,
    state_root: PathBuf,
    questions: Mutex<HashMap<String, Waiting>>,
    progress: Mutex<HashMap<String, watch::Sender<PluginRunProgress>>>,
    runtime: OnceLock<Weak<Runtime>>,
}

/// The progress of one running command, latest report only: a plugin reporting
/// faster than the job publishes leaves nothing queued. Dropping it stops the
/// routing.
pub struct ProgressWatch {
    surface: Arc<Surface>,
    job: String,
    reports: watch::Receiver<PluginRunProgress>,
}

impl ProgressWatch {
    /// The newest report, once there is one the job has not seen.
    pub async fn changed(&mut self) -> Option<PluginRunProgress> {
        self.reports.changed().await.ok()?;
        Some(self.reports.borrow_and_update().clone())
    }

    /// A report that arrived and has not been seen.
    pub fn unseen(&mut self) -> Option<PluginRunProgress> {
        match self.reports.has_changed() {
            Ok(true) => Some(self.reports.borrow_and_update().clone()),
            _ => None,
        }
    }
}

impl Drop for ProgressWatch {
    fn drop(&mut self) {
        self.surface.routes().remove(&self.job);
    }
}

/// Withdraws a question however its wait ends, so no front-end keeps a dialog
/// for a call that is gone.
struct Asked<'a> {
    surface: &'a Surface,
    request: String,
    run: Option<String>,
}

impl Drop for Asked<'_> {
    fn drop(&mut self) {
        self.surface.questions().remove(&self.request);
        self.surface.hub.publish_topic(&PluginQuestionClosed {
            request: self.request.clone(),
            id: self.run.take(),
        });
    }
}

impl Surface {
    /// `<data_home>/sandbox/<id>` is the plugin's `/data`;
    /// `<data_home>/plugins/<id>` holds what only Hestia reads, apart from it,
    /// so no link inside `/data` can name it by a relative path.
    pub fn new(hub: Arc<EventHub>, data_home: PathBuf) -> Self {
        Surface {
            hub,
            data_root: kernel::sandbox_root(&data_home),
            state_root: data_home.join("plugins"),
            questions: Mutex::default(),
            progress: Mutex::default(),
            runtime: OnceLock::new(),
        }
    }

    /// Lets plugins start their own jobs through `runtime`, which owns the
    /// surface and so cannot be handed to it at construction.
    pub fn bind(&self, runtime: &Arc<Runtime>) {
        let _ = self.runtime.set(Arc::downgrade(runtime));
    }

    /// Routes what the plugin reports in job `job` to the returned watch.
    pub fn watch_progress(self: &Arc<Self>, job: &str) -> ProgressWatch {
        let (sender, reports) = watch::channel(PluginRunProgress::default());
        self.routes().insert(job.to_owned(), sender);
        ProgressWatch {
            surface: self.clone(),
            job: job.to_owned(),
            reports,
        }
    }

    fn routes(
        &self,
    ) -> std::sync::MutexGuard<'_, HashMap<String, watch::Sender<PluginRunProgress>>> {
        self.progress.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Delivers the user's answer to `request` from `plugin`; false when nobody
    /// is waiting on that question from that plugin.
    pub fn answer(&self, request: &str, plugin: &str, accepted: bool) -> bool {
        let mut questions = self.questions();
        if questions
            .get(request)
            .is_none_or(|waiting| waiting.plugin != plugin)
        {
            return false;
        }
        let waiting = questions.remove(request);
        waiting.is_some_and(|waiting| waiting.answer.send(accepted).is_ok())
    }

    fn questions(&self) -> std::sync::MutexGuard<'_, HashMap<String, Waiting>> {
        self.questions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn dir(root: &std::path::Path, plugin: &str) -> Option<PathBuf> {
        kernel::id::is_valid(plugin).then(|| root.join(plugin))
    }

    fn watched(&self, plugin: &str, job: Option<&str>, what: &str) -> bool {
        let watched = self.hub.has_plugin_surface(job);
        if !watched {
            tracing::info!(plugin, what, "no front-end is connected to draw it");
        }
        watched
    }
}

impl Services for Surface {
    fn data_dir(&self, plugin: &str) -> Option<PathBuf> {
        Self::dir(&self.data_root, plugin)
    }

    fn state_dir(&self, plugin: &str) -> Option<PathBuf> {
        Self::dir(&self.state_root, plugin)
    }

    fn notify(&self, plugin: &str, job: Option<&str>, level: Level, message: &str) {
        if !self.watched(plugin, job, "notice") {
            return;
        }
        let level = match level {
            Level::Info => NoticeLevel::Info,
            Level::Warn => NoticeLevel::Warn,
            Level::Error => NoticeLevel::Error,
        };
        self.hub.publish_topic(&PluginNotice {
            plugin: plugin.to_owned(),
            level,
            message: message.to_owned(),
            id: job.map(str::to_owned),
        });
    }

    fn confirm<'a>(
        &'a self,
        plugin: &'a str,
        job: Option<&'a str>,
        message: &'a str,
    ) -> Answer<'a> {
        Box::pin(async move {
            if !self.watched(plugin, job, "question") {
                return false;
            }
            let request = uuid::Uuid::new_v4().to_string();
            let (send, receive) = oneshot::channel();
            self.questions().insert(
                request.clone(),
                Waiting {
                    plugin: plugin.to_owned(),
                    answer: send,
                },
            );
            let run = job.map(str::to_owned);
            let _asked = Asked {
                surface: self,
                request: request.clone(),
                run: run.clone(),
            };
            self.hub.publish_topic(&PluginQuestion {
                request,
                plugin: plugin.to_owned(),
                message: message.to_owned(),
                id: run,
            });
            matches!(receive.await, Ok(true))
        })
    }

    fn open_url(&self, plugin: &str, job: Option<&str>, url: &str) -> bool {
        if !self.watched(plugin, job, "link") {
            return false;
        }
        self.hub.publish_topic(&PluginLink {
            plugin: plugin.to_owned(),
            url: url.to_owned(),
            id: job.map(str::to_owned),
        });
        true
    }

    fn progress(&self, _plugin: &str, job: &str, current: u64, total: u64, message: &str) {
        if let Some(reports) = self.routes().get(job) {
            reports.send_replace(PluginRunProgress {
                current,
                total,
                message: message.to_owned(),
            });
        }
    }

    fn start_job(&self, plugin: &str, command: &str, args: &str) -> Result<String, GuestError> {
        let runtime = self
            .runtime
            .get()
            .and_then(Weak::upgrade)
            .ok_or(GuestError::Unsupported)?;
        runtime
            .start_plugin_command(plugin, command, args, false, String::new())
            .map_err(|error| match error {
                ErrorInfo::PluginInvalid { detail } => GuestError::Invalid(detail),
                ErrorInfo::PluginNotFound { plugin } => GuestError::NotFound(plugin),
                other => GuestError::Failed(other.to_string()),
            })
    }
}

#[cfg(test)]
mod tests {
    use tokio::sync::mpsc::unbounded_channel;

    use super::*;

    #[tokio::test]
    async fn a_question_reaches_a_drawing_front_end_and_takes_only_its_answer() {
        let hub = Arc::new(EventHub::default());
        let surface = Arc::new(Surface::new(hub.clone(), PathBuf::from("home")));
        let (metrics, _ignored) = unbounded_channel();
        hub.subscribe(1, metrics, None, false);
        assert!(
            !surface.confirm("tidy", None, "go?").await,
            "a front-end that draws nothing is nobody to ask"
        );
        assert!(!surface.open_url("tidy", None, "https://example.com/"));

        let (following, _ignored) = unbounded_channel();
        hub.subscribe(3, following, Some("run-a".into()), true);
        assert!(
            !surface.confirm("tidy", Some("run-b"), "go?").await,
            "a front-end following another run is nobody to ask"
        );
        assert!(!surface.confirm("tidy", None, "go?").await);
        hub.unsubscribe(3);

        let (out, mut frames) = unbounded_channel();
        hub.subscribe(2, out, None, true);
        let asking = tokio::spawn({
            let surface = surface.clone();
            async move { surface.confirm("tidy", Some("run-b"), "go?").await }
        });
        let frame = frames.recv().await.unwrap();
        let question: serde_json::Value = serde_json::from_str(&frame).unwrap();
        let request = question["payload"]["request"].as_str().unwrap().to_owned();
        assert!(frame.contains("tidy"), "{frame}");

        assert!(
            !surface.answer(&request, "other", true),
            "another plugin's answer"
        );
        assert!(surface.answer(&request, "tidy", true));
        assert!(asking.await.unwrap());
        assert!(!surface.answer(&request, "tidy", true), "answered once");
        let closed = frames.recv().await.unwrap();
        assert!(
            closed.contains("plugin.question.closed") && closed.contains(&request),
            "{closed}"
        );
    }

    #[tokio::test]
    async fn a_flood_of_progress_reports_leaves_only_the_latest() {
        let surface = Arc::new(Surface::new(Arc::default(), PathBuf::from("home")));
        let mut reports = surface.watch_progress("run-a");
        for step in 0..100_000 {
            surface.progress("tidy", "run-a", step, 100_000, "working");
            surface.progress("tidy", "run-b", step, 100_000, "someone else's");
        }
        let latest = reports.changed().await.unwrap();
        assert_eq!(
            (latest.current, latest.message.as_str()),
            (99_999, "working")
        );
        assert!(reports.unseen().is_none());
        drop(reports);
        assert!(surface.routes().is_empty(), "a finished run is forgotten");
    }

    #[test]
    fn plugin_files_stay_under_their_roots_and_apart() {
        let surface = Surface::new(Arc::default(), PathBuf::from("home"));
        let data = surface.data_dir("tidy").unwrap();
        let state = surface.state_dir("tidy").unwrap();
        assert!(!state.starts_with(&data) && !data.starts_with(&state));
        for id in ["../escape", "a/b", "CON", "con"] {
            assert!(surface.data_dir(id).is_none(), "{id}");
        }
    }
}
