use ipc::errors::IpcError;
use ipc::protocol::Event;
use proto::process::{
    ProcessExitEvent, ProcessInfo, ProcessList, ProcessLogLine, ProcessLogs, ProcessLogsParams,
    ProcessMetrics, ProcessRef, ProcessSpec, ProcessStart, ProcessStartResult, ProcessStartedEvent,
    ProcessStatus, ProcessStop,
};
use proto::Topic;

use crate::session::{job_id, Session, Terminal};

/// One event from a subscribed process's live stream.
pub enum ProcessEvent {
    Started(ProcessStartedEvent),
    Output(ProcessLogLine),
    Exit(ProcessExitEvent),
}

pub struct Process<'a> {
    pub(crate) session: &'a Session,
}

impl Process<'_> {
    pub async fn start(&self, spec: ProcessSpec) -> Result<ProcessStartResult, IpcError> {
        self.session.call::<ProcessStart>(&spec).await
    }

    pub async fn stop(&self, id: &str) -> Result<(), IpcError> {
        self.session
            .call::<ProcessStop>(&ProcessRef { id: id.to_string() })
            .await?;
        Ok(())
    }

    pub async fn list(&self) -> Result<Vec<ProcessInfo>, IpcError> {
        Ok(self
            .session
            .call::<ProcessList>(&proto::Empty {})
            .await?
            .processes)
    }

    pub async fn status(&self, id: &str) -> Result<ProcessInfo, IpcError> {
        self.session
            .call::<ProcessStatus>(&ProcessRef { id: id.to_string() })
            .await
    }

    pub async fn logs(
        &self,
        id: &str,
        tail: Option<usize>,
    ) -> Result<Vec<ProcessLogLine>, IpcError> {
        let params = ProcessLogsParams {
            id: id.to_string(),
            tail,
        };
        Ok(self.session.call::<ProcessLogs>(&params).await?.lines)
    }

    /// Stream a process's start, output, and exit as they happen. `id` is a
    /// process key, or an *entry* key (`server-<id>`, `instance-<id>`) to follow
    /// the entry across stops and starts — its processes come and go on the one
    /// stream. Installs the session's (single) event callback, so it composes
    /// with `call`s on the same client but not with `run_job` or another
    /// subscription; the stream ends when the connection closes.
    pub async fn subscribe(
        &self,
        id: &str,
    ) -> Result<tokio::sync::mpsc::UnboundedReceiver<ProcessEvent>, IpcError> {
        use proto::events::{EventsSubscribe, EventsSubscribeParams};
        use proto::process::ProcessOutputEvent;
        use proto::Topic;

        self.session
            .call::<EventsSubscribe>(&EventsSubscribeParams { id: id.to_string() })
            .await?;
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        self.session.set_event_callback(Some(std::sync::Arc::new(
            move |event: &Event| match event.topic.as_str() {
                ProcessStartedEvent::TOPIC => {
                    if let Ok(e) =
                        serde_json::from_value::<ProcessStartedEvent>(event.payload.clone())
                    {
                        let _ = tx.send(ProcessEvent::Started(e));
                    }
                }
                ProcessOutputEvent::TOPIC => {
                    if let Ok(e) =
                        serde_json::from_value::<ProcessOutputEvent>(event.payload.clone())
                    {
                        for line in e.lines {
                            let _ = tx.send(ProcessEvent::Output(line));
                        }
                    }
                }
                ProcessExitEvent::TOPIC => {
                    if let Ok(e) = serde_json::from_value::<ProcessExitEvent>(event.payload.clone())
                    {
                        let _ = tx.send(ProcessEvent::Exit(e));
                    }
                }
                _ => {}
            },
        )));
        Ok(rx)
    }

    /// Stream the daemon's periodic resource samples for every running process.
    /// Claims the session's event callback like [`subscribe`], so it does not
    /// compose with another subscription on the same client.
    pub async fn subscribe_metrics(
        &self,
    ) -> Result<tokio::sync::mpsc::UnboundedReceiver<Vec<ProcessMetrics>>, IpcError> {
        use proto::events::{EventsSubscribe, EventsSubscribeParams};
        use proto::process::ProcessMetricsEvent;
        use proto::Topic;

        self.session
            .call::<EventsSubscribe>(&EventsSubscribeParams { id: String::new() })
            .await?;
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        self.session
            .set_event_callback(Some(std::sync::Arc::new(move |event: &Event| {
                if event.topic == ProcessMetricsEvent::TOPIC {
                    if let Ok(e) =
                        serde_json::from_value::<ProcessMetricsEvent>(event.payload.clone())
                    {
                        let _ = tx.send(e.samples);
                    }
                }
            })));
        Ok(rx)
    }

    /// Launch a process and block until it exits, forwarding each output line to
    /// `on_output`. Returns the terminal exit event (state + code). The spec's id
    /// is filled in when empty so events can be matched before the process starts.
    pub async fn run(
        &self,
        mut spec: ProcessSpec,
        on_output: impl Fn(&ProcessLogLine) + Send + Sync + 'static,
    ) -> Result<ProcessExitEvent, IpcError> {
        if spec.id.is_empty() {
            spec.id = job_id("process");
        }
        let id = spec.id.clone();

        let on_event = move |event: &Event| {
            if event.topic == ProcessExitEvent::TOPIC {
                return Some(
                    serde_json::from_value::<ProcessExitEvent>(event.payload.clone())
                        .map_err(|e| Terminal::Malformed(e.to_string())),
                );
            }
            if let Ok(out) =
                serde_json::from_value::<proto::process::ProcessOutputEvent>(event.payload.clone())
            {
                for line in &out.lines {
                    on_output(line);
                }
            }
            None
        };

        let session = self.session;
        let start_spec = spec.clone();
        self.session
            .await_events(
                &id,
                move || async move { session.call::<ProcessStart>(&start_spec).await.map(|_| ()) },
                on_event,
            )
            .await
    }
}
