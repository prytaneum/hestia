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
mod hooks;
pub mod http;
pub mod log;
pub mod progress;
pub mod ui;

pub use call::{call, run_job};
pub use command::args;
pub use error::Error;
pub use hooks::{InstallContext, LaunchContext, LaunchPatch, ServerContext};
/// The daemon's wire contracts and domain types, the same ones the CLI uses.
pub use proto;
use proto::plugin::PageContext;
pub use serde_json::{self, json, Value};
use ui::{Action, Effect, UiNode};

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

    /// Draws a page the manifest contributes; build the tree with [`ui`]. An
    /// `entry` page finds the id of its server or instance in `context.entry`.
    fn render(page: &str, context: PageContext) -> Result<UiNode, Error> {
        let _ = context;
        Err(Error::NotFound(format!("unknown page `{page}`")))
    }

    /// Handles a click or a submit on a page. `values` holds a submitted form's
    /// fields by name. The effects it returns are applied in order.
    fn action(
        page: &str,
        action: Action,
        values: Value,
        context: PageContext,
    ) -> Result<Vec<Effect>, Error> {
        let _ = (action, values, context);
        Err(Error::NotFound(format!("unknown page `{page}`")))
    }

    /// Runs just before a game launch, for a plugin that declares the
    /// `before-launch` hook. Answer the arguments to add; the host keeps only
    /// an allowlist of them. An `Err` vetoes the launch, and its text is the
    /// reason shown to the user. A call that overruns its two seconds is skipped
    /// instead.
    fn before_launch(context: LaunchContext) -> Result<LaunchPatch, String> {
        let _ = context;
        Ok(LaunchPatch::default())
    }

    /// Runs just before a server is spawned, for a plugin that declares the
    /// `before-server-start` hook. The same allowlist applies as for
    /// [`before_launch`](Plugin::before_launch), except that
    /// `extra_game_args` is ignored. An `Err` vetoes the start, and its text is
    /// the reason shown to the user. A call that overruns its two seconds is
    /// skipped instead.
    fn before_server_start(context: ServerContext) -> Result<LaunchPatch, String> {
        let _ = context;
        Ok(LaunchPatch::default())
    }

    /// Runs for each file about to land in a server or instance (an install and
    /// each required dependency, a local import, an update, or a modpack's
    /// file), for a plugin that declares the `before-content-install` hook. An `Err` refuses that file, and its text is
    /// shown to the user with the file name. The context holds names and a
    /// sha1, never the file's bytes, and a plugin that fails or overruns its two
    /// seconds is skipped: this is a convenience check, not a security scanner.
    fn before_content_install(context: InstallContext) -> Result<(), String> {
        let _ = context;
        Ok(())
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

            impl $crate::bindings::exports::hestia::plugin::ui::Guest for Export {
                fn render(
                    page: ::std::string::String,
                    context: ::std::string::String,
                ) -> ::core::result::Result<
                    ::std::string::String,
                    $crate::bindings::hestia::plugin::types::PluginError,
                > {
                    let context = $crate::serde_json::from_str(&context)
                        .map_err(|error| $crate::Error::Invalid(error.to_string()))?;
                    let tree = <$plugin as $crate::Plugin>::render(&page, context)?;
                    $crate::serde_json::to_string(&tree)
                        .map_err(|error| $crate::Error::Failed(error.to_string()).into())
                }

                fn action(
                    page: ::std::string::String,
                    action: ::std::string::String,
                    values: ::std::string::String,
                    context: ::std::string::String,
                ) -> ::core::result::Result<
                    ::std::string::String,
                    $crate::bindings::hestia::plugin::types::PluginError,
                > {
                    let invalid = |error: $crate::serde_json::Error| {
                        $crate::Error::Invalid(error.to_string())
                    };
                    let effects = <$plugin as $crate::Plugin>::action(
                        &page,
                        $crate::serde_json::from_str(&action).map_err(invalid)?,
                        $crate::serde_json::from_str(&values).map_err(invalid)?,
                        $crate::serde_json::from_str(&context).map_err(invalid)?,
                    )?;
                    $crate::serde_json::to_string(&effects)
                        .map_err(|error| $crate::Error::Failed(error.to_string()).into())
                }
            }

            impl $crate::bindings::exports::hestia::plugin::launch_hooks::Guest for Export {
                fn before_launch(
                    ctx: $crate::LaunchContext,
                ) -> ::core::result::Result<
                    $crate::bindings::exports::hestia::plugin::launch_hooks::LaunchPatch,
                    ::std::string::String,
                > {
                    <$plugin as $crate::Plugin>::before_launch(ctx).map(::core::convert::Into::into)
                }

                fn before_server_start(
                    ctx: $crate::ServerContext,
                ) -> ::core::result::Result<
                    $crate::bindings::exports::hestia::plugin::launch_hooks::LaunchPatch,
                    ::std::string::String,
                > {
                    <$plugin as $crate::Plugin>::before_server_start(ctx)
                        .map(::core::convert::Into::into)
                }
            }

            impl $crate::bindings::exports::hestia::plugin::content_hooks::Guest for Export {
                fn before_content_install(
                    ctx: $crate::InstallContext,
                ) -> ::core::result::Result<(), ::std::string::String> {
                    <$plugin as $crate::Plugin>::before_content_install(ctx)
                }
            }

            $crate::bindings::__export_world!(Export with_types_in $crate::bindings);
        };
    };
}
