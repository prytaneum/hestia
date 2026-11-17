use std::collections::HashSet;
use std::sync::{Arc, Mutex, OnceLock};

use super::services::Services;
use super::settings::Files;
use super::wakeups::Wakeups;
use crate::api::HostCalls;

/// What every call of a sandbox shares: the host's services and the state that
/// outlives a call.
#[derive(Clone)]
pub(super) struct Shared {
    pub services: Arc<dyn Services>,
    /// The named locks held now, as (plugin, name).
    pub locks: Arc<Mutex<HashSet<(String, String)>>>,
    pub files: Arc<Files>,
    /// Plugins whose `/data` directory exists, so a call does not create it again.
    pub mounted: Arc<Mutex<HashSet<String>>>,
    /// The router the adapters call through; set once the daemon has one.
    pub calls: Arc<OnceLock<Arc<dyn HostCalls>>>,
    pub wakeups: Arc<Wakeups>,
    #[cfg(feature = "probe")]
    pub events: Arc<Wakeups>,
}

impl Shared {
    pub fn new(services: Arc<dyn Services>) -> Self {
        Shared {
            services,
            locks: Arc::default(),
            files: Arc::default(),
            mounted: Arc::default(),
            calls: Arc::default(),
            wakeups: Arc::default(),
            #[cfg(feature = "probe")]
            events: Arc::default(),
        }
    }
}
