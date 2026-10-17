# A degraded outcome rides on the result, never only in the log

*Applies to: [The socket boundary](../architecture/wire.md)*

Several steps in this codebase are deliberately best-effort — the properties
schema run, each `sync` target's reconcile — because failing the whole operation
over them would be worse than proceeding. But "proceed" was implemented as a
`tracing::warn!` and an unqualified success to the caller, so the user learned
nothing: a 1.6.4 server was created with no validatable property schema and said
only "created", and an instance whose `saves` could not be linked launched and
played against the *wrong world* with the only trace in the daemon's log. The
daemon's log is not where a user finds out what just happened to their data.

So a degraded outcome is part of the operation's **result**:
`proto::warning::WarningInfo` is a structured, exhaustive enum — the exact shape
and discipline as `ErrorInfo`, no prose authored at a call site — carried on the
job done events (`server.create`, `server.update`,
`instance.launch`) and on the standing views that stay true afterwards
(`ServerDetails`, so `server info` keeps saying it long after the create
scrolled past). `Sync::apply` therefore *returns* its warnings instead of
logging them, naming the unit it left the instance to settle on its own. Every
variant carries a `hint()` beside its `Display`
headline: a warning the user cannot act on is noise, so the remediation is part
of the type rather than something each front-end invents. Front-ends get it for
free and localize it generically — the CLI prints a `warning:` line plus the
hint (`View::Warning`), the desktop renders `warning.kind.*` / `warning.hint.*`
message keys as a toast on the operation and a standing `WarningNotice` on the
entry.

The rejected alternatives were raising the log level to WARN (same invisible
place) and hard-failing the operation (refusing to launch over a file sync
cannot read, or refusing to create a server whose schema run timed out — both
break a recoverable situation). A front-end must not have to *ask* whether the thing it
just did worked properly.
