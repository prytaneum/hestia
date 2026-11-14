//! A mock host, so plugin code that calls the SDK runs in a plain `cargo test`
//! on the native target: no daemon, no WebAssembly, no component to build.
//!
//! The mock replaces only the transport underneath [`call`](crate::call),
//! [`run_job`](crate::run_job), [`http`](crate::http),
//! [`settings`](crate::settings), [`process`](crate::process),
//! [`download`](crate::download) and [`data`](crate::data); the functions a
//! plugin calls are the ones it ships with. It exists on every target but `wasm32`, so nothing is switched on
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
//! A call, job, request, program run or download nobody scripted fails with
//! [`Error::Failed`] naming the channel, the method and URL, the program or the
//! URL, so a missing answer is never mistaken for an empty one. A setting or a
//! data key nobody scripted reads as unset, as in the host.
//!
//! Nothing touches the disk: [`data`](crate::data) values live in the mock, and
//! a download writes no file at its destination.

use std::cell::RefCell;
use std::collections::HashMap;
use std::marker::PhantomData;

use proto::contract::Contract;
use proto::job::Job;
use serde::Serialize;
use serde_json::Value;

use crate::bindings::hestia::plugin::types::PluginError;
use crate::http::{Headers, Method, Response};
use crate::process::Output;
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
    outputs: HashMap<String, Result<Output, PluginError>>,
    runs: Vec<Run>,
    spawned: Vec<String>,
    running: Vec<String>,
    logs: HashMap<String, Vec<String>>,
    files: HashMap<String, Result<u64, PluginError>>,
    downloads: Vec<Download>,
    data: HashMap<String, Vec<u8>>,
}

/// A program the plugin ran or started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub cwd: Option<String>,
    pub java_major: Option<u32>,
    /// Started with [`spawn`](crate::process::spawn) rather than run to
    /// completion with [`exec`](crate::process::exec).
    pub spawned: bool,
}

