//! The Hestia plugin SDK. The imports every plugin may use are always here;
//! each export is a cargo feature, and [`export!`] exports exactly the enabled
//! ones.

#[doc(hidden)]
pub mod base {
    wit_bindgen::generate!({
        path: "wit/kernel",
        world: "everything",
    });
}

/// The error a plugin returns to Hestia.
pub use base::hestia::kernel::types::PluginError as Error;
#[cfg(target_arch = "wasm32")]
pub use base::hestia::kernel::{
    app, data, download, ipc, jobs, log, messages, notify, permissions, programs, progress,
    secrets, settings,
};
#[cfg(not(target_arch = "wasm32"))]
pub use mock::imports::{
    app, data, download, ipc, jobs, log, messages, notify, permissions, programs, progress,
    secrets, settings,
};

#[cfg(not(target_arch = "wasm32"))]
pub mod mock;
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
            "hestia:kernel/jobs@0.1.0": crate::base::hestia::kernel::jobs,
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
            "hestia:kernel/jobs@0.1.0": crate::base::hestia::kernel::jobs,
        },
        pub_export_macro: true,
        export_macro_name: "export_settings_changed",
        default_bindings_module: "hestia::settings_changed",
    });
}

/// Called after the user changes one of the plugin's settings.
#[cfg(feature = "settings-changed")]
pub use settings_changed::exports::hestia::kernel::settings_changed::Guest as SettingsChanged;

#[doc(hidden)]
#[cfg(feature = "commands")]
pub mod commands {
    wit_bindgen::generate!({
        path: "wit/kernel",
        world: "commands-plugin",
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
            "hestia:kernel/jobs@0.1.0": crate::base::hestia::kernel::jobs,
        },
        pub_export_macro: true,
        export_macro_name: "export_commands",
        default_bindings_module: "hestia::commands",
    });
}

/// Runs a command the manifest declares; `args` is a JSON object of its declared arguments.
#[cfg(feature = "commands")]
pub use commands::exports::hestia::kernel::commands::Guest as Commands;

#[doc(hidden)]
#[cfg(feature = "on-schedule")]
pub mod on_schedule {
    wit_bindgen::generate!({
        path: "wit/kernel",
        world: "on-schedule-plugin",
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
            "hestia:kernel/jobs@0.1.0": crate::base::hestia::kernel::jobs,
        },
        pub_export_macro: true,
        export_macro_name: "export_on_schedule",
        default_bindings_module: "hestia::on_schedule",
    });
}

/// Called when a schedule the manifest declares ticks.
#[cfg(feature = "on-schedule")]
pub use on_schedule::exports::hestia::kernel::on_schedule::Guest as OnSchedule;

#[doc(hidden)]
#[cfg(feature = "on-job-done")]
pub mod on_job_done {
    wit_bindgen::generate!({
        path: "wit/kernel",
        world: "on-job-done-plugin",
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
            "hestia:kernel/jobs@0.1.0": crate::base::hestia::kernel::jobs,
        },
        pub_export_macro: true,
        export_macro_name: "export_on_job_done",
        default_bindings_module: "hestia::on_job_done",
    });
}

/// Called when a job this plugin started ends.
#[cfg(feature = "on-job-done")]
pub use on_job_done::exports::hestia::kernel::on_job_done::{
    Guest as OnJobDone, Outcome as JobOutcome,
};

/// Exports `$plugin` for every export feature enabled on this crate; the type
/// implements the trait of each one.
#[macro_export]
macro_rules! export {
    ($plugin:ident) => {
        $crate::__export_lifecycle!($plugin);
        $crate::__export_settings_changed!($plugin);
        $crate::__export_commands!($plugin);
        $crate::__export_on_schedule!($plugin);
        $crate::__export_on_job_done!($plugin);
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

#[doc(hidden)]
#[cfg(feature = "commands")]
#[macro_export]
macro_rules! __export_commands {
    ($plugin:ident) => {
        $crate::commands::export_commands!($plugin);
    };
}

#[doc(hidden)]
#[cfg(not(feature = "commands"))]
#[macro_export]
macro_rules! __export_commands {
    ($plugin:ident) => {};
}

#[doc(hidden)]
#[cfg(feature = "on-schedule")]
#[macro_export]
macro_rules! __export_on_schedule {
    ($plugin:ident) => {
        $crate::on_schedule::export_on_schedule!($plugin);
    };
}

#[doc(hidden)]
#[cfg(not(feature = "on-schedule"))]
#[macro_export]
macro_rules! __export_on_schedule {
    ($plugin:ident) => {};
}

#[doc(hidden)]
#[cfg(feature = "on-job-done")]
#[macro_export]
macro_rules! __export_on_job_done {
    ($plugin:ident) => {
        $crate::on_job_done::export_on_job_done!($plugin);
    };
}

#[doc(hidden)]
#[cfg(not(feature = "on-job-done"))]
#[macro_export]
macro_rules! __export_on_job_done {
    ($plugin:ident) => {};
}
