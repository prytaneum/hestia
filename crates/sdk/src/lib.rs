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
pub use base::hestia::kernel::{app, data, log, messages, notify, progress, secrets, settings};
/// The bindings generator the SDK is built with, for a guest that imports a
/// built-in's package (`runtime_path: "hestia::wit_bindgen::rt"`).
pub use wit_bindgen;

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
            "hestia:kernel/data@0.1.0": crate::base::hestia::kernel::data,
            "hestia:kernel/settings@0.1.0": crate::base::hestia::kernel::settings,
            "hestia:kernel/secrets@0.1.0": crate::base::hestia::kernel::secrets,
            "hestia:kernel/messages@0.1.0": crate::base::hestia::kernel::messages,
            "hestia:kernel/progress@0.1.0": crate::base::hestia::kernel::progress,
            "hestia:kernel/notify@0.1.0": crate::base::hestia::kernel::notify,
        },
        pub_export_macro: true,
        export_macro_name: "export_lifecycle",
        default_bindings_module: "hestia::lifecycle",
    });
}

/// Called when the plugin is enabled or loaded.
#[cfg(feature = "lifecycle")]
pub use lifecycle::exports::hestia::kernel::lifecycle::Guest as Lifecycle;

#[doc(hidden)]
#[cfg(feature = "settings-changed")]
pub mod settings_changed {
    wit_bindgen::generate!({
        path: "wit/kernel",
        world: "settings-changed-plugin",
        with: {
            "hestia:kernel/types@0.1.0": crate::base::hestia::kernel::types,
            "hestia:kernel/log@0.1.0": crate::base::hestia::kernel::log,
            "hestia:kernel/app@0.1.0": crate::base::hestia::kernel::app,
            "hestia:kernel/data@0.1.0": crate::base::hestia::kernel::data,
            "hestia:kernel/settings@0.1.0": crate::base::hestia::kernel::settings,
            "hestia:kernel/secrets@0.1.0": crate::base::hestia::kernel::secrets,
            "hestia:kernel/messages@0.1.0": crate::base::hestia::kernel::messages,
            "hestia:kernel/progress@0.1.0": crate::base::hestia::kernel::progress,
            "hestia:kernel/notify@0.1.0": crate::base::hestia::kernel::notify,
        },
        pub_export_macro: true,
        export_macro_name: "export_settings_changed",
        default_bindings_module: "hestia::settings_changed",
    });
}

/// Called after the user changes one of the plugin's settings.
#[cfg(feature = "settings-changed")]
pub use settings_changed::exports::hestia::kernel::settings_changed::Guest as SettingsChanged;

/// Exports `$plugin` for every export feature enabled on this crate; the type
/// implements the trait of each one.
#[macro_export]
macro_rules! export {
    ($plugin:ident) => {
        $crate::__export_lifecycle!($plugin);
        $crate::__export_settings_changed!($plugin);
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

#[doc(hidden)]
#[cfg(feature = "settings-changed")]
#[macro_export]
macro_rules! __export_settings_changed {
    ($plugin:ident) => {
        $crate::settings_changed::export_settings_changed!($plugin);
    };
}

#[doc(hidden)]
#[cfg(not(feature = "settings-changed"))]
#[macro_export]
macro_rules! __export_settings_changed {
    ($plugin:ident) => {};
}
