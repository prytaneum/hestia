//! Progress of the running command or flavor build, and whether it was cancelled.
//!
//! ```no_run
//! for step in 0..10 {
//!     if hestia::progress::cancelled() {
//!         break;
//!     }
//!     hestia::progress::report(Some(step as f32 / 10.0), "working");
//! }
//! ```

pub use crate::bindings::hestia::plugin::progress::{cancelled, report};
