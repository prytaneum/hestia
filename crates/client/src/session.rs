//! The client SDK's connection core, shared by every domain facade: one
//! persistent, multiplexed connection whose reader task fulfils pending requests
//! by id and routes events to the registered listeners. The typed `call::<C>()`
//! marshals through the contract, so facades stay one-liners and cannot drift.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ipc::errors::IpcError;
use ipc::protocol::{self, DecodeError, Event, Request, Response};
use ipc::{Connection, FrameWriter};
use proto::error::ErrorInfo;
use proto::job::{Job, JobEvent, JobState};
use proto::Contract;
use serde_json::Value;
use tokio::sync::{mpsc, oneshot};

/// How a job ended, other than well. Cancellation is kept apart from failure
/// because nothing went wrong: a front-end must be able to say "cancelled"
/// rather than render it as an error.
pub(crate) enum Terminal {
    Cancelled,
    Failed(ErrorInfo),
    Malformed(String),
}

impl Terminal {
    fn into_error(self) -> IpcError {
        match self {
            Terminal::Cancelled => IpcError::Cancelled,
            Terminal::Malformed(detail) => IpcError::Malformed(detail),
            Terminal::Failed(info) => {
                let raw = serde_json::to_value(&info).unwrap_or(Value::Null);
                daemon_error(&info, raw)
            }
        }
    }
}

struct Shared {
    pending: Mutex<HashMap<i64, oneshot::Sender<Response>>>,
    listeners: Mutex<Vec<(Option<String>, mpsc::UnboundedSender<Event>)>>,
    closed: AtomicBool,
    /// Set when the connection was torn down for a protocol-version mismatch, so
    /// a woken waiter reports why rather than a bare `ConnectionLost`.
    mismatch: Mutex<Option<(i64, i64)>>,
}

