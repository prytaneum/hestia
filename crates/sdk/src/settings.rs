//! The plugin's own settings, as the manifest declares them. The user sets
//! them; the plugin may set them too, for a token it obtained itself or a choice
//! it wants to remember. Secrets arrive in clear, to this plugin only.
//!
//! ```no_run
//! let key: Option<String> = hestia::settings::get("api-key")?;
//! # Ok::<(), hestia::Error>(())
//! ```

use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::Error;

/// Reads `key`: what the user set, else the manifest's default, else `None`.
pub fn get<T: DeserializeOwned>(key: &str) -> Result<Option<T>, Error> {
    host_get(key)
        .map(|json| serde_json::from_str(&json).map_err(Error::from))
        .transpose()
}

/// Stores `value` as the plugin's own setting `key`.
///
/// The host checks it against the manifest's declaration exactly as it checks
/// the user's write: [`Error::NotFound`] for a key the manifest does not
/// declare, [`Error::Invalid`] for a value of the wrong kind or outside its
/// constraints. The user's windows are told the settings changed. The plugin's
/// own [`Plugin::on_settings_changed`](crate::Plugin::on_settings_changed) does
/// not run for this write, so setting a key from there cannot loop.
///
/// ```no_run
/// // A plugin with a `secret` setting `token`, filled in by its own sign-in:
/// hestia::settings::set("token", &"s3cret")?;
/// # Ok::<(), hestia::Error>(())
/// ```
pub fn set<T: Serialize>(key: &str, value: &T) -> Result<(), Error> {
    host_set(key, &serde_json::to_string(value)?)
}

/// Clears `key` back to the manifest's default.
///
/// ```no_run
/// hestia::settings::clear("token")?;
/// # Ok::<(), hestia::Error>(())
/// ```
pub fn clear(key: &str) -> Result<(), Error> {
    host_set(key, "null")
}

#[cfg(target_arch = "wasm32")]
fn host_get(key: &str) -> Option<String> {
    crate::bindings::hestia::plugin::settings::get(key)
}

#[cfg(target_arch = "wasm32")]
fn host_set(key: &str, json: &str) -> Result<(), Error> {
    Ok(crate::bindings::hestia::plugin::settings::set(key, json)?)
}

#[cfg(not(target_arch = "wasm32"))]
fn host_get(key: &str) -> Option<String> {
    crate::mock::setting_get(key)
}

#[cfg(not(target_arch = "wasm32"))]
fn host_set(key: &str, json: &str) -> Result<(), Error> {
    crate::mock::setting_set(key, json)
}
