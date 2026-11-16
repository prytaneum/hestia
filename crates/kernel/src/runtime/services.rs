use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

pub use super::bindings::hestia::kernel::notify::Level as NoticeLevel;
use super::error::GuestError;

pub type Answer<'a> = Pin<Box<dyn Future<Output = bool> + Send + 'a>>;

/// What the sandbox needs from the host around it: where a plugin's files live,
/// its message catalogue, and the front-ends that draw what it shows. The
/// defaults provide nothing: no files (`settings` answers `unsupported`), no
/// catalogue (a key reads as itself), and nobody watching. Notices and progress
/// are best effort; `confirm` and `open_url` say whether anyone saw them.
pub trait Services: Send + Sync {
    /// The directory mounted as the plugin's `/data`.
    fn data_dir(&self, _plugin: &str) -> Option<PathBuf> {
        None
    }

    /// Where settings and secrets are kept, outside `/data`, so the plugin never
    /// sees or links to it.
    fn state_dir(&self, _plugin: &str) -> Option<PathBuf> {
        None
    }

    /// The text for `key` in the plugin's catalogue, in the user's language.
    fn message(&self, _plugin: &str, _key: &str) -> Option<String> {
        None
    }

    /// `job` is the job the call runs in, here and in [`Services::confirm`] and
    /// [`Services::open_url`]: what the call shows is for the front-end that started it.
    fn notify(&self, _plugin: &str, _job: Option<&str>, _level: NoticeLevel, _message: &str) {}

    /// Asks the user; `false` when there is nobody to ask or they declined. The
    /// sandbox bounds the wait.
    fn confirm<'a>(
        &'a self,
        _plugin: &'a str,
        _job: Option<&'a str>,
        _message: &'a str,
    ) -> Answer<'a> {
        Box::pin(async { false })
    }

    /// Shows the user an https address to open; `url` is already vetted.
    /// `false` when there is nobody to show it to.
    fn open_url(&self, _plugin: &str, _job: Option<&str>, _url: &str) -> bool {
        false
    }

    /// Starts the plugin's own command `command` as a job and returns its id.
    /// The host checks that it is one the plugin declared; the sandbox has
    /// already spent the wake-up.
    fn start_job(&self, _plugin: &str, _command: &str, _args: &str) -> Result<String, GuestError> {
        Err(GuestError::Unsupported)
    }

    /// Progress of the job `job` the call runs in; a call outside a job has none.
    fn progress(&self, _plugin: &str, _job: &str, _current: u64, _total: u64, _message: &str) {}
}

/// The services of a host that provides none.
pub(super) struct Unavailable;

impl Services for Unavailable {}
