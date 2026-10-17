//! The daemon bridge: one shared client connection held as Tauri state, a
//! generic `ipc_call` command that forwards the frontend's typed calls over
//! the socket, and daemon-event forwarding into the webview.
//!
//! The shell deliberately does not mirror the client facades as one Tauri
//! command per channel: the daemon already validates every payload through
//! the wire contract (`bad_request` / `unknown_channel`), so a per-channel
//! Rust layer would only add a third naming seam that can drift. The typed
//! surface is the frontend's `src/api/`, checked against a table generated
//! from the contracts.

use std::sync::Arc;
use std::time::{Duration, Instant};

use client::proto::contract::DEFAULT_TIMEOUT;
use client::proto::events::{EventsSubscribe, EventsSubscribeParams};
use client::{Client, IpcError};
use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::Mutex;

/// The webview event carrying every daemon push: `{ topic, payload }`.
pub const EVENT_CHANNEL: &str = "hestia:event";
/// The webview event carrying connection-state transitions.
pub const CONNECTION_CHANNEL: &str = "hestia:connection";

const WATCH_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Default)]
pub struct Bridge {
    state: Mutex<Connection>,
}

#[derive(Default)]
struct Connection {
    client: Option<Arc<Client>>,
    failed_at: Option<Instant>,
    announced: Option<ConnectionState>,
}

impl Connection {
    /// A down daemon costs one socket attempt per `WATCH_INTERVAL`, not one
    /// per call: the webview keeps issuing reads while it is offline.
    fn backing_off(&self) -> bool {
        self.failed_at
            .is_some_and(|at| at.elapsed() < WATCH_INTERVAL)
    }
}

#[derive(Serialize, Clone)]
struct EventPayload {
    topic: String,
    payload: Value,
}

#[derive(Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum ConnectionState {
    Connected,
    Disconnected,
}

/// The uniform rejection shape of `ipc_call`: daemon error codes pass through
/// (`not_found`, `bad_request`, …); transport failures get stable codes of
/// their own (`timeout`, `connection_lost`, `transport`).
#[derive(Serialize, Debug)]
pub struct CallError {
    code: String,
    message: String,
    /// The serialized `proto::error::ErrorInfo` the webview localizes from;
    /// `null` for transport failures with no daemon error.
    info: Value,
}

impl CallError {
    /// A shell-side failure with no daemon error code of its own.
    pub(crate) fn other(message: impl Into<String>) -> Self {
        CallError {
            code: "error".into(),
            message: message.into(),
            info: Value::Null,
        }
    }

    /// Answered without touching the socket while the daemon is known down.
    fn offline() -> Self {
        CallError {
            code: "connection_lost".into(),
            message: "the daemon is not running".into(),
            info: Value::Null,
        }
    }
}

impl From<IpcError> for CallError {
    fn from(error: IpcError) -> Self {
        let (code, info) = match &error {
            IpcError::Daemon { code, info, .. } => (code.clone(), info.clone()),
            IpcError::Timeout(_) => ("timeout".into(), Value::Null),
            IpcError::ConnectionLost => ("connection_lost".into(), Value::Null),
            IpcError::IncompatibleVersion { .. } => ("version_mismatch".into(), Value::Null),
            _ => ("transport".into(), Value::Null),
        };
        CallError {
            code,
            message: error.to_string(),
            info,
        }
    }
}

#[tauri::command]
pub async fn ipc_call(
    app: AppHandle,
    bridge: State<'_, Bridge>,
    channel: String,
    payload: Value,
    timeout_ms: Option<u64>,
) -> Result<Value, CallError> {
    let client = acquire(&app, &bridge).await?;
    let timeout = timeout_ms.map_or(DEFAULT_TIMEOUT, Duration::from_millis);
    match client.session().call_raw(&channel, payload, timeout).await {
        Ok(response) if response.ok => Ok(response.payload),
        Ok(response) => {
            let raw = response.error.unwrap_or(Value::Null);
            let info = serde_json::from_value::<client::proto::error::ErrorInfo>(raw.clone())
                .unwrap_or(client::proto::error::ErrorInfo::Internal {
                    detail: "daemon error".into(),
                });
            Err(CallError {
                code: info.code().into(),
                message: info.to_string(),
                info: raw,
            })
        }
        Err(error) => {
            tracing::warn!(channel, %error, "call failed");
            if client.session().is_closed() {
                release(&app, &bridge, &client).await;
            }
            Err(error.into())
        }
    }
}

