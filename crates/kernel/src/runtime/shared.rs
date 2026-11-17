use std::collections::HashSet;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, OnceLock};

use super::services::Services;
use super::settings::Files;
use super::wakeups::Wakeups;
use crate::api::HostCalls;
use crate::grant::Reserved;

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
    /// Hestia's own endpoints, which no connection may reach however it is spelled.
    pub own: Arc<OnceLock<Reserved>>,
    pub wakeups: Arc<Wakeups>,
    /// Developer mode, which every plugin log of the sandbox reads.
    pub developer: Arc<AtomicBool>,
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
            own: Arc::default(),
            wakeups: Arc::default(),
            developer: Arc::default(),
            #[cfg(feature = "probe")]
            events: Arc::default(),
        }
    }
}
