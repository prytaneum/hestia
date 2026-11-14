//! A mock host, so plugin code that calls the SDK runs in a plain `cargo test`
//! on the native target: no daemon, no WebAssembly, no component to build.
//!
//! The mock replaces only the transport underneath [`call`](crate::call),
//! [`run_job`](crate::run_job), [`http`](crate::http) and
//! [`settings`](crate::settings); the functions a plugin calls are the ones it
//! ships with. It exists on every target but `wasm32`, so nothing is switched on
//! and a plugin's own tests need no feature.
//!
//! Each test owns its mock: [`install`] binds a fresh one to the current thread
//! and dropping the returned [`Mock`] removes it, so tests running in parallel on
//! their own threads never see each other's answers. Script what the plugin will
//! ask, run the plugin code, then assert on what it did:
//!
//! ```
//! use hestia::http::{Method, Response};
//! use hestia::proto::host::{HostInfo, HostInfoResult};
//!
//! fn describe() -> Result<String, hestia::Error> {
//!     let host = hestia::host::info()?;
//!     let key: String = hestia::settings::get("api-key")?
//!         .ok_or_else(|| hestia::Error::NotConfigured("api-key".into()))?;
//!     let status: hestia::Value = hestia::http::Request::get("https://api.example.com/status")
//!         .header("authorization", format!("Bearer {key}"))
//!         .send_json()?;
//!     Ok(format!("{} {}", host.os, status["state"]))
//! }
//!
//! let mock = hestia::mock::install();
//! mock.reply::<HostInfo>(&HostInfoResult {
//!     os: "linux".into(),
//!     ..Default::default()
//! });
//! mock.setting("api-key", &"k");
//! mock.respond(
//!     Method::Get,
//!     "https://api.example.com/status",
//!     Response {
//!         status: 200,
//!         body: br#"{"state":"ok"}"#.to_vec(),
//!         ..Default::default()
//!     },
//! );
//!
//! assert_eq!(describe().unwrap(), "linux \"ok\"");
//! assert_eq!(
//!     mock.requests()[0].headers.get("authorization"),
//!     Some("Bearer k")
//! );
//! ```
//!
//! A call, job or request nobody scripted fails with [`Error::Failed`] naming
//! the channel or the method and URL, so a missing answer is never mistaken for
//! an empty one. A setting nobody scripted reads as unset, as in the host.

use std::cell::RefCell;
use std::collections::HashMap;
use std::marker::PhantomData;

use proto::contract::Contract;
use proto::job::Job;
use serde::Serialize;
use serde_json::Value;

use crate::bindings::hestia::plugin::types::PluginError;
use crate::http::{Headers, Method, Response};
use crate::Error;

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

#[derive(Default)]
struct State {
    replies: HashMap<&'static str, Result<String, PluginError>>,
    responses: HashMap<(Method, String), Result<Response, PluginError>>,
    settings: HashMap<String, Value>,
    calls: Vec<Call>,
    requests: Vec<SentRequest>,
}

/// A call or job the plugin made, with the params it sent.
#[derive(Debug, Clone, PartialEq)]
pub struct Call {
    pub channel: String,
    pub params: Value,
    /// Made with [`run_job`](crate::run_job) rather than [`call`](crate::call).
    pub job: bool,
}

/// An http request the plugin sent.
#[derive(Debug, Clone)]
pub struct SentRequest {
    pub method: Method,
    pub url: String,
    pub headers: Headers,
    pub body: Vec<u8>,
}

/// The mock host of the current thread; dropping it removes the mock. It stays
/// on the thread that installed it.
pub struct Mock {
    not_send: PhantomData<*const ()>,
}

/// Installs a fresh mock on the current thread, replacing any earlier one.
pub fn install() -> Mock {
    STATE.with(|state| *state.borrow_mut() = Some(State::default()));
    Mock {
        not_send: PhantomData,
    }
}

impl Drop for Mock {
    fn drop(&mut self) {
        STATE.with(|state| *state.borrow_mut() = None);
    }
}

