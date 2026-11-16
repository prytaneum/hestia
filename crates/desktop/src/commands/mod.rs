//! The shell's bespoke Tauri commands — the deliberate exceptions to the
//! generic `bridge::ipc_call` pipe, one module per feature.

pub mod auth;
pub mod diagnostics;
pub mod icons;
pub mod open;
pub mod opener;
pub mod prefs;
pub mod screenshots;
pub mod splash;
