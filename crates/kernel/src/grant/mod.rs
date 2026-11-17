//! What a component asks for, read from its imports and exports, and the
//! digest the user's approval is bound to.

mod approval;
mod derivation;
mod endpoint;
mod kernel;
#[cfg(feature = "sandbox")]
mod pin;
#[cfg(feature = "sandbox")]
mod plan;
mod program;
mod table;
mod versions;

use crate::shape::Shape;
#[cfg(feature = "sandbox")]
pub use approval::approval_digest;
pub use approval::widens;
#[cfg(feature = "sandbox")]
pub(crate) use approval::{carry, check_choice, effective};
pub use derivation::{derive, derive_everywhere};
pub(crate) use kernel::table as kernel_table;
#[cfg(all(feature = "sandbox", test))]
pub(crate) use pin::digest;
#[cfg(feature = "sandbox")]
pub(crate) use pin::{hash, open};
#[cfg(feature = "sandbox")]
pub use pin::{pin, Reserved};
#[cfg(feature = "sandbox")]
pub(crate) use plan::plan_digest;
pub use table::{Gate, Permission, Stability, Table};
pub use versions::check_versions;

pub(crate) fn package_of(interface: &str) -> Option<&str> {
    interface.split_once('/').map(|(package, _)| package)
}

#[derive(Debug, thiserror::Error)]
pub enum GrantError {
    #[error("the component imports '{0}', which is not an interface Hestia provides")]
    UnsupportedImport(String),
    #[error("'{0}' is not an interface of its package")]
    UnknownInterface(String),
    #[error("plugin.toml gives a scope to '{0}', which the component does not need")]
    ScopeNotNeeded(String),
    #[error("plugin.toml marks '{0}' optional, but the component does not need it")]
    OptionalNotNeeded(String),
    #[error("'{0}' cannot be optional: an interface it gates cannot be answered `denied`")]
    OptionalUndeniable(String),
    #[error("plugin.toml gives '{permission}' {given} scope, but it takes {takes}")]
    ScopeKind {
        permission: String,
        given: String,
        takes: String,
    },
    #[error("'{0}' needs a scope in plugin.toml")]
    ScopeMissing(String),
    #[error("'{0}' is not an optional permission of this plugin")]
    NotOptional(String),
    #[error("plugin.toml gives '{0}' unhashed hosts, but only hosts take them")]
    UnhashedNotTaken(String),
    #[error("plugin.toml gives '{1}' the unhashed host '{0}', which is not one of its hosts")]
    UnhashedOutsideHosts(String, String),
    #[error("plugin.toml grants an endpoint `{entry}`: {reason}")]
    Endpoint { entry: String, reason: String },
    #[error("plugin.toml grants '{permission}' the program `{entry}`: {reason}")]
    Program {
        permission: String,
        entry: String,
        reason: String,
    },
    #[error("plugin.toml declares {0}, but the component does not export them")]
    NotExported(&'static str),
    #[error("{what} is at API {asked}, but this Hestia provides {host}")]
    Api {
        what: String,
        asked: String,
        host: String,
    },
    #[error("plugin.toml says api {api}, but the component uses {import}")]
    ApiMismatch { api: String, import: String },
    #[error("'{interface}' is unstable ({feature}): only a plugin installed from a directory in developer mode may import it")]
    Unstable { interface: String, feature: String },
    #[error("'{interface}' arrived in {since}, but the plugin is built against {api}")]
    Since {
        interface: String,
        since: String,
        api: String,
    },
    #[error("plugin.toml grants '{permission}' {reason}")]
    Host {
        permission: String,
        reason: crate::vetting::HostError,
    },
}

/// The WIT package of the kernel's own interfaces.
pub const KERNEL_PACKAGE: &str = "hestia:kernel";

pub(crate) fn unversioned(name: &str) -> &str {
    name.split_once('@').map_or(name, |(base, _)| base)
}

/// Whether the sandbox can answer `denied` for the interface when its
/// permission is withheld: `wasi:` interfaces and `ipc`, which holds resources,
/// cannot, so a plugin importing one without the permission does not load.
pub(crate) fn deniable(import: &str) -> bool {
    let interface = unversioned(import);
    !interface.starts_with("wasi:") && interface != "hestia:kernel/ipc"
}

/// Interface name to the permission whose absence replaces it with a stub.
pub type Denials = std::collections::HashMap<String, String>;

/// Each import that needs a permission the grant lacks, to that permission.
pub fn denials(table: &Table, shape: &Shape, granted: &[&str]) -> Denials {
    shape
        .imports
        .iter()
        .filter_map(|import| match table.gate(unversioned(import)) {
            Gate::Needs(permission) if !granted.contains(&permission.name) => {
                Some((import.clone(), permission.name.to_owned()))
            }
            _ => None,
        })
        .collect()
}
