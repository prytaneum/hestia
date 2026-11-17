//! Shared application identity — the single source of truth every front-end and
//! the daemon read, mirroring the generated `app_info.h` in the C++ tree.

pub const NAME: &str = "Hestia";
pub const ID: &str = "org.prytaneum.hestia";
/// The tray's own GApplication/desktop id. It must differ from [`ID`] (the
/// desktop shell's Tauri identifier): both front-ends register a GApplication
/// under this name on Linux, and GApplication enforces single-instance by
/// D-Bus name ownership — sharing the id makes the second process launched a
/// remote instance that never shows, so the tray and desktop would block each
/// other.
pub const TRAY_ID: &str = "org.prytaneum.hestia.tray";
/// The flag the tray passes to re-launch the desktop shell just to close it:
/// the running instance routes it through single-instance and exits.
pub const DESKTOP_QUIT_ARG: &str = "--quit";
/// The flag that has the desktop shell start the daemon and exit, opening no
/// window. An AppImage's login entry starts the daemon this way, since the
/// shell is the image's only entry point.
pub const DESKTOP_DAEMON_ARG: &str = "--daemon";
/// The named-pipe namespace Hestia's own pipes live in; plugins are never granted a name under it.
pub const PIPE_NAMESPACE: &str = r"\\.\pipe\hestia-";
pub const VENDOR: &str = "prytaneum";
/// The release channel this build was shipped on, stamped at compile time by
/// the release workflow. An untagged local build is `dev`.
pub const CHANNEL: &str = match option_env!("HESTIA_CHANNEL") {
    Some(channel) => channel,
    None => "dev",
};
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

// Shipped name first, cargo's output last, so a dev build still resolves.
#[cfg(windows)]
pub const DESKTOP_BIN: &[&str] = &["Hestia.exe", "hestia-desktop.exe"];
#[cfg(not(windows))]
pub const DESKTOP_BIN: &[&str] = &["hestia-desktop"];

#[cfg(windows)]
pub const TRAY_BIN: &[&str] = &["Hestia Tray.exe", "hestia-tray.exe"];
#[cfg(not(windows))]
pub const TRAY_BIN: &[&str] = &["hestia-tray"];

#[cfg(windows)]
pub const CLI_BIN: &[&str] = &["hestia.exe"];
#[cfg(not(windows))]
pub const CLI_BIN: &[&str] = &["hestia"];

#[cfg(windows)]
pub const DAEMON_BIN: &[&str] = &["hestiad.exe"];
#[cfg(not(windows))]
pub const DAEMON_BIN: &[&str] = &["hestiad"];

/// The agent every outbound HTTP request identifies itself with.
///
/// PaperMC and Modrinth both *ask* for a contact URL or address alongside the
/// name — so an upstream can reach a misbehaving client instead of blocking it
/// — but neither enforces it, and Hestia has no published home to name yet. A
/// contact belongs here when there is a real one; an invented URL would point
/// upstreams somewhere that is not us, which is worse than staying anonymous.
pub fn user_agent() -> String {
    format!("{NAME}/{VERSION}")
}

#[cfg(debug_assertions)]
pub const VERSION_LABEL: &str = concat!(env!("CARGO_PKG_VERSION"), "-debug");
#[cfg(not(debug_assertions))]
pub const VERSION_LABEL: &str = env!("CARGO_PKG_VERSION");
