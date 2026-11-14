//! Facts about the machine the daemon runs on. They need no permission.
//!
//! ```no_run
//! let host = hestia::host::info()?;
//! hestia::log::info(&format!("{}-{}", host.os, host.arch));
//! # Ok::<(), hestia::Error>(())
//! ```

use proto::host::{HostInfo, HostInfoResult};
use proto::Empty;

use crate::Error;

/// The operating system and architecture in the vocabulary of a flavor's
/// `Target`, the classpath separator, the daemon's process id and the installed
/// memory where the platform reports it.
pub fn info() -> Result<HostInfoResult, Error> {
    crate::call::<HostInfo>(&Empty {})
}
