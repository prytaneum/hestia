//! The Hestia plugin SDK. The imports every plugin may use are always here;
//! each export is a cargo feature, and [`export!`] exports exactly the enabled
//! ones.

#[doc(hidden)]
pub mod base {
    wit_bindgen::generate!({
        path: "wit/kernel",
        world: "base",
    });
}

/// The error a plugin returns to Hestia.
pub use base::hestia::kernel::types::PluginError as Error;
pub use base::hestia::kernel::{app, log};

#[doc(hidden)]
#[cfg(feature = "lifecycle")]
pub mod lifecycle {
    wit_bindgen::generate!({
        path: "wit/kernel",
        world: "lifecycle-plugin",
        with: {
            "hestia:kernel/types@0.1.0": crate::base::hestia::kernel::types,
            "hestia:kernel/log@0.1.0": crate::base::hestia::kernel::log,
            "hestia:kernel/app@0.1.0": crate::base::hestia::kernel::app,
        },
        pub_export_macro: true,
        export_macro_name: "export_lifecycle",
        default_bindings_module: "hestia::lifecycle",
    });
}

/// Called when the plugin is enabled or loaded.
#[cfg(feature = "lifecycle")]
pub use lifecycle::exports::hestia::kernel::lifecycle::Guest as Lifecycle;

/// Exports `$plugin` for every export feature enabled on this crate; the type
/// implements the trait of each one.
#[macro_export]
macro_rules! export {
    ($plugin:ident) => {
        $crate::__export_lifecycle!($plugin);
    };
}

#[doc(hidden)]
#[cfg(feature = "lifecycle")]
#[macro_export]
macro_rules! __export_lifecycle {
    ($plugin:ident) => {
        $crate::lifecycle::export_lifecycle!($plugin);
    };
}

#[doc(hidden)]
#[cfg(not(feature = "lifecycle"))]
#[macro_export]
macro_rules! __export_lifecycle {
    ($plugin:ident) => {};
}
