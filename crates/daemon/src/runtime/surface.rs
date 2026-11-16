//! The daemon's half of `kernel::Services`: where a plugin's files live and
//! the front-ends that draw what it shows. The message catalogue comes with
//! the plugin install, which has the manifest.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use kernel::{Answer, NoticeLevel as Level, Services};
use proto::plugin::{NoticeLevel, PluginLink, PluginNotice, PluginQuestion, PluginQuestionClosed};
use tokio::sync::oneshot;

use super::EventHub;

struct Waiting {
    plugin: String,
    answer: oneshot::Sender<bool>,
}

pub struct Surface {
    hub: Arc<EventHub>,
    data_root: PathBuf,
    state_root: PathBuf,
    questions: Mutex<HashMap<String, Waiting>>,
}

/// Withdraws a question however its wait ends, so no front-end keeps a dialog
/// for a call that is gone.
struct Asked<'a> {
    surface: &'a Surface,
    request: String,
}

impl Drop for Asked<'_> {
    fn drop(&mut self) {
        self.surface.questions().remove(&self.request);
        self.surface.hub.publish_topic(&PluginQuestionClosed {
            request: self.request.clone(),
        });
    }
}

impl Surface {
    /// `<data_home>/plugin-data/<id>` is the plugin's `/data`;
    /// `<data_home>/plugins/<id>` holds what only Hestia reads, apart from it,
    /// so no link inside `/data` can name it by a relative path.
    pub fn new(hub: Arc<EventHub>, data_home: PathBuf) -> Self {
        Surface {
            hub,
            data_root: data_home.join("plugin-data"),
            state_root: data_home.join("plugins"),
            questions: Mutex::default(),
        }
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

    fn watched(&self, plugin: &str, what: &str) -> bool {
        let watched = self.hub.has_plugin_surface();
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

    fn notify(&self, plugin: &str, level: Level, message: &str) {
        if !self.watched(plugin, "notice") {
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
        });
    }

    fn confirm<'a>(&'a self, plugin: &'a str, message: &'a str) -> Answer<'a> {
        Box::pin(async move {
            if !self.watched(plugin, "question") {
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
            let _asked = Asked {
                surface: self,
                request: request.clone(),
            };
            self.hub.publish_topic(&PluginQuestion {
                request,
                plugin: plugin.to_owned(),
                message: message.to_owned(),
            });
            matches!(receive.await, Ok(true))
        })
    }

    fn open_url(&self, plugin: &str, url: &str) -> bool {
        if !self.watched(plugin, "link") {
            return false;
        }
        self.hub.publish_topic(&PluginLink {
            plugin: plugin.to_owned(),
            url: url.to_owned(),
        });
        true
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
            !surface.confirm("tidy", "go?").await,
            "a front-end that draws nothing is nobody to ask"
        );
        assert!(!surface.open_url("tidy", "https://example.com/"));

        let (out, mut frames) = unbounded_channel();
        hub.subscribe(2, out, None, true);
        let asking = tokio::spawn({
            let surface = surface.clone();
            async move { surface.confirm("tidy", "go?").await }
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
