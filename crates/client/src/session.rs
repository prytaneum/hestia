//! The client SDK's connection core, shared by every domain facade: one
//! persistent, multiplexed connection whose reader task fulfils pending requests
//! by id and delivers events to the installed callback. The typed `call::<C>()`
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
use tokio::sync::{oneshot, Notify};

type EventCallback = Arc<dyn Fn(&Event) + Send + Sync>;

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
            Terminal::Failed(info) => IpcError::Daemon {
                code: info.code().to_string(),
                message: info.to_string(),
                info: serde_json::to_value(&info).unwrap_or(Value::Null),
            },
        }
    }
}

struct Shared {
    pending: Mutex<HashMap<i64, oneshot::Sender<Response>>>,
    event_cb: Mutex<Option<EventCallback>>,
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
            event_cb: Mutex::new(None),
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
            Err(IpcError::Daemon { code, .. }) if code == ipc::errors::NOT_FOUND => {
                return Ok(None)
            }
            other => other?,
        };
        serde_json::from_value(res.payload)
            .map(Some)
            .map_err(|e| IpcError::Malformed(e.to_string()))
    }

    pub fn set_event_callback(&self, cb: Option<EventCallback>) {
        *self.shared.event_cb.lock().unwrap() = cb;
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
        on_event: impl Fn(&Event) -> Option<Result<T, Terminal>> + Send + Sync + 'static,
    ) -> Result<T, IpcError>
    where
        T: Send + 'static,
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<(), IpcError>>,
    {
        struct Outcome<T> {
            state: Mutex<Option<Result<T, Terminal>>>,
            notify: Notify,
        }
        let outcome = Arc::new(Outcome {
            state: Mutex::new(None),
            notify: Notify::new(),
        });

        let id_owned = id.to_string();
        let cb_outcome = outcome.clone();
        self.set_event_callback(Some(Arc::new(move |event: &Event| {
            if event.payload.get("id").and_then(Value::as_str) != Some(id_owned.as_str()) {
                return;
            }
            if let Some(settled) = on_event(event) {
                *cb_outcome.state.lock().unwrap() = Some(settled);
                cb_outcome.notify.notify_waiters();
            }
        })));

        let result = self
            .await_settled(id, start, &outcome.state, &outcome.notify)
            .await;
        self.set_event_callback(None);
        result
    }

    async fn await_settled<T, F, Fut>(
        &self,
        id: &str,
        start: F,
        state: &Mutex<Option<Result<T, Terminal>>>,
        notify: &Notify,
    ) -> Result<T, IpcError>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<(), IpcError>>,
    {
        use proto::events::{EventsSubscribe, EventsSubscribeParams};
        self.call::<EventsSubscribe>(&EventsSubscribeParams { id: id.to_string() })
            .await?;
        start().await?;

        loop {
            if let Some(result) = state.lock().unwrap().take() {
                return result.map_err(Terminal::into_error);
            }
            if self.is_closed() {
                return Err(self.shared.close_error());
            }
            let _ = tokio::time::timeout(Duration::from_millis(500), notify.notified()).await;
        }
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
            let cb = self.event_cb.lock().unwrap().clone();
            if let Some(cb) = cb {
                cb(&event);
            }
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
    Err(IpcError::Daemon {
        code: info.code().into(),
        message: info.to_string(),
        info: raw,
    })
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