/// Watch the shared connection: notice a lost daemon between calls and
/// reconnect to one that comes back (never spawning — a deliberately stopped
/// daemon must stay stopped) so event forwarding resumes with it.
pub fn watch(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(WATCH_INTERVAL).await;
            let bridge = app.state::<Bridge>();
            let mut state = bridge.state.lock().await;
            match state.client.as_ref() {
                Some(client) if client.session().is_closed() => {
                    tracing::info!("daemon connection lost");
                    state.client = None;
                    announce(&app, &mut state, ConnectionState::Disconnected);
                }
                Some(_) => {}
                None => {
                    let _ = adopt(&app, &mut state, connect(&app).await);
                }
            }
        }
    });
}

/// Bring the daemon up at shell start: opening the desktop is a deliberate
/// launch, so it spawns `hestiad` when none is running. `ipc_call` still never
/// spawns.
pub fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let bridge = app.state::<Bridge>();
        let mut state = bridge.state.lock().await;
        if let Err(error) = ensure_started(&app, &mut state).await {
            tracing::warn!(?error, "could not start the daemon at shell start");
        }
    });
}

pub(crate) async fn acquire(app: &AppHandle, bridge: &Bridge) -> Result<Arc<Client>, CallError> {
    let mut state = bridge.state.lock().await;
    if let Some(client) = state.client.as_ref() {
        if !client.session().is_closed() {
            return Ok(client.clone());
        }
        state.client = None;
        announce(app, &mut state, ConnectionState::Disconnected);
    }
    if state.backing_off() {
        return Err(CallError::offline());
    }
    let connected = connect(app).await;
    adopt(app, &mut state, connected)
}

/// Start the daemon and adopt the connection — the start button's trigger.
#[tauri::command]
pub async fn start_daemon(app: AppHandle, bridge: State<'_, Bridge>) -> Result<(), CallError> {
    let mut state = bridge.state.lock().await;
    ensure_started(&app, &mut state).await
}

async fn ensure_started(app: &AppHandle, state: &mut Connection) -> Result<(), CallError> {
    if state
        .client
        .as_ref()
        .is_some_and(|c| !c.session().is_closed())
    {
        return Ok(());
    }
    tracing::info!("starting the daemon");
    let started = match Client::start().await {
        Ok(client) => attach(app, client).await,
        Err(error) => Err(error.into()),
    };
    adopt(app, state, started).map(|_| ())
}

/// Record a connect attempt's outcome and announce the resulting state.
fn adopt(
    app: &AppHandle,
    state: &mut Connection,
    connected: Result<Arc<Client>, CallError>,
) -> Result<Arc<Client>, CallError> {
    match connected {
        Ok(client) => {
            state.client = Some(client.clone());
            state.failed_at = None;
            announce(app, state, ConnectionState::Connected);
            Ok(client)
        }
        Err(error) => {
            state.failed_at = Some(Instant::now());
            announce(app, state, ConnectionState::Disconnected);
            Err(error)
        }
    }
}

/// Only transitions reach the webview; a repeat is noise it would react to.
fn announce(app: &AppHandle, state: &mut Connection, next: ConnectionState) {
    if state.announced == Some(next) {
        return;
    }
    state.announced = Some(next);
    let _ = app.emit(CONNECTION_CHANNEL, next);
}

async fn release(app: &AppHandle, bridge: &Bridge, lost: &Arc<Client>) {
    let mut state = bridge.state.lock().await;
    if state
        .client
        .as_ref()
        .is_some_and(|held| Arc::ptr_eq(held, lost))
    {
        tracing::info!("daemon connection released");
        state.client = None;
        announce(app, &mut state, ConnectionState::Disconnected);
    }
}

/// Connect to a running daemon and wire event forwarding; never spawns.
async fn connect(app: &AppHandle) -> Result<Arc<Client>, CallError> {
    attach(app, Client::connect().await?).await
}

/// Forward every daemon event into the webview and subscribe to all of them.
async fn attach(app: &AppHandle, client: Client) -> Result<Arc<Client>, CallError> {
    let client = Arc::new(client);
    let emitter = app.clone();
    let mut events = client.session().listen(None);
    tauri::async_runtime::spawn(async move {
        while let Some(event) = events.recv().await {
            let _ = emitter.emit(
                EVENT_CHANNEL,
                EventPayload {
                    topic: event.topic,
                    payload: event.payload,
                },
            );
        }
    });
    client
        .session()
        .call::<EventsSubscribe>(&EventsSubscribeParams::default())
        .await?;
    tracing::info!("connected to the daemon");
    Ok(client)
}
