use std::sync::Arc;

/// What a built-in is, known before it runs.
#[derive(Debug, Clone, Copy)]
pub struct Descriptor {
    pub id: &'static str,
    pub name: &'static str,
    /// The WIT package it owns.
    pub provides: Option<&'static str>,
    /// The built-ins that must be enabled first.
    pub requires: &'static [&'static str],
}

/// The plugin has running jobs or processes, so it cannot be turned off.
#[derive(Debug)]
pub struct Busy;

/// Hestia's own native code, registered through the same registry as any plugin.
///
/// The registry holds its lock while it calls `enable` and `disable`, so neither
/// may reach `Engine::plugins()`. Releasing the lock would need an intermediate
/// "enabling" state to keep concurrent calls consistent; a callback that only
/// starts or stops its own work does not need that.
pub trait Builtin: Send + Sync {
    fn descriptor(&self) -> Descriptor;
    fn enable(&self) -> anyhow::Result<()>;
    fn disable(&self) -> Result<(), Busy>;
}

/// The built-ins compiled into `hestiad`. Servers, instances and content join
/// this list as they become plugins.
pub fn all() -> Vec<Arc<dyn Builtin>> {
    Vec::new()
}