pub struct Session {
    shared: Arc<Shared>,
    writer: tokio::sync::Mutex<FrameWriter>,
    next_id: AtomicI64,
    reader: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl Session {
    pub fn new(connection: Connection) -> Self {
        let (mut reader, writer) = connection.into_split();
        let shared = Arc::new(Shared {
            pending: Mutex::new(HashMap::new()),
            listeners: Mutex::new(Vec::new()),
            closed: AtomicBool::new(false),
            mismatch: Mutex::new(None),
        });
        let reader_shared = shared.clone();
        tracing::trace!("session opened");
        let handle = tokio::spawn(async move {
            while let Ok(Some(frame)) = reader.recv().await {
                reader_shared.dispatch(&frame);
            }
            reader_shared.close();
        });
        Session {
            shared,
            writer: tokio::sync::Mutex::new(writer),
            next_id: AtomicI64::new(1),
            reader: Mutex::new(Some(handle)),
        }
    }

    pub fn is_closed(&self) -> bool {
        self.shared.closed.load(Ordering::SeqCst)
    }

    /// Resolves once the connection has closed — a stopping daemon has let go of
    /// its endpoint by then.
    pub async fn closed(&self) {
        let mut listener = self.listen(None);
        while listener.recv().await.is_some() {}
    }

    /// Raw request; errors only on transport failure (a daemon-side error is a
    /// `Response` with `ok == false`).
    pub async fn call_raw(
        &self,
        channel: &str,
        payload: Value,
        timeout: Duration,
    ) -> Result<Response, IpcError> {
        if self.is_closed() {
            return Err(self.shared.close_error());
        }
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.shared.pending.lock().unwrap().insert(id, tx);

        let req = Request::new(channel, payload, Some(id));
        tracing::debug!(channel, id, "call");
        let frame = protocol::encode_request(&req);
        // The wire is what a client can show that the daemon's own logs cannot:
        // which frames this process sent, correlated, sized and timed. Payloads
        // are never logged — they carry access tokens and rcon passwords — so
        // only the byte count goes out.
        tracing::trace!(
            direction = "send",
            channel,
            id,
            bytes = frame.len(),
            "frame"
        );
        let started = std::time::Instant::now();
        if let Err(e) = self.writer.lock().await.send(&frame).await {
            self.shared.pending.lock().unwrap().remove(&id);
            tracing::trace!(channel, id, error = %e, "send failed");
            return Err(e);
        }

        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(res)) => {
                tracing::debug!(channel, id, ok = res.ok, "call complete");
                tracing::trace!(
                    direction = "recv",
                    channel,
                    id,
                    ok = res.ok,
                    elapsed_ms = started.elapsed().as_millis(),
                    "frame"
                );
                Ok(res)
            }
            Ok(Err(_)) => Err(self.shared.close_error()),
            Err(_) => {
                self.shared.pending.lock().unwrap().remove(&id);
                tracing::trace!(
                    channel,
                    id,
                    elapsed_ms = started.elapsed().as_millis(),
                    "timed out waiting for a reply"
                );
                Err(IpcError::Timeout(channel.to_string()))
            }
        }
    }

    /// Send `C::Params` over `C::CHANNEL` and decode `C::Result`, mapping a
    /// daemon-side error to `IpcError::Daemon`.
    pub async fn call<C: Contract>(&self, params: &C::Params) -> Result<C::Result, IpcError> {
        let payload =
            serde_json::to_value(params).map_err(|e| IpcError::Malformed(e.to_string()))?;
        let res = self.call_raw(C::CHANNEL, payload, C::TIMEOUT).await?;
        let res = must(res)?;
        serde_json::from_value(res.payload).map_err(|e| IpcError::Malformed(e.to_string()))
    }

    /// Like `call`, but a `not_found` error becomes `Ok(None)` instead of an error.
    pub async fn try_call<C: Contract>(
        &self,
        params: &C::Params,
    ) -> Result<Option<C::Result>, IpcError> {
        let payload =
            serde_json::to_value(params).map_err(|e| IpcError::Malformed(e.to_string()))?;
        let res = self.call_raw(C::CHANNEL, payload, C::TIMEOUT).await?;
        let res = match must(res) {
            Err(IpcError::Daemon { code, .. }) if code == proto::error::code::NOT_FOUND => {
                return Ok(None)
            }
            other => other?,
        };
        serde_json::from_value(res.payload)
            .map(Some)
            .map_err(|e| IpcError::Malformed(e.to_string()))
    }

    /// Receive the daemon's events. `Some(scope)` hears only events whose
    /// payload id is in that process scope; `None` hears every event. The
    /// registration ends when the [`Listener`] drops, and the stream ends when
    /// the connection closes. Subscribe on the daemon with `events.subscribe`
    /// for the events to be sent at all.
    pub fn listen(&self, scope: Option<&str>) -> Listener {
        let (tx, rx) = mpsc::unbounded_channel();
        {
            let mut listeners = self.shared.listeners.lock().unwrap();
            if !self.is_closed() {
                listeners.push((scope.map(str::to_string), tx));
            }
        }
        Listener {
            rx,
            shared: self.shared.clone(),
        }
    }

    /// [`listen`](Self::listen), then subscribe on the daemon. The listener is
    /// registered first so no event sent after the subscription is missed.
    pub async fn listen_subscribed(&self, scope: Option<&str>) -> Result<Listener, IpcError> {
        use proto::events::{EventsSubscribe, EventsSubscribeParams};

        let listener = self.listen(scope);
        self.call::<EventsSubscribe>(&EventsSubscribeParams {
            id: scope.unwrap_or_default().to_string(),
        })
        .await?;
        Ok(listener)
    }

    /// Start the job `C` under `id` and block until it settles, handing each
    /// progress payload to `on_progress`. `params` must carry `id` as its job id;
    /// the subscription is taken before the start call so an instant job cannot
    /// slip its terminal event past us. A cancelled job is
    /// [`IpcError::Cancelled`], not a failure.
    pub async fn run_job<C: Job>(
        &self,
        id: &str,
        params: &C::Params,
        on_progress: impl Fn(&C::Progress) + Send + Sync + 'static,
    ) -> Result<C::Done, IpcError> {
        self.await_events(
            id,
            || async { self.call::<C>(params).await.map(|_| ()) },
            move |event: &Event| {
                if event.topic != proto::job::TOPIC {
                    return None;
                }
                let decoded =
                    serde_json::from_value::<JobEvent<C::Progress, C::Done>>(event.payload.clone());
                match decoded.map(|job| job.state) {
                    Ok(JobState::Progress(progress)) => {
                        on_progress(&progress);
                        None
                    }
                    Ok(JobState::Done(done)) => Some(Ok(done)),
                    Ok(JobState::Failed { error }) => Some(Err(Terminal::Failed(error))),
                    Ok(JobState::Cancelled) => Some(Err(Terminal::Cancelled)),
                    Err(e) => Some(Err(Terminal::Malformed(e.to_string()))),
                }
            },
        )
        .await
    }

    /// Subscribe to `id`'s events, run `start`, and block until `on_event`
    /// settles. `on_event` sees only events whose payload carries `id`; it
    /// returns `None` to keep waiting.
    pub(crate) async fn await_events<T, F, Fut>(
        &self,
        id: &str,
        start: F,
        mut on_event: impl FnMut(&Event) -> Option<Result<T, Terminal>>,
    ) -> Result<T, IpcError>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<(), IpcError>>,
    {
        let mut listener = self.listen_subscribed(Some(id)).await?;
        start().await?;

        while let Some(event) = listener.recv().await {
            if let Some(settled) = on_event(&event) {
                return settled.map_err(Terminal::into_error);
            }
        }
        Err(self.shared.close_error())
    }
}

