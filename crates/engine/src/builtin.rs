use std::sync::Arc;

use kernel::Builtin;

/// The built-ins compiled into `hestiad`. Servers, instances and content join
/// this list as they become plugins.
pub fn builtins() -> Vec<Arc<dyn Builtin>> {
    Vec::new()
}
