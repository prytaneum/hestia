use proto::plugin::Tier;

use crate::grant::{Permission, Table};

/// `hestia:probe`: `read` asks, `work` is open, and `filler` is the extension
/// point a plugin fills by exporting it.
pub fn table() -> Table {
    Table::new(
        vec![
            ("hestia:probe/read", Some("probe:read")),
            ("hestia:probe/work", None),
            ("hestia:probe/filler", None),
        ],
        vec![Permission {
            name: "probe:read",
            tier: Tier::Sandboxed,
            scope: None,
            prompt: "Read what the probe built-in echoes",
        }],
    )
}
