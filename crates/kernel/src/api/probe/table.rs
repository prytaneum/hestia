use proto::plugin::{ScopeKind, Tier};

use crate::grant::{Permission, Stability, Table};

pub const READ: &str = "probe:read";
pub const ENTRIES: &str = "probe:entries";

/// `hestia:probe`: `read` asks, `entries` asks and takes the user's entries
/// scope, `work` is open, and `filler` is the extension point a plugin fills by
/// exporting it. `entry-opened` is a topic whose payload names an entry, so its
/// read permission is `entries`; `echoed` is one gated by `read`; `before-echo`
/// is a hook. `preview` and `legacy` stand in for an unstable and a deprecated
/// interface.
pub fn table() -> Table {
    Table::new(
        vec![
            ("hestia:probe/read", Some(READ)),
            ("hestia:probe/work", None),
            ("hestia:probe/entries", Some(ENTRIES)),
            ("hestia:probe/filler", None),
            ("hestia:probe/entry-opened", Some(ENTRIES)),
            ("hestia:probe/echoed", Some(READ)),
            ("hestia:probe/before-echo", None),
            ("hestia:probe/preview", None),
            ("hestia:probe/legacy", None),
        ],
        vec![
            Permission {
                name: READ,
                tier: Tier::Sandboxed,
                scope: None,
                prompt: "Read what the probe built-in echoes",
            },
            Permission {
                name: ENTRIES,
                tier: Tier::Sandboxed,
                scope: Some(ScopeKind::Entries),
                prompt: "Open and create entries of the probe built-in",
            },
        ],
    )
    .versioned("hestia:probe", "0.1.0")
    .stability(
        "hestia:probe/preview",
        Stability::Unstable { feature: "preview" },
    )
    .stability(
        "hestia:probe/legacy",
        Stability::Stable {
            since: "0.1.0",
            deprecated: Some("0.1.0"),
        },
    )
}
