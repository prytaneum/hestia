# The kernel's runtime sits behind a `sandbox` feature, and the CLI links the kernel without it

*Applies to: [The plugin kernel](../architecture/plugins.md#the-kernel-crate)*

**Decision.** The registry, wasmtime and the network policy are behind the kernel's default `sandbox` feature. `cli`
depends on the kernel with `default-features = false` and gets only what reads a package: the manifest parse, the
component shape, grant derivation, the version gates and the `.hplugin` and index formats. `hestia plugin check`, `pack`
and `publish` call these directly and need no daemon.

**Why.** The author tooling must apply the host's own rules, and must work with no daemon running. Linking the kernel
without its runtime keeps wasmtime out of `hestia`. Cargo unifies features within one invocation, so `cli` is built in
its own (`scripts/sidecars.sh`); a build that includes the daemon brings the sandbox back.

**Rejected.** Routing the tooling through the daemon: checking a file would need a running daemon. A second
implementation of the rules in `cli`: it would drift from what the host enforces.
