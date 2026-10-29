use proto::contract::Contract;
use proto::job::Job;

use crate::bindings::hestia::plugin::host;
use crate::Error;

/// Calls a daemon channel through its contract, so the params and the result
/// are the same typed `proto` values the CLI uses. The plugin needs the
/// permission the contract names.
///
/// ```no_run
/// use hestia::proto::{server::ServerList, Empty};
///
/// let servers = hestia::call::<ServerList>(&Empty {})?;
/// hestia::log::info(&format!("{} servers", servers.len()));
/// # Ok::<(), hestia::Error>(())
/// ```
pub fn call<C: Contract>(params: &C::Params) -> Result<C::Result, Error> {
    let reply = host::call(C::CHANNEL, &serde_json::to_string(params)?)?;
    Ok(serde_json::from_str(&reply)?)
}

/// Starts a job and waits for it to settle, returning its `Done` payload. The
/// daemon assigns the job's id, so leave the params' `id` empty. A job that
/// fails or is cancelled returns an error.
///
/// ```no_run
/// use hestia::proto::java::{JavaInstall, JavaInstallParams};
///
/// let params = JavaInstallParams {
///     major: 21,
///     ..JavaInstallParams::default()
/// };
/// let done = hestia::run_job::<JavaInstall>(&params)?;
/// # Ok::<(), hestia::Error>(())
/// ```
pub fn run_job<C: Job>(params: &C::Params) -> Result<C::Done, Error> {
    let reply = host::run_job(C::CHANNEL, &serde_json::to_string(params)?)?;
    Ok(serde_json::from_str(&reply)?)
}
