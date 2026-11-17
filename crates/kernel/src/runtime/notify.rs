use std::time::Duration;

use reqwest::Url;

use super::bindings::hestia::kernel::notify::{self, Level};
use super::bindings::hestia::kernel::types::PluginError;
use super::host::State;
use crate::vetting::{Hosts, Policy};

const NOTICES_PER_CALL: usize = 5;
/// Across calls: a burst of this many notices, then one more every 5 seconds.
pub(super) const BURST: f64 = 10.0;
pub(super) const REFILL_PER_SECOND: f64 = 0.2;
const MESSAGE_BYTES: usize = 1024;
const URL_BYTES: usize = 2048;
/// How long a question waits for its answer; the call's budget is paused meanwhile.
const QUESTION_PATIENCE: Duration = Duration::from_secs(120);

/// What one call has put in front of the user so far; each kind has its own
/// allowance.
#[derive(Default)]
pub(super) struct Drawn {
    notices: usize,
    confirms: usize,
    urls: usize,
}

fn text(message: &str) -> &str {
    &message[..message.floor_char_boundary(MESSAGE_BYTES)]
}

/// The network policy's rules for a URL nobody granted: https, a host name, no
/// credentials.
fn vetted(url: &str) -> Result<String, PluginError> {
    let invalid = || PluginError::Invalid("url".into());
    if url.len() > URL_BYTES {
        return Err(invalid());
    }
    let parsed = Url::parse(url).map_err(|_| invalid())?;
    Policy::new(Hosts::open_unhashed())
        .vet(&parsed, true)
        .map_err(|_| invalid())?;
    Ok(parsed.into())
}

impl notify::Host for State {
    fn notify(&mut self, level: Level, message: String) -> Result<(), PluginError> {
        if self.drawn.notices >= NOTICES_PER_CALL {
            return Err(PluginError::Busy);
        }
        if !self.shared.notices.spend(&self.plugin) {
            return Err(PluginError::Busy);
        }
        self.drawn.notices += 1;
        self.shared.services.notify(
            &self.plugin,
            self.call.job.as_deref(),
            level,
            text(&message),
        );
        Ok(())
    }

    async fn confirm(&mut self, message: String) -> Result<bool, PluginError> {
        if !self.call.person {
            return Err(PluginError::Unsupported);
        }
        if self.drawn.confirms >= 1 {
            return Err(PluginError::Busy);
        }
        self.drawn.confirms += 1;
        let _waiting = self.call.pause.wait();
        let asked =
            self.shared
                .services
                .confirm(&self.plugin, self.call.job.as_deref(), text(&message));
        Ok(tokio::time::timeout(QUESTION_PATIENCE, asked)
            .await
            .unwrap_or(false))
    }

    fn open_url(&mut self, url: String) -> Result<(), PluginError> {
        let url = vetted(&url)?;
        if self.drawn.urls >= 1 {
            return Err(PluginError::Busy);
        }
        self.drawn.urls += 1;
        match self
            .shared
            .services
            .open_url(&self.plugin, self.call.job.as_deref(), &url)
        {
            true => Ok(()),
            false => Err(PluginError::Unsupported),
        }
    }
}
