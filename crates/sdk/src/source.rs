//! The decoding half of a content source's guest exports. See the
//! `source_*` methods of [`Plugin`](crate::Plugin).

use proto::content::ContentKind;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::bindings::hestia::plugin::types::PluginError;
use crate::Error;

#[doc(hidden)]
#[derive(Deserialize)]
pub struct UrlArgs {
    pub url: String,
}

#[doc(hidden)]
#[derive(Deserialize)]
pub struct ProjectArgs {
    pub project: String,
    pub kind: Option<ContentKind>,
}

#[doc(hidden)]
#[derive(Deserialize)]
pub struct IdsArgs {
    pub ids: Vec<String>,
}

#[doc(hidden)]
#[derive(Deserialize)]
pub struct Sha1Args {
    pub sha1s: Vec<String>,
}

/// Decodes `args`, runs `call` and encodes its answer; a payload that does not
/// decode is the host's mistake and answers `invalid`.
#[doc(hidden)]
pub fn answer<A: DeserializeOwned, R: Serialize>(
    args: &str,
    call: impl FnOnce(A) -> Result<R, Error>,
) -> Result<String, PluginError> {
    let args = serde_json::from_str(args).map_err(|error| Error::Invalid(error.to_string()))?;
    let reply = call(args)?;
    serde_json::to_string(&reply).map_err(|error| Error::Failed(error.to_string()).into())
}
