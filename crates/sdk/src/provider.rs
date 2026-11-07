//! Answering provider operations. The host calls [`Plugin::provide`](crate::Plugin::provide)
//! for every operation of every provider the manifest contributes under
//! `contributes.providers`; route each to a handler with [`Provide::on`], typed
//! by the operation in [`proto::provider`](crate::proto::provider). An operation
//! no handler takes answers `unsupported`, which the host treats as the
//! operation's documented fallback.
//!
//! ```no_run
//! use hestia::provider::{Provide, Provided};
//! use hestia::proto::content::{SearchResult, SourceInfo};
//! use hestia::proto::provider::content::{Info, Search};
//!
//! struct Mirror;
//!
//! impl hestia::Plugin for Mirror {
//!     fn provide(call: Provide) -> Provided {
//!         call.on::<Info>(|_source, _| {
//!             Ok(SourceInfo { name: "Mirror".into(), ..SourceInfo::default() })
//!         })
//!         .on::<Search>(|_source, _query| Ok(SearchResult::default()))
//!         .finish()
//!     }
//! }
//!
//! hestia::plugin!(Mirror);
//! ```

use proto::provider::{ProviderKind, ProviderOp};

use crate::bindings::hestia::plugin::types::PluginError;
use crate::Error;

/// One operation the host is asking, waiting for the handler that answers it.
pub struct Provide {
    kind: String,
    id: String,
    op: String,
    args: String,
    answer: Option<Result<String, Error>>,
}

/// What [`Provide::finish`] hands back to the host.
pub struct Provided(Result<String, PluginError>);

impl Provided {
    #[doc(hidden)]
    pub fn into_answer(self) -> Result<String, PluginError> {
        self.0
    }
}

impl Provide {
    #[doc(hidden)]
    pub fn new(kind: String, id: String, op: String, args: String) -> Self {
        Provide {
            kind,
            id,
            op,
            args,
            answer: None,
        }
    }

    /// The kind of the provider being asked, such as `content`.
    pub fn kind(&self) -> &str {
        &self.kind
    }

    /// The id the manifest gives the provider being asked.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Answers `O` with `handler` when it is the operation being asked. The
    /// handler receives the provider's id, for a plugin that contributes several.
    pub fn on<O: ProviderOp>(
        mut self,
        handler: impl FnOnce(&str, O::Args) -> Result<O::Reply, Error>,
    ) -> Self {
        let matches = self.kind == <O::Kind as ProviderKind>::KIND && self.op == O::OP;
        if self.answer.is_some() || !matches {
            return self;
        }
        let answer = serde_json::from_str(&self.args)
            .map_err(|error| Error::Invalid(error.to_string()))
            .and_then(|args| handler(&self.id, args))
            .and_then(|reply| Ok(serde_json::to_string(&reply)?));
        self.answer = Some(answer);
        self
    }

    /// The answer of the handler that matched, or `unsupported`.
    pub fn finish(self) -> Provided {
        Provided(match self.answer {
            Some(answer) => answer.map_err(Into::into),
            None => Err(PluginError::Unsupported(format!(
                "{} `{}` does not implement `{}`",
                self.kind, self.id, self.op
            ))),
        })
    }
}
