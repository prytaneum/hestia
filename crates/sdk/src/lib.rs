//! Write Hestia plugins in Rust.
//!
//! A plugin is a WebAssembly component built for `wasm32-wasip2`. Implement
//! [`Plugin`] on a type and export it with [`plugin!`]; reach the daemon with
//! [`call`] and [`run_job`], log through [`log`], and keep state with [`data`].
//!
//! ```no_run
//! use hestia::{Error, Plugin};
//!
//! struct Hello;
//!
//! impl Plugin for Hello {
//!     fn init() -> Result<(), Error> {
//!         hestia::log::info("hello from a plugin");
//!         Ok(())
//!     }
//! }
//!
//! hestia::plugin!(Hello);
//! ```

mod call;
mod command;
pub mod data;
mod error;
pub mod log;
pub mod progress;

pub use call::{call, run_job};
pub use command::args;
pub use error::Error;
/// The daemon's wire contracts and domain types, the same ones the CLI uses.
pub use proto;
pub use serde_json::{self, json, Value};

#[doc(hidden)]
pub mod bindings {
    wit_bindgen::generate!({
        path: "wit",
        world: "plugin",
        pub_export_macro: true,
        export_macro_name: "__export_world",
    });
}

/// What a plugin does when the daemon calls it. Every method has a default
/// that does nothing, so a plugin implements only what it needs.
///
/// Every call runs on a fresh instance of the plugin: nothing kept in memory,
/// statics included, survives from one call to the next. Keep state in
/// [`data`]. Calls may also run concurrently, each on its own instance.
///
/// ```no_run
/// use hestia::{Error, Plugin};
///
/// struct Greeter;
///
/// impl Plugin for Greeter {
///     fn init() -> Result<(), Error> {
///         hestia::data::save("greeting", &"hello")
///     }
/// }
///
/// hestia::plugin!(Greeter);
/// ```
pub trait Plugin {
    /// Runs once when the plugin is enabled. Use it for one-time setup in
    /// [`data`] and to check the plugin's environment; an error leaves the
    /// plugin failed with that message.
    fn init() -> Result<(), Error> {
        Ok(())
    }

    /// Runs a command the manifest contributes; `args` is an object of its
    /// declared arguments, plus `entry` for a scoped command. Read them with
    /// [`args`]. Report through [`progress`] and return soon after
    /// [`progress::cancelled`] turns true.
    fn run(command: &str, args: Value) -> Result<Value, Error> {
        let _ = args;
        Err(Error::NotFound(format!("unknown command `{command}`")))
    }

    /// Receives a daemon event matching one of the manifest's `contributes.events`
    /// that the plugin's permissions allow. Events arrive one at a time and in
    /// order, each with a few seconds to finish; a plugin that falls behind loses
    /// the oldest.
    fn on_event(topic: &str, payload: Value) {
        let _ = (topic, payload);
    }
}

/// Exports a [`Plugin`] implementation as the component's entry points. Use it
/// once, at the crate root of a `cdylib` crate.
///
/// ```no_run
/// struct Quiet;
///
/// impl hestia::Plugin for Quiet {}
///
/// hestia::plugin!(Quiet);
/// ```
#[macro_export]
macro_rules! plugin {
    ($plugin:ty) => {
        const _: () = {
            struct Export;

            impl $crate::bindings::exports::hestia::plugin::lifecycle::Guest for Export {
                fn init(
                ) -> ::core::result::Result<(), $crate::bindings::hestia::plugin::types::PluginError>
                {
                    <$plugin as $crate::Plugin>::init().map_err(::core::convert::Into::into)
                }
            }

            impl $crate::bindings::exports::hestia::plugin::commands::Guest for Export {
                fn run(
                    command: ::std::string::String,
                    args: ::std::string::String,
                ) -> ::core::result::Result<
                    ::std::string::String,
                    $crate::bindings::hestia::plugin::types::PluginError,
                > {
                    let args = $crate::serde_json::from_str(&args)
                        .map_err(|error| $crate::Error::Invalid(error.to_string()))?;
                    <$plugin as $crate::Plugin>::run(&command, args)
                        .map(|output| output.to_string())
                        .map_err(::core::convert::Into::into)
                }
            }

            impl $crate::bindings::exports::hestia::plugin::events::Guest for Export {
                fn on_event(topic: ::std::string::String, payload: ::std::string::String) {
                    let payload = $crate::serde_json::from_str(&payload).unwrap_or_default();
                    <$plugin as $crate::Plugin>::on_event(&topic, payload)
                }
            }

            $crate::bindings::__export_world!(Export with_types_in $crate::bindings);
        };
    };
}