fn with_state<T>(use_state: impl FnOnce(&mut State) -> T) -> T {
    STATE.with(|state| match state.borrow_mut().as_mut() {
        Some(state) => use_state(state),
        None => panic!("no mock host is installed on this thread; call hestia::mock::install()"),
    })
}

impl Mock {
    /// Answers every call of `C` with `result` until scripted again.
    pub fn reply<C: Contract>(&self, result: &C::Result) {
        let json = serde_json::to_string(result).expect("a contract result encodes");
        with_state(|state| state.replies.insert(C::CHANNEL, Ok(json)));
    }

    /// Settles every run of the job `C` with `done`.
    pub fn done<C: Job>(&self, done: &C::Done) {
        let json = serde_json::to_string(done).expect("a job's done payload encodes");
        with_state(|state| state.replies.insert(C::CHANNEL, Ok(json)));
    }

    /// Fails every call or run of `C` with `error`.
    pub fn fail<C: Contract>(&self, error: Error) {
        with_state(|state| state.replies.insert(C::CHANNEL, Err(error.into())));
    }

    /// Answers `method` to exactly `url` with `response`.
    pub fn respond(&self, method: Method, url: &str, response: Response) {
        with_state(|state| {
            state
                .responses
                .insert((method, url.to_owned()), Ok(response))
        });
    }

    /// Fails `method` to exactly `url` with `error`, as a refused or unreachable
    /// host would.
    pub fn refuse(&self, method: Method, url: &str, error: Error) {
        with_state(|state| {
            state
                .responses
                .insert((method, url.to_owned()), Err(error.into()))
        });
    }

    /// Sets the plugin setting `key`, as the user would.
    pub fn setting<T: Serialize>(&self, key: &str, value: &T) {
        let value = serde_json::to_value(value).expect("a setting value encodes");
        with_state(|state| state.settings.insert(key.to_owned(), value));
    }

    /// The current value of `key`, including what the plugin wrote itself.
    pub fn setting_value(&self, key: &str) -> Option<Value> {
        with_state(|state| state.settings.get(key).cloned())
    }

    /// Every call and job so far, in order.
    pub fn calls(&self) -> Vec<Call> {
        with_state(|state| state.calls.clone())
    }

    /// Every http request so far, in order.
    pub fn requests(&self) -> Vec<SentRequest> {
        with_state(|state| state.requests.clone())
    }
}

pub(crate) fn call(channel: &'static str, params: &str, job: bool) -> Result<String, Error> {
    with_state(|state| {
        state.calls.push(Call {
            channel: channel.to_owned(),
            params: serde_json::from_str(params).unwrap_or(Value::Null),
            job,
        });
        match state.replies.get(channel) {
            Some(reply) => reply.clone().map_err(Error::from),
            None => Err(Error::Failed(format!(
                "mock host: no reply scripted for channel `{channel}`"
            ))),
        }
    })
}

pub(crate) fn send(request: crate::http::Request) -> Result<Response, Error> {
    with_state(|state| {
        state.requests.push(SentRequest {
            method: request.method,
            url: request.url.clone(),
            headers: request.headers.clone(),
            body: request.body.clone(),
        });
        match state.responses.get(&(request.method, request.url.clone())) {
            Some(response) => response.clone().map_err(Error::from),
            None => Err(Error::Failed(format!(
                "mock host: no response scripted for {:?} {}",
                request.method, request.url
            ))),
        }
    })
}

pub(crate) fn setting_get(key: &str) -> Option<String> {
    with_state(|state| state.settings.get(key).map(Value::to_string))
}

pub(crate) fn setting_set(key: &str, json: &str) -> Result<(), Error> {
    let value: Value = serde_json::from_str(json)?;
    with_state(|state| {
        if value.is_null() {
            state.settings.remove(key);
        } else {
            state.settings.insert(key.to_owned(), value);
        }
    });
    Ok(())
}
