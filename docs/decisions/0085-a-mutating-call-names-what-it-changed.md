# A mutating call names the state it changes, and the registrar publishes it on one topic

*Applies to: [The socket boundary](../architecture/wire.md#jobs-events-and-cancellation), [Front-ends](../architecture/frontends.md#the-frontend)*

The desktop keeps its caches fresh for changes other clients make by listening
to daemon topics: process lifecycle and the single `job` topic. A mutation that
is a plain call publishes nothing, so `hestia instance <name> remove` left the
removed instance in an open desktop's library until the cache went stale. The
same held for every non-job mutation a window renders: entry create, remove,
rename, settings, content enable/remove, backups, tasks, player lists, accounts,
skins, Java, sync, screenshots and the cache.

**A contract declares what it changes.** `Contract::CHANGES` is an
`Option<proto::change::Scope>`, `None` for a read and for a job (whose events
already say). A mutating contract sets it beside its `CHANNEL`. `Channels::handle`
— the one place every handler passes through — publishes a single
`changed { scope }` event after the handler succeeds, so no handler announces
anything and a failed call announces nothing.

**The scope is coarse.** `Entries` (servers and instances with everything kept
beside them), `Accounts`, `Skins`, `Config`, `Java`, `Sync`, `Screenshots`,
`Cache`. The event names no entry: contracts reach an entry by name or id, a
remove has no entry left to name, and a reader that is told "entries changed"
refetches what it has open. The desktop maps a scope to the key prefixes it
sweeps, as it already does for an entry-wide job. The event carries no `id`, so
only unfiltered subscribers receive it.

The window that made the call hears its own event and refetches what its mutation
already invalidated; TanStack deduplicates the in-flight fetch.

Not covered: `server.files.*` (a live directory view, refetched by its own
mutations), process start/stop (already `process.*` events), job-shaped work
(the `job` topic), and reads.

Rejected:

- **Publishing from the engine's entry store** (`registry::Store`
  register/rename/remove). It would carry a stable id, but it covers only the
  entry record: content, backups, tasks, players, accounts, skins, config and the
  rest would each need their own seam, and the engine would need an event sink
  it deliberately does not have (it does not know a socket exists).
- **A publish call in each handler.** A checklist that a new handler forgets,
  which is how the gap arose.
- **Deriving the scope from the channel name** (`*.remove`, `*.set`). Magic that
  misclassifies (`server.players.set`, `skin.equip`) and cannot say which scope.
- **A topic per domain** (`instance.removed`, `skin.changed`, …). The zoo
  [0083](0083-a-job-is-its-contract-and-reports-on-one-topic.md) removed.
- **Carrying the entry id and the kind of change.** Needs the registrar to
  resolve a reference before and after the handler, for a refetch that is cheap
  and already scoped to what is open.
