//! The receiving side of one connection. A reader task drains the socket into a
//! bounded buffer, so the host learns about arriving bytes and a closing peer
//! without a call being in flight, and says so once per edge.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use proto::pushed::{IpcClosedEvent, IpcReadableEvent};
use proto::Topic;
use serde::Serialize;
use serde_json::Value;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tokio::sync::Notify;
use tokio::time::{timeout_at, Instant};

const CHUNK: usize = 64 * 1024;
const BUFFER_LIMIT: usize = 1024 * 1024;

/// One event for the plugin that owns the connection, ready for its inbox.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pushed {
    pub topic: &'static str,
    pub payload: Value,
}

/// Where a plugin's connections report. Nothing is sent until the daemon takes
/// the receiver with [`open`](Signals::open), so a plugin that subscribes to
/// nothing queues nothing.
#[derive(Clone, Default)]
pub(super) struct Signals(Arc<Mutex<Option<UnboundedSender<Pushed>>>>);

impl Signals {
    pub(super) fn open(&self) -> UnboundedReceiver<Pushed> {
        let (sender, receiver) = unbounded_channel();
        *self.0.lock().unwrap() = Some(sender);
        receiver
    }

    fn emit<T: Topic + Serialize>(&self, event: &T) {
        if let Some(sender) = self.0.lock().unwrap().as_ref() {
            let payload = serde_json::to_value(event).expect("an event is plain data");
            let _ = sender.send(Pushed {
                topic: T::TOPIC,
                payload,
            });
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Ended {
    Peer,
    Failed(String),
}

struct State {
    buffer: VecDeque<u8>,
    armed: bool,
    waiting: usize,
    ended: Option<Ended>,
}

pub(super) struct Inbound {
    connection: String,
    signals: Signals,
    state: Mutex<State>,
    arrived: Notify,
    room: Notify,
}

impl Inbound {
    /// Starts the reader over `reader`; the task ends with the connection or when
    /// aborted.
    pub(super) fn spawn<R>(
        connection: String,
        signals: Signals,
        reader: R,
    ) -> (Arc<Inbound>, tokio::task::JoinHandle<()>)
    where
        R: AsyncRead + Send + Unpin + 'static,
    {
        let inbound = Arc::new(Inbound {
            connection,
            signals,
            state: Mutex::new(State {
                buffer: VecDeque::new(),
                armed: true,
                waiting: 0,
                ended: None,
            }),
            arrived: Notify::new(),
            room: Notify::new(),
        });
        let task = tokio::spawn(inbound.clone().read(reader));
        (inbound, task)
    }

    async fn read<R: AsyncRead + Unpin>(self: Arc<Self>, mut reader: R) {
        let mut chunk = vec![0; CHUNK];
        loop {
            self.wait_for_room().await;
            match reader.read(&mut chunk).await {
                Ok(0) => return self.end(Ended::Peer),
                Ok(read) => self.push(&chunk[..read]),
                Err(error) => return self.end(Ended::Failed(error.to_string())),
            }
        }
    }

    async fn wait_for_room(&self) {
        loop {
            let notified = self.room.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.state.lock().unwrap().buffer.len() < BUFFER_LIMIT {
                return;
            }
            notified.await;
        }
    }

    fn push(&self, bytes: &[u8]) {
        let fire = {
            let mut state = self.state.lock().unwrap();
            state.buffer.extend(bytes);
            let fire = state.armed && state.waiting == 0;
            state.armed &= !fire;
            fire
        };
        self.arrived.notify_waiters();
        if fire {
            self.signals.emit(&IpcReadableEvent {
                connection: self.connection.clone(),
            });
        }
    }

    fn end(&self, reason: Ended) {
        tracing::debug!(connection = %self.connection, ?reason, "local connection ended");
        self.state.lock().unwrap().ended = Some(reason);
        self.arrived.notify_waiters();
        self.signals.emit(&IpcClosedEvent {
            connection: self.connection.clone(),
        });
    }

    /// At most `max` of what has arrived, waiting up to `wait` for the first
    /// byte. Empty when nothing came in time; `Err` once the peer is gone and
    /// everything it sent was taken. Re-arms the readable event.
    pub(super) async fn take(&self, max: usize, wait: Duration) -> Result<Vec<u8>, Ended> {
        let deadline = Instant::now() + wait;
        loop {
            let notified = self.arrived.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            {
                let mut state = self.state.lock().unwrap();
                if !state.buffer.is_empty() {
                    return Ok(self.drain(&mut state, max));
                }
                if let Some(reason) = &state.ended {
                    return Err(reason.clone());
                }
                state.waiting += 1;
            }
            let timed_out = timeout_at(deadline, notified).await.is_err();
            let mut state = self.state.lock().unwrap();
            state.waiting -= 1;
            if !state.buffer.is_empty() {
                return Ok(self.drain(&mut state, max));
            }
            if let Some(reason) = &state.ended {
                return Err(reason.clone());
            }
            if timed_out {
                state.armed = true;
                return Ok(Vec::new());
            }
        }
    }

    fn drain(&self, state: &mut State, max: usize) -> Vec<u8> {
        let taken: Vec<u8> = state.buffer.drain(..max.min(state.buffer.len())).collect();
        let leftover = !state.buffer.is_empty();
        state.armed = !leftover;
        self.room.notify_waiters();
        if leftover {
            self.signals.emit(&IpcReadableEvent {
                connection: self.connection.clone(),
            });
        }
        taken
    }
}
