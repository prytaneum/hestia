//! Answering the commands the manifest contributes under
//! `contributes.commands`. The host calls [`Plugin::command`](crate::Plugin::command)
//! for each run; route each command id to a handler with [`Commands::on`]. The
//! daemon has already checked the arguments against the manifest, so a handler
//! takes a struct of the declared arguments and a bad payload means the struct
//! and the declaration disagree. A command no handler takes answers
//! `unsupported`.
//!
//! A command's id is a runtime string from `plugin.toml`, so it is routed by
//! name; the things `proto` defines (hook points, provider operations, events)
//! are routed by type.
//!
//! ```no_run
//! use hestia::commands::{Commands, Ran, Scoped};
//! use hestia::Plugin;
//! use serde::Deserialize;
//!
//! #[derive(Deserialize)]
//! struct Greet {
//!     name: String,
//! }
//!
//! struct Hello;
//!
//! impl Plugin for Hello {
//!     fn command(call: Commands) -> Ran {
//!         call.on("greet", |args: Greet| Ok(format!("hello, {}", args.name)))
//!             .on("restart", |args: Scoped<Greet>| {
//!                 hestia::log::info(&format!("{} on {}", args.args.name, args.entry));
//!                 Ok(())
//!             })
//!             .finish()
//!     }
//! }
//!
//! hestia::plugin!(Hello);
//! ```

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::bindings::hestia::plugin::types::PluginError;
use crate::Error;

/// The arguments of a `server` or `instance` scoped command: the id of the
/// entry it acts on, which the daemon adds to the declared arguments as `entry`.
///
/// ```
/// use hestia::commands::Scoped;
///
/// #[derive(serde::Deserialize)]
/// struct Args {
///     count: u32,
/// }
///
/// let scoped: Scoped<Args> =
///     serde_json::from_str(r#"{"entry":"srv-1","count":3}"#).unwrap();
/// assert_eq!((scoped.entry.as_str(), scoped.args.count), ("srv-1", 3));
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scoped<A> {
    pub entry: String,
    #[serde(flatten)]
    pub args: A,
}

/// One command the host is running, waiting for the handler that answers it.
pub struct Commands {
    name: String,
    args: String,
    answer: Option<Result<String, Error>>,
}

/// What [`Commands::finish`] hands back to the host.
pub struct Ran(Result<String, PluginError>);

impl Ran {
    #[doc(hidden)]
    pub fn into_answer(self) -> Result<String, PluginError> {
        self.0
    }
}

impl Commands {
    #[doc(hidden)]
    pub fn new(name: String, args: String) -> Self {
        Commands {
            name,
            args,
            answer: None,
        }
    }

    /// The id of the command being run.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Answers `name` with `handler` when it is the command being run. `A` is the
    /// plugin's own struct of the declared arguments, [`Scoped`] around it for a
    /// scoped command, or [`Value`](crate::Value) for the raw object; a command
    /// with none declared takes [`Empty`](crate::proto::Empty). The handler
    /// returns any serializable value, which is the job's result. Report through
    /// [`progress`](crate::progress) and return soon after
    /// [`progress::cancelled`](crate::progress::cancelled) turns true.
    pub fn on<A: DeserializeOwned, R: Serialize>(
        mut self,
        name: &str,
        handler: impl FnOnce(A) -> Result<R, Error>,
    ) -> Self {
        if self.answer.is_some() || self.name != name {
            return self;
        }
        self.answer = Some(
            serde_json::from_str(&self.args)
                .map_err(|error| Error::Invalid(format!("arguments of `{name}`: {error}")))
                .and_then(handler)
                .and_then(|output| Ok(serde_json::to_string(&output)?)),
        );
        self
    }

    /// The answer of the handler that matched, or `unsupported` naming the
    /// command.
    pub fn finish(self) -> Ran {
        Ran(match self.answer {
            Some(answer) => answer.map_err(Into::into),
            None => Err(PluginError::Unsupported(format!("command {}", self.name))),
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[derive(Deserialize)]
    struct Args {
        name: String,
    }

    fn call(name: &str, args: serde_json::Value) -> Commands {
        Commands::new(name.into(), args.to_string())
    }

    fn answer(call: Commands) -> Result<serde_json::Value, PluginError> {
        call.finish()
            .into_answer()
            .map(|text| serde_json::from_str(&text).unwrap())
    }

    #[test]
    fn a_command_routes_to_its_handler_with_typed_arguments() {
        let reply = answer(
            call("greet", json!({ "name": "Bob" }))
                .on("other", |_: Args| Ok("wrong"))
                .on("greet", |args: Args| Ok(format!("hello, {}", args.name))),
        );
        assert_eq!(reply.unwrap(), json!("hello, Bob"));
    }

    #[test]
    fn the_first_matching_handler_answers() {
        let reply = answer(
            call("a", json!({}))
                .on("a", |_: serde_json::Value| Ok(1))
                .on("a", |_: serde_json::Value| Ok(2)),
        );
        assert_eq!(reply.unwrap(), json!(1));
    }

    #[test]
    fn a_scoped_command_receives_its_entry() {
        let reply = answer(
            call("go", json!({ "entry": "srv-1", "name": "x" }))
                .on("go", |scoped: Scoped<Args>| {
                    Ok(format!("{}:{}", scoped.entry, scoped.args.name))
                }),
        );
        assert_eq!(reply.unwrap(), json!("srv-1:x"));
    }

    #[test]
    fn mismatched_arguments_are_invalid() {
        let reply = answer(call("greet", json!({ "name": 3 })).on("greet", |_: Args| Ok(())));
        assert!(matches!(reply, Err(PluginError::Invalid(message)) if message.contains("greet")));
    }

    #[test]
    fn a_handler_error_is_the_answer() {
        let reply = answer(
            call("fail", json!({})).on("fail", |_: serde_json::Value| -> Result<(), Error> {
                Err(Error::Busy("claimed".into()))
            }),
        );
        assert!(matches!(reply, Err(PluginError::Busy(_))));
    }

    #[test]
    fn an_unrouted_command_is_unsupported_and_named() {
        let reply = answer(call("nope", json!({})).on("greet", |_: Args| Ok(())));
        assert!(
            matches!(reply, Err(PluginError::Unsupported(message)) if message.contains("nope"))
        );
    }
}
