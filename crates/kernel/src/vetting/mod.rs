//! The one network policy: every request and download Hestia makes, core or
//! plugin, is vetted here. https only, no IP literal, no credentials in the URL,
//! a granted host, a resolver that refuses addresses inside the machine, a time
//! and size cap per request, and a body that is checksummed or comes from a host
//! declared to serve unhashed files. Redirects are walked by [`Policy::get`], so
//! each hop is vetted again.

#[cfg(feature = "sandbox")]
mod client;
mod hosts;
#[cfg(feature = "sandbox")]
mod policy;
#[cfg(feature = "sandbox")]
mod resolver;

use proto::error::ErrorInfo;

pub use hosts::{HostError, Hosts};
#[cfg(feature = "sandbox")]
pub use policy::{read, Failure, Limits, Policy};
#[cfg(feature = "sandbox")]
pub use resolver::was_refused;

/// Why a request was not made or its answer was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Refusal {
    #[error("only https requests are allowed")]
    NotHttps,
    #[error("the request URL is not valid")]
    BadUrl,
    #[error("the request URL carries credentials")]
    Credentials,
    #[error("an IP address cannot be a destination")]
    IpLiteral,
    #[error("`{0}` is not one of the hosts this request may use")]
    HostNotGranted(String),
    #[error("`{0}` is not a host declared as serving unhashed files")]
    Unhashed(String),
    #[error("the answer is larger than {0} bytes")]
    TooLarge(u64),
    #[error("the redirect target is not a valid URL")]
    BadRedirect,
    #[error("too many redirects")]
    TooManyRedirects,
}

impl From<Refusal> for ErrorInfo {
    fn from(refusal: Refusal) -> Self {
        ErrorInfo::DownloadFailed {
            detail: refusal.to_string(),
        }
    }
}
