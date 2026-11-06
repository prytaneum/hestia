//! The plugin's own settings, as the manifest declares them and the user set
//! them. Secrets arrive in clear, to this plugin only.
//!
//! ```no_run
//! let key: Option<String> = hestia::settings::get("api-key")?;
//! # Ok::<(), hestia::Error>(())
//! ```

use serde::de::DeserializeOwned;

use crate::bindings::hestia::plugin::settings;
use crate::Error;

/// Reads `key`: what the user set, else the manifest's default, else `None`.
pub fn get<T: DeserializeOwned>(key: &str) -> Result<Option<T>, Error> {
    settings::get(key)
        .map(|json| serde_json::from_str(&json).map_err(Error::from))
        .transpose()
}
