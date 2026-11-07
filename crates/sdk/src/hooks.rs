//! Answering hook points. The host calls [`Plugin::intercept`](crate::Plugin::intercept)
//! for every point the manifest declares under `contributes.hooks`; route each
//! to a handler with [`Intercept::on`], typed by the point in
//! [`proto::hook`](crate::proto::hook).
//!
//! ```no_run
//! use hestia::hooks::{Intercept, Intercepted};
//! use hestia::proto::hook::{BeforeLaunch, LaunchPatch};
//!
//! struct Tuner;
//!
//! impl hestia::Plugin for Tuner {
//!     fn intercept(call: Intercept) -> Intercepted {
//!         call.on::<BeforeLaunch>(|context| {
//!             let mut patch = LaunchPatch::default();
//!             if context.memory.is_none() {
//!                 patch.extra_jvm_args.push("-XX:+UseZGC".into());
//!             }
//!             Ok(patch)
//!         })
//!         .finish()
//!     }
//! }
//!
//! hestia::plugin!(Tuner);
//! ```

use proto::hook::HookPoint;

/// One hook point the host is running, waiting for the handler that answers it.
pub struct Intercept {
    point: String,
    context: String,
    answer: Option<Result<String, String>>,
}

/// What [`Intercept::finish`] hands back to the host.
pub struct Intercepted(Result<String, String>);

impl Intercepted {
    #[doc(hidden)]
    pub fn into_answer(self) -> Result<String, String> {
        self.0
    }
}

impl Intercept {
    #[doc(hidden)]
    pub fn new(point: String, context: String) -> Self {
        Intercept {
            point,
            context,
            answer: None,
        }
    }

    /// The point being run.
    pub fn point(&self) -> &str {
        &self.point
    }

    /// Answers `P` with `handler` when it is the point being run. The handler
    /// returns the patch to add, or an `Err` that vetoes the operation; its text
    /// is the reason shown to the user.
    pub fn on<P: HookPoint>(
        mut self,
        handler: impl FnOnce(P::Context) -> Result<P::Patch, String>,
    ) -> Self {
        if self.answer.is_some() || self.point != P::NAME {
            return self;
        }
        self.answer = Some(match serde_json::from_str(&self.context) {
            Ok(context) => handler(context)
                .and_then(|patch| serde_json::to_string(&patch).map_err(|error| error.to_string())),
            Err(error) => {
                crate::log::warn(&format!("unreadable `{}` context: {error}", P::NAME));
                Ok("null".to_string())
            }
        });
        self
    }

    /// The answer of the handler that matched; a point no handler took adds
    /// nothing.
    pub fn finish(self) -> Intercepted {
        Intercepted(self.answer.unwrap_or_else(|| Ok("null".to_string())))
    }
}
