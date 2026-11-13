use std::fmt;

use serde::{Deserialize, Serialize};

use super::scope::ScopeKind;

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum Permission {
    #[serde(rename = "entry:read")]
    EntryRead,
    #[serde(rename = "entry:write")]
    EntryWrite,
    #[serde(rename = "process:read")]
    ProcessRead,
    #[serde(rename = "process:control")]
    ProcessControl,
    /// Run only the programs its scope names.
    #[serde(rename = "process:spawn")]
    ProcessSpawn,
    #[serde(rename = "content:read")]
    ContentRead,
    #[serde(rename = "content:write")]
    ContentWrite,
    /// Profiles only, never tokens.
    #[serde(rename = "account:read")]
    AccountRead,
    /// Only the settings [`crate::config::PLUGIN_ACCESS`] marks readable.
    #[serde(rename = "config:read")]
    ConfigRead,
    /// Only the settings [`crate::config::PLUGIN_ACCESS`] marks writable.
    #[serde(rename = "config:write")]
    ConfigWrite,
    #[serde(rename = "java:read")]
    JavaRead,
    #[serde(rename = "java:manage")]
    JavaManage,
    /// Outbound HTTP to the hosts its scope lists.
    #[serde(rename = "network:connect")]
    NetworkConnect,
    #[serde(rename = "ui:css")]
    UiCss,
    /// Return an allowlisted patch from a launch or server-start hook.
    #[serde(rename = "launch:patch")]
    LaunchPatch,
    /// Connect to the local endpoints its scope lists: Unix sockets in the
    /// user's runtime and temp directories, or Windows named pipes.
    #[serde(rename = "ipc:connect")]
    IpcConnect,
    /// Contribute a Minecraft flavor, which decides the main class, libraries and
    /// arguments the game runs with and so bypasses the launch-arg allowlist.
    #[serde(rename = "flavor:provide")]
    FlavorProvide,
    /// Contribute an archive format, whose blueprint chooses the game version,
    /// loader and JVM settings of the instance the user will run.
    #[serde(rename = "format:provide")]
    FormatProvide,
    /// Contribute a Java distribution, which picks the runtime binary Hestia
    /// executes for every server and game.
    #[serde(rename = "java:provide")]
    JavaProvide,
}

/// How far a permission reaches. An unrestricted one can run native code, so
/// the install prompt states it as full access to the computer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    Sandboxed,
    Unrestricted,
}

/// What a permission is: how far it reaches and which scope kinds narrow it.
/// The one table validation, widening, the digest and the approval prompt read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spec {
    pub tier: Tier,
    /// The kinds a manifest may give this permission, and no others.
    pub scopes: &'static [ScopeKind],
    /// Whether the permission is useless without at least one entry of its
    /// first scope kind.
    pub requires_scope: bool,
}

const fn spec(tier: Tier, scopes: &'static [ScopeKind], requires_scope: bool) -> Spec {
    Spec {
        tier,
        scopes,
        requires_scope,
    }
}

impl Permission {
    /// Every permission, in declaration order.
    pub const ALL: [Permission; 19] = [
        Permission::EntryRead,
        Permission::EntryWrite,
        Permission::ProcessRead,
        Permission::ProcessControl,
        Permission::ProcessSpawn,
        Permission::ContentRead,
        Permission::ContentWrite,
        Permission::AccountRead,
        Permission::ConfigRead,
        Permission::ConfigWrite,
        Permission::JavaRead,
        Permission::JavaManage,
        Permission::NetworkConnect,
        Permission::UiCss,
        Permission::LaunchPatch,
        Permission::IpcConnect,
        Permission::FlavorProvide,
        Permission::FormatProvide,
        Permission::JavaProvide,
    ];

    pub const fn spec(self) -> Spec {
        use ScopeKind::{Endpoints, Files, Hosts, Programs, Properties, Unhashed};
        const SANDBOXED: Tier = Tier::Sandboxed;
        const UNRESTRICTED: Tier = Tier::Unrestricted;
        match self {
            Permission::EntryRead => spec(SANDBOXED, &[Files], false),
            Permission::EntryWrite => spec(UNRESTRICTED, &[], false),
            Permission::ProcessRead => spec(SANDBOXED, &[], false),
            Permission::ProcessControl => spec(UNRESTRICTED, &[], false),
            Permission::ProcessSpawn => spec(UNRESTRICTED, &[Programs], true),
            Permission::ContentRead => spec(SANDBOXED, &[], false),
            Permission::ContentWrite => spec(UNRESTRICTED, &[], false),
            Permission::AccountRead => spec(SANDBOXED, &[], false),
            Permission::ConfigRead => spec(SANDBOXED, &[], false),
            Permission::ConfigWrite => spec(UNRESTRICTED, &[], false),
            Permission::JavaRead => spec(SANDBOXED, &[], false),
            Permission::JavaManage => spec(SANDBOXED, &[], false),
            Permission::NetworkConnect => spec(SANDBOXED, &[Hosts, Unhashed], true),
            Permission::UiCss => spec(SANDBOXED, &[], false),
            Permission::LaunchPatch => spec(SANDBOXED, &[Properties], false),
            Permission::IpcConnect => spec(UNRESTRICTED, &[Endpoints], true),
            Permission::FlavorProvide => spec(UNRESTRICTED, &[], false),
            Permission::FormatProvide => spec(UNRESTRICTED, &[], false),
            Permission::JavaProvide => spec(UNRESTRICTED, &[], false),
        }
    }

    pub fn tier(self) -> Tier {
        self.spec().tier
    }

    /// One line for the install prompt.
    pub fn description(self) -> &'static str {
        match self {
            Permission::EntryRead => "See your servers and instances and their settings",
            Permission::EntryWrite => {
                "Create, change and delete servers and instances, including their files and JVM arguments"
            }
            Permission::ProcessRead => "See which servers and games are running",
            Permission::ProcessControl => {
                "Start and stop servers and games, and run console commands"
            }
            Permission::ProcessSpawn => "Run the programs it names",
            Permission::ContentRead => "Search and inspect mods, packs and other content",
            Permission::ContentWrite => "Install, update and remove mods, packs and other content",
            Permission::AccountRead => "See the names of signed-in accounts, never their tokens",
            Permission::ConfigRead => "See the few launcher settings that are safe to share, never keys, locations or update settings",
            Permission::ConfigWrite => "Change the few launcher settings that are safe to hand over; everything that decides where data lives, what runs, what updates and what is downloaded stays yours",
            Permission::JavaRead => "See installed and available Java runtimes",
            Permission::JavaManage => "Install and remove Java runtimes",
            Permission::NetworkConnect => "Connect to the hosts it lists",
            Permission::UiCss => "Restyle the app with its own stylesheet",
            Permission::LaunchPatch => {
                "Adjust launch settings from an allowlist when a game or server starts"
            }
            Permission::IpcConnect => {
                "Talk to the programs running on this computer that it lists, such as a chat client"
            }
            Permission::FlavorProvide => {
                "Add game distributions that choose what code the game runs, including its main class and arguments"
            }
            Permission::FormatProvide => {
                "Add archive formats that choose the game version, loader and JVM settings of instances you import"
            }
            Permission::JavaProvide => {
                "Add Java distributions that choose the runtime binary Hestia executes"
            }
        }
    }
}

/// The `domain:action` name a manifest declares.
impl fmt::Display for Permission {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match serde_json::to_value(self) {
            Ok(serde_json::Value::String(name)) => f.write_str(&name),
            _ => Err(fmt::Error),
        }
    }
}