/// A registration on a [`Session`]'s event stream, removed on drop.
pub struct Listener {
    rx: mpsc::UnboundedReceiver<Event>,
    shared: Arc<Shared>,
}

impl Listener {
    /// The next event, or `None` once the connection has closed.
    pub async fn recv(&mut self) -> Option<Event> {
        self.rx.recv().await
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.rx.close();
        self.shared
            .listeners
            .lock()
            .unwrap()
            .retain(|(_, tx)| !tx.is_closed());
    }
}

impl Shared {
    fn dispatch(&self, frame: &str) {
        let Ok(value) = serde_json::from_str::<Value>(frame) else {
            tracing::trace!(bytes = frame.len(), "ignoring a malformed frame");
            return; // ignore a malformed frame rather than tear down
        };
        if protocol::is_event(&value) {
            let event = protocol::decode_event(&value);
            // Unsolicited, so it correlates with no call: the topic is what a
            // reader needs to see a job's progress arriving (or not).
            tracing::trace!(
                direction = "recv",
                topic = %event.topic,
                bytes = frame.len(),
                "event"
            );
            let id = event.payload.get("id").and_then(Value::as_str);
            self.listeners.lock().unwrap().retain(|(scope, tx)| {
                let wanted = match scope {
                    None => true,
                    Some(scope) => id.is_some_and(|id| proto::naming::process_in_scope(scope, id)),
                };
                !wanted || tx.send(event.clone()).is_ok()
            });
            return;
        }
        match protocol::decode_response(&value) {
            Ok(res) => {
                let id = res.id.unwrap_or(0);
                if let Some(tx) = self.pending.lock().unwrap().remove(&id) {
                    let _ = tx.send(res);
                }
            }
            // A foreign-major daemon is refused, not silently consumed: tear the
            // connection down so every waiter fails fast rather than timing out,
            // recording the mismatch so they report why.
            Err(DecodeError::IncompatibleVersion { got, want }) => {
                tracing::warn!(
                    got,
                    want,
                    "closing connection: incompatible daemon protocol version"
                );
                *self.mismatch.lock().unwrap() = Some((got, want));
                self.close();
            }
            // A junk frame is ignored, as before — one bad frame is not a reason
            // to drop an otherwise-compatible connection.
            Err(DecodeError::Malformed(_)) => {}
        }
    }

    fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        // Wake every waiter so they fail instead of blocking forever.
        let waiting = self.pending.lock().unwrap().drain().count();
        self.listeners.lock().unwrap().clear();
        tracing::trace!(waiting, "connection closed");
    }

    /// The error a waiter woken by a torn-down connection should report: a
    /// version mismatch names itself, everything else is a bare disconnect.
    fn close_error(&self) -> IpcError {
        match *self.mismatch.lock().unwrap() {
            Some((got, want)) => IpcError::IncompatibleVersion { got, want },
            None => IpcError::ConnectionLost,
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if let Some(handle) = self.reader.lock().unwrap().take() {
            handle.abort();
        }
    }
}

/// Turn a daemon-side error `Response` into an `IpcError`; otherwise hand it back.
pub fn must(res: Response) -> Result<Response, IpcError> {
    if res.ok {
        return Ok(res);
    }
    let raw = res.error.unwrap_or(Value::Null);
    let info =
        serde_json::from_value::<ErrorInfo>(raw.clone()).unwrap_or_else(|_| ErrorInfo::Internal {
            detail: "daemon error".into(),
        });
    Err(daemon_error(&info, raw))
}

fn daemon_error(info: &ErrorInfo, raw: Value) -> IpcError {
    IpcError::Daemon {
        code: info.code().into(),
        message: info.to_string(),
        info: raw,
    }
}

/// A client-generated job id lets callers subscribe before starting a job, so
/// even one that finishes instantly cannot slip its terminal event past us.
pub fn job_id(prefix: &str) -> String {
    use std::sync::atomic::AtomicU64;
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed) + 1;
    let pid = std::process::id();
    format!("{prefix}-{pid}-{n}")
}
