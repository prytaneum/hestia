use proto::plugin::{ScopeKind, Tier};

use crate::grant::{Permission, Table, KERNEL_PACKAGE};

/// The version of `crates/sdk/wit/kernel`; the WIT snapshot test keeps the two equal.
const KERNEL_VERSION: &str = "0.1.0";

/// `hestia:kernel` and the `wasi:http` that rides on `network:connect`.
/// Beyond `secrets` and the network, programs and local endpoints, nothing
/// asks: the rest stays inside the plugin's own call, files and dialogs
/// Hestia draws.
pub fn table() -> Table {
    Table::new(
        vec![
            ("hestia:kernel/types", None),
            ("hestia:kernel/log", None),
            ("hestia:kernel/app", None),
            ("hestia:kernel/data", None),
            ("hestia:kernel/settings", None),
            ("hestia:kernel/permissions", None),
            ("hestia:kernel/secrets", Some("secrets")),
            ("hestia:kernel/messages", None),
            ("hestia:kernel/progress", None),
            ("hestia:kernel/notify", None),
            ("hestia:kernel/download", Some("network:connect")),
            ("hestia:kernel/programs", Some("process:spawn")),
            ("hestia:kernel/ipc", Some("ipc:connect")),
            ("wasi:http/types", Some("network:connect")),
            ("wasi:http/outgoing-handler", Some("network:connect")),
            ("hestia:kernel/lifecycle", None),
            ("hestia:kernel/settings-changed", None),
            ("hestia:kernel/jobs", None),
            ("hestia:kernel/commands", None),
            ("hestia:kernel/on-schedule", None),
            ("hestia:kernel/on-job-done", None),
        ],
        vec![
            Permission {
                name: "secrets",
                tier: Tier::Sandboxed,
                scope: None,
                prompt: "Keep secrets you enter for it, such as tokens",
            },
            Permission {
                name: "network:connect",
                tier: Tier::Sandboxed,
                scope: Some(ScopeKind::Hosts),
                prompt: "Connect to the listed hosts over https and download files from them",
            },
            Permission {
                name: "process:spawn",
                tier: Tier::Unrestricted,
                scope: Some(ScopeKind::Programs),
                prompt: "Run the listed programs on your computer, with the listed arguments",
            },
            Permission {
                name: "ipc:connect",
                tier: Tier::Unrestricted,
                scope: Some(ScopeKind::Endpoints),
                prompt: "Talk to the listed local sockets and pipes, which other programs serve",
            },
        ],
    )
    .versioned(KERNEL_PACKAGE, KERNEL_VERSION)
}
