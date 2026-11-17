# Built-in plugins are native code in the same registry

*Applies to: [The plugin kernel](../architecture/plugins.md#the-model)*

**Decision.** A built-in plugin is Rust compiled into `hestiad` and registered through the same registry, API packages
and extension points as a WASM plugin. It needs no grant and no prompt, can be disabled unless an enabled plugin needs
it, and cannot be removed. A disabled built-in's channels answer `plugin_disabled`.

**Why.** A feature is something you plug in, and a plugin that extends servers depends on the servers plugin. The vendor
runs its own code through the public extension model, and no process supervisor, RCON or archive primitive enters the
third-party ABI, so the sandbox promise is unchanged.

**Rejected.** A privileged WASM plugin signed by Hestia's key with signature-only interfaces: the dangerous primitives
would exist in the ABI behind one key check, and large backups would run through WASM. A monolithic core that plugins
decorate: "plugin" would only be a name. Built-ins that are always on: nothing could be turned off.
