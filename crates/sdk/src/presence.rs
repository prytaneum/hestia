//! The plugin's Discord status card.
//!
//! The plugin needs the `presence:set` permission. While its card is set it is
//! shown instead of the launcher's own; [`clear`] restores that. The host
//! publishes at most 5 updates per 20 seconds, so a card may reach Discord a
//! few seconds after the call. A text outside 2 to 128 characters, more than 2
//! buttons, a label over 32 characters or a button url that is not https is
//! [`Error::Invalid`].
//!
//! ```no_run
//! use hestia::presence::{self, Activity, Button};
//!
//! presence::set(&Activity {
//!     details: Some("Playing Skyblock".into()),
//!     buttons: vec![Button { label: "Website".into(), url: "https://example.com".into() }],
//!     ..Activity::default()
//! })?;
//! presence::clear()?;
//! # Ok::<(), hestia::Error>(())
//! ```

pub use proto::presence::{Activity, Button};

use crate::bindings::hestia::plugin::presence as host;
use crate::Error;

/// Replaces the plugin's card.
pub fn set(activity: &Activity) -> Result<(), Error> {
    Ok(host::set(&serde_json::to_string(activity)?)?)
}

/// Removes the plugin's card.
pub fn clear() -> Result<(), Error> {
    Ok(host::clear()?)
}
