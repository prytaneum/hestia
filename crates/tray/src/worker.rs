//! The daemon-facing side of the tray: a background thread that polls the
//! daemon over the client SDK, executes menu actions, and reports state
//! changes to whatever renders the icon.

use std::ops::ControlFlow;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::Duration;

use client::Client;
use tokio::runtime::Runtime;

const POLL_INTERVAL: Duration = Duration::from_secs(2);
const RESTART_GRACE: Duration = Duration::from_millis(500);
const AUTOSTART_KEY: &str = "autostart";

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DaemonState {
    pub running: bool,
    pub version: Option<String>,
    pub autostart: bool,
}

pub enum Action {
    Start,
    Restart,
    SetAutostart(bool),
    OpenDesktop,
    Quit,
}

/// Where the worker reports to — the platform's rendering of the icon.
/// Implemented over the event loop on Windows and over the tray handle on
/// Linux, so the polling loop itself is platform-agnostic.
pub trait Sink: Send + 'static {
    fn state(&self, state: DaemonState);
    /// Tear the icon down and end the process.
    fn exit(&self);
}

pub fn spawn<S: Sink>(rx: Receiver<Action>, sink: S) {
    std::thread::spawn(move || run(rx, sink));
}

fn run<S: Sink>(rx: Receiver<Action>, sink: S) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build tokio runtime");
    let mut worker = Worker {
        rt,
        client: None,
        autostart: false,
    };
    let mut last: Option<DaemonState> = None;

    worker.push_state(&mut last, &sink);
    loop {
        match rx.recv_timeout(POLL_INTERVAL) {
            Ok(action) => {
                if worker.perform(action).is_break() {
                    sink.exit();
                    return;
                }
                worker.push_state(&mut last, &sink);
            }
            Err(RecvTimeoutError::Timeout) => {
                worker.push_state(&mut last, &sink);
            }
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

struct Worker {
    rt: Runtime,
    client: Option<Client>,
    // Reading autostart runs a subprocess on some platforms (schtasks on
    // Windows), so it is fetched per connection and after a toggle — never on
    // the poll tick.
    autostart: bool,
}

impl Worker {
    fn perform(&mut self, action: Action) -> ControlFlow<()> {
        match action {
            Action::Start => {
                tracing::info!("starting the daemon");
                self.connect_spawning();
            }
            Action::Restart => {
                tracing::info!("restarting the daemon");
                if let Some(c) = self.client.take() {
                    let _ = self.rt.block_on(c.daemon().stop(false));
                }
                // Give the old daemon a moment to release the endpoint.
                std::thread::sleep(RESTART_GRACE);
                self.connect_spawning();
            }
            Action::SetAutostart(enabled) => {
                self.connect_if_needed();
                if let Some(c) = &self.client {
                    if let Err(e) = self
                        .rt
                        .block_on(c.config().set(AUTOSTART_KEY, serde_json::json!(enabled)))
                    {
                        tracing::warn!(enabled, "cannot set autostart: {e}");
                    }
                    self.autostart = fetch_autostart(&self.rt, c);
                }
            }
            Action::OpenDesktop => crate::desktop::launch(),
            Action::Quit => {
                crate::desktop::quit();
                self.stop_daemon();
                return ControlFlow::Break(());
            }
        }
        ControlFlow::Continue(())
    }

    /// Quit means "stop the daemon, leave the workloads" — the third meaning of
    /// `daemon stop` (ask) has nowhere to go from a menu item, and a tray quit
    /// silently killing someone's running server would be the worse guess.
    fn stop_daemon(&mut self) {
        self.connect_if_needed();
        let Some(c) = &self.client else {
            return;
        };
        tracing::info!("quit: stopping the daemon, keeping supervised workloads");
        if let Err(e) = self.rt.block_on(c.daemon().stop(false)) {
            tracing::warn!("daemon stop failed: {e}");
        }
    }

    fn connect_spawning(&mut self) {
        match self.rt.block_on(Client::start()) {
            Ok(c) => {
                self.autostart = fetch_autostart(&self.rt, &c);
                self.client = Some(c);
            }
            Err(e) => tracing::warn!("cannot start the daemon: {e}"),
        }
    }

    fn connect_if_needed(&mut self) {
        if self.client.is_none() {
            if let Ok(c) = self.rt.block_on(Client::connect()) {
                self.autostart = fetch_autostart(&self.rt, &c);
                self.client = Some(c);
            }
        }
    }

    fn poll(&mut self) -> DaemonState {
        self.connect_if_needed();
        let Some(c) = &self.client else {
            return DaemonState::default();
        };
        let status = self.rt.block_on(c.daemon().status());
        match status {
            Ok(s) => DaemonState {
                running: true,
                version: Some(s.version),
                autostart: self.autostart,
            },
            Err(_) => {
                self.client = None;
                DaemonState::default()
            }
        }
    }

    fn push_state<S: Sink>(&mut self, last: &mut Option<DaemonState>, sink: &S) {
        let state = self.poll();
        if last.as_ref() != Some(&state) {
            *last = Some(state.clone());
            sink.state(state);
        }
    }
}

fn fetch_autostart(rt: &Runtime, client: &Client) -> bool {
    rt.block_on(client.config().get(AUTOSTART_KEY))
        .ok()
        .flatten()
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
}
