//! The machine the daemon runs on.

use proto::host::{HostInfo, HostInfoResult};
use proto::Empty;

use crate::runtime::Channels;

pub(super) fn register(on: &mut Channels<'_>) {
    on.handle::<HostInfo, _, _>(|_: Empty, _ctx| async move {
        Ok(HostInfoResult {
            total_memory_bytes: engine::host::total_memory(),
        })
    });
}
