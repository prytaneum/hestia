use proto::plugin::Tier;

use crate::grant::{Permission, Stability, Table};

/// `hestia:probe`: `read` asks, `work` is open, and `filler` is the extension
/// point a plugin fills by exporting it. `preview` and `legacy` stand in for an
/// unstable and a deprecated interface.
pub fn table() -> Table {
    Table::new(
        vec![
            ("hestia:probe/read", Some("probe:read")),
            ("hestia:probe/work", None),
            ("hestia:probe/filler", None),
            ("hestia:probe/preview", None),
            ("hestia:probe/legacy", None),
        ],
        vec![Permission {
            name: "probe:read",
            tier: Tier::Sandboxed,
            scope: None,
            prompt: "Read what the probe built-in echoes",
        }],
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
