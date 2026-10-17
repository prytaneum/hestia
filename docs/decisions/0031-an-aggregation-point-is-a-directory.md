# An aggregation point is a directory, not a file

*Applies to: [The daemon](../architecture/daemon.md)*

Four places in this codebase exist to gather every domain in one spot — the
engine aggregate, the client's facades, the daemon's router, the daemon's job
managers — and each grew linearly with the feature count until it was the
largest file in its crate. The convention that caused it ("wire-in is one line,
in one place") is right; the mistake was reading "one place" as "one file". Each
is now a module directory where the aggregating seam stays thin (`make_router()`
is a list of `register()`s; `Engine` is fields and getters) and every domain has
its own file. Nothing about the crate graph, the wire, or the call sites changed
— `Engine`'s flows are still `engine.provision_server(…)`, because Rust lets an
inherent `impl` span modules within a crate. Splitting also surfaced the real
duplication each file had been hiding: seven copies of a lock-insert-remove
in-flight set became one `InFlight`/`Claim` guard (since one entry-keyed map,
[0086](0086-an-entry-has-one-claim.md)), and four copies of a
progress-decode closure became one `forward()`.
