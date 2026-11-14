# An endpoint is scoped exactly as the data home behind it

*Applies to: [The socket boundary](../architecture/wire.md)*

A portable build resolves its data home inside its own layout (`<root>/data`)
rather than `~/.hestia`, but every build resolved the *same* endpoint —
`$XDG_RUNTIME_DIR/hestia/hestiad.sock`, `\\.\pipe\hestia-hestiad`. So a portable
copy unpacked beside an installed one never started its own daemon: it connected
to the installed one already holding that socket and drove *its* data home.
The install that carries its own data was, from the user's side, indistinguishable
from the one that does not — the single thing it exists to guarantee.

The endpoint now takes its name from `common::paths::install_scope`, the same
anchor the data home is resolved from: a build carrying its own `data/` (the
portable archives, every debug build) appends a tag for that directory, and a
build using the per-user platform directory appends nothing. Two front-ends
agree on an endpoint if and only if they agree on a data home, which is the
invariant that was missing. The tag is a hash, not the path — a unix socket path
is capped near 108 bytes.

Rejected: a compile-time `portable` suffix. It needs no path logic, but it only
separates *portable from installed* — two unpacked copies of the archive would
still share one daemon while resolving different data homes, leaving the same
bug one step further out. Rejected too: having each front-end export
`HESTIA_SOCK` at startup, which leaks into every process the daemon spawns.

This puts a `ipc → common` arrow in the graph. `ipc` stays free of anything
launcher-specific; it reads one function that answers "which install is this",
and `common` depends on no workspace crate, so nothing about the layering
changes. `scripts/run.sh` sets `HESTIA_SOCK` only for `--release`, which is not
a contained build and would otherwise land on the session's endpoint; a debug
run relies on the scope, so the script's shell, its desktop and a bare
`target/debug/hestia` all reach one daemon.