/// A download the plugin asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Download {
    pub url: String,
    pub dest: String,
    /// The digest the file had to match, as hex; `None` for an unhashed one.
    pub digest: Option<String>,
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

    /// Finishes every [`exec`](crate::process::exec) of `program` with `output`.
    pub fn exec(&self, program: &str, output: Output) {
        with_state(|state| state.outputs.insert(program.to_owned(), Ok(output)));
    }

    /// Fails every [`exec`](crate::process::exec) of `program` with `error`.
    pub fn fail_exec(&self, program: &str, error: Error) {
        with_state(|state| state.outputs.insert(program.to_owned(), Err(error.into())));
    }

    /// The captured output lines of the spawned process `id`.
    pub fn process_logs(&self, id: &str, lines: &[&str]) {
        let lines = lines.iter().map(|line| line.to_string()).collect();
        with_state(|state| state.logs.insert(id.to_owned(), lines));
    }

    /// Every program run or started so far, in order.
    pub fn runs(&self) -> Vec<Run> {
        with_state(|state| state.runs.clone())
    }

    /// Serves exactly `url` as a file of `size` bytes that matches whatever
    /// digest the plugin expects.
    pub fn file(&self, url: &str, size: u64) {
        with_state(|state| state.files.insert(url.to_owned(), Ok(size)));
    }

    /// Fails the download of exactly `url` with `error`.
    pub fn fail_download(&self, url: &str, error: Error) {
        with_state(|state| state.files.insert(url.to_owned(), Err(error.into())));
    }

    /// Every download asked for so far, in order.
    pub fn downloads(&self) -> Vec<Download> {
        with_state(|state| state.downloads.clone())
    }

    /// Saves `value` under the data key `key`, as an earlier call would have.
    pub fn data<T: Serialize>(&self, key: &str, value: &T) {
        let bytes = serde_json::to_vec(value).expect("a data value encodes");
        with_state(|state| state.data.insert(key.to_owned(), bytes));
    }

    /// What is saved under the data key `key` now.
    pub fn data_value(&self, key: &str) -> Option<Value> {
        with_state(|state| state.data.get(key).cloned())
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
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

pub(crate) mod process {
    use super::{with_state, Run};
    use crate::bindings::hestia::plugin::types::PluginError;
    use crate::Error;

    pub(crate) use crate::bindings::hestia::plugin::process::{ExecOutput, ProcessStatus, State};

    pub(crate) fn exec(
        program: &str,
        args: &[String],
        env: &[(String, String)],
        _timeout_ms: u32,
        cwd: Option<&str>,
        java_major: Option<u32>,
    ) -> Result<ExecOutput, PluginError> {
        with_state(|state| {
            state.runs.push(Run {
                program: program.to_owned(),
                args: args.to_vec(),
                env: env.to_vec(),
                cwd: cwd.map(str::to_owned),
                java_major,
                spawned: false,
            });
            match state.outputs.get(program) {
                Some(output) => output.clone().map(|output| ExecOutput {
                    status: output.status,
                    stdout: output.stdout,
                    stderr: output.stderr,
                }),
                None => Err(Error::Failed(format!(
                    "mock host: no output scripted for program `{program}`"
                ))
                .into()),
            }
        })
    }

    pub(crate) fn spawn(
        program: &str,
        args: &[String],
        env: &[(String, String)],
        java_major: Option<u32>,
    ) -> Result<String, PluginError> {
        with_state(|state| {
            state.runs.push(Run {
                program: program.to_owned(),
                args: args.to_vec(),
                env: env.to_vec(),
                cwd: None,
                java_major,
                spawned: true,
            });
            let id = format!("mock-{}", state.spawned.len() + 1);
            state.spawned.push(id.clone());
            state.running.push(id.clone());
            Ok(id)
        })
    }

    fn unknown(id: &str) -> PluginError {
        Error::NotFound(format!("mock host: the plugin started no process `{id}`")).into()
    }

    pub(crate) fn stop(id: &str) -> Result<(), PluginError> {
        with_state(|state| {
            if !state.spawned.iter().any(|known| known == id) {
                return Err(unknown(id));
            }
            state.running.retain(|running| running != id);
            Ok(())
        })
    }

    pub(crate) fn status(id: &str) -> Result<ProcessStatus, PluginError> {
        with_state(|state| {
            if !state.spawned.iter().any(|known| known == id) {
                return Err(unknown(id));
            }
            Ok(match state.running.iter().any(|running| running == id) {
                true => ProcessStatus {
                    state: State::Running,
                    exit_code: None,
                },
                false => ProcessStatus {
                    state: State::Exited,
                    exit_code: Some(0),
                },
            })
        })
    }

    pub(crate) fn logs(id: &str, tail: u32) -> Result<Vec<String>, PluginError> {
        with_state(|state| {
            if !state.spawned.iter().any(|known| known == id) {
                return Err(unknown(id));
            }
            let lines = state.logs.get(id).cloned().unwrap_or_default();
            let skip = lines.len().saturating_sub(tail as usize);
            Ok(lines.into_iter().skip(skip).collect())
        })
    }

    pub(crate) fn list() -> Result<Vec<String>, PluginError> {
        with_state(|state| Ok(state.running.clone()))
    }
}

pub(crate) mod download {
    use super::{with_state, Download};
    use crate::bindings::hestia::plugin::types::PluginError;
    use crate::Error;

    pub(crate) use crate::bindings::hestia::plugin::download::{Algorithm, Checksum, Outcome};

    pub(crate) fn download(
        url: &str,
        expect: Option<&Checksum>,
        dest: &str,
    ) -> Result<Outcome, PluginError> {
        with_state(|state| {
            state.downloads.push(Download {
                url: url.to_owned(),
                dest: dest.to_owned(),
                digest: expect.map(|checksum| checksum.hex.clone()),
            });
            match state.files.get(url) {
                Some(size) => size.clone().map(|size| Outcome { size }),
                None => Err(Error::Failed(format!("mock host: no file scripted for {url}")).into()),
            }
        })
    }
}

pub(crate) mod data {
    use super::with_state;

    pub(crate) fn read(key: &str) -> Option<Vec<u8>> {
        with_state(|state| state.data.get(key).cloned())
    }

    pub(crate) fn write(key: &str, bytes: Vec<u8>) {
        with_state(|state| state.data.insert(key.to_owned(), bytes));
    }
}

pub(crate) mod log {
    pub fn trace(_message: &str) {}
    pub fn debug(_message: &str) {}
    pub fn info(_message: &str) {}
    pub fn warn(_message: &str) {}
    pub fn error(_message: &str) {}
}

pub(crate) mod progress {
    pub fn report(_fraction: Option<f32>, _message: &str) {}

    pub fn cancelled() -> bool {
        false
    }
}
