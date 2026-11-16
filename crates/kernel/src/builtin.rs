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
/// The registry holds its lock while it calls `enable` and `disable`, and the
/// router reads that lock on every call to a plugin-owned channel. Both must
/// return promptly (register or unregister, never wait on work) and must not
/// reach the registry.
pub trait Builtin: Send + Sync {
    fn descriptor(&self) -> Descriptor;

    /// The interfaces of the package it provides and the permission each
    /// needs. The registry adds it to the kernel's, so a plugin can import them.
    fn table(&self) -> Option<crate::grant::Table> {
        None
    }
    fn enable(&self) -> anyhow::Result<()>;
    fn disable(&self) -> Result<(), Busy>;
}
