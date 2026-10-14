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
pub const VENDOR: &str = "prytaneum";
/// The release channel this build was shipped on, stamped at compile time by
/// the release workflow. It seeds the self-update feed (`update.channel`), so a
/// beta build looks for betas without the user having to ask — an untagged
/// local build is `dev`, which is no channel and falls back to stable.
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
pub const DAEMON_BIN: &[&str] = &["hestiad.exe"];
#[cfg(not(windows))]
pub const DAEMON_BIN: &[&str] = &["hestiad"];

/// The release feed the daemon checks for a newer version, and the minisign
/// public key its artifacts are verified against. Every front-end reaches this
/// through `update.check`, so these two constants are the only place either is
/// written down.
///
/// The channel is the last path segment (`…/updates/stable`), because GitHub's
/// own `releases/latest/download/` resolves only to the newest *non*-prerelease
/// release — it can serve the stable feed and has no way to name the beta one.
pub const UPDATE_ENDPOINT: &str = "https://api.hestia.prytaneum.dev/updates";
pub const UPDATE_PUBKEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDJDNjM3NzcxQUEwRTdDQUQKUldTdGZBNnFjWGRqTERoaEIzaXFJcU1ZdU1YdXBVUk16cFdGVFQzYmZtT3ZVRC9mbjdYU0dOQlkK";

/// The rotation spare. A binary trusts only the keys compiled into it, so a
/// successor must ship *before* it is ever needed — an empty slot here cannot
/// be filled in retrospectively for copies already installed. Generate it in
/// the same session as [`UPDATE_PUBKEY`], keep its private half offline and
/// apart from the signing key, and start signing with it only once the builds
/// that trust it are the ones in the field.
pub const UPDATE_PUBKEY_NEXT: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IENEQzQxRUVGQkU3M0UyRjkKUldUNTRuTys3eDdFelFCQXM2R21VbTN2RjljYTRpSGZOOURCclhxdGJuR0JNZlpwWEJCOHVaazQK";

/// Every key a release artifact may be signed with, newest last. Empty slots
/// are skipped, so an unrotated build simply trusts one key.
pub fn update_pubkeys() -> impl Iterator<Item = &'static str> {
    let keys = match dev_pubkey() {
        Some(key) => vec![key],
        None => vec![UPDATE_PUBKEY, UPDATE_PUBKEY_NEXT],
    };
    keys.into_iter().filter(|key| !key.is_empty())
}

/// `HESTIA_UPDATE_PUBKEY` replaces the compiled-in key set for a local test, so
/// testing self-update never means editing this file and remembering to put it
/// back. It is honoured only in a **debug** build, and only alongside
/// `HESTIA_UPDATE_ENDPOINT`: a key without a feed to go with it would change
/// what an ordinary debug build trusts, which is not what anyone is asking for.
#[cfg(debug_assertions)]
fn dev_pubkey() -> Option<&'static str> {
    use std::sync::OnceLock;

    static KEY: OnceLock<Option<&'static str>> = OnceLock::new();
    *KEY.get_or_init(|| {
        let set = |name| std::env::var(name).ok().filter(|v| !v.trim().is_empty());
        set("HESTIA_UPDATE_ENDPOINT")?;
        // Leaked once, deliberately: every caller wants `&'static str`, and a
        // debug-only override read once is not worth threading lifetimes for.
        Some(&*Box::leak(set("HESTIA_UPDATE_PUBKEY")?.into_boxed_str()))
    })
}

#[cfg(not(debug_assertions))]
fn dev_pubkey() -> Option<&'static str> {
    None
}

/// The Discord application Rich Presence is published as, from the Discord
/// developer portal. Not a secret and not a credential: it names the
/// application whose title and art the Discord client renders, and it travels
/// in every presence payload. There is no backend behind it — presence is local
/// IPC to the user's own Discord client, so the application exists only to hold
/// the name and the uploaded art assets.
pub const DISCORD_APP_ID: &str = "1532750283753656543";

/// The art asset key uploaded under the application's Rich Presence assets.
/// Discord resolves it against that application; an unknown key renders no
/// image rather than failing the update.
pub const DISCORD_LARGE_IMAGE: &str = "hestia";

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
