# A session routes events to its listeners

*Applies to: [The socket boundary](../architecture/wire.md)*

A `Session` holds a list of listeners, each an optional process scope and a
channel. The reader task sends an event to every listener whose scope is `None`
or covers the event's payload id, using the same `process_in_scope` predicate
the daemon's event hub filters with. `run_job`, `Process::subscribe` and the
desktop bridge each take a listener, so event-driven calls on one connection no
longer exclude each other.

A listener unregisters when it drops, which also covers a `run_job` future that
is dropped mid-flight; closing the connection clears the list, so every
receiver ends.

Rejected: one callback slot. It forced drivers to serialize event-driven calls
(a violation silently dropped events) and made the desktop demultiplex every
event in TypeScript. Rejected: a map keyed by exact id, because an entry-scoped
subscription must also hear its session keys (`instance-<id>_<seq>`), and
`process.metrics` events carry no id at all.
