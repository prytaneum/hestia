# The plugin kernel is its own crate, below the engine

*Applies to: [The plugin kernel](../architecture/plugins.md#the-kernel-crate)*

**Decision.** The registry, `Builtin`, the sandbox, the WIT host and its adapters, the grant, dependencies, trust and
network vetting live in `crates/kernel`, which depends on `proto` and `common` only. `engine` depends on it (built-ins
implement `Builtin`, core downloads use vetting) and `daemon` on both. A plugin's call reaches the launcher through the
`HostCalls` trait, which the daemon implements by routing through its own router.

**Why.** The compiler then enforces the boundary the way it already does for `engine`: no plugin call can reach launcher
logic except through the router, where entry guards, scopes and the call chain apply. wasmtime compiles in one crate.

**Rejected.** A module inside `engine`: the sandbox would sit beside launcher logic and the boundary would be a
convention. A kernel that depends on `engine`: it could call launcher code directly, skipping the router's gates.
