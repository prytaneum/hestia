# Following logs is scoped to the entry, not to one run of it

*Applies to: [The engine](../architecture/engine.md)*

`logs -f` used to resolve a *live process* first (erroring "not running" when
there was none) and key the whole stream to it, so a stop ended the session and
a restart needed a fresh invocation — the opposite of what file-backed output
buys us. The subject is now the server or instance itself: the supervisor's keys
are deterministic (`server-<id>`, `instance-<id>_<seq>`), so a front-end names
an entry's process family from the entry id alone, running or not. Three pieces
make that expressible. (1) The key vocabulary moved to `proto::naming` beside
`reference_matches` — a front-end derives the same keys the daemon does, through
the one no-drift seam. (2) An event subscription filter now covers the *session
keys beneath* an entry key (`naming::process_in_scope`), which is what lets one
subscription follow an instance across launches; job ids carry no `_`, so a job
filter still matches exactly one job. (3) The client stream carries
`process.started` as well as output and exit, so a follower can tell a restart
from silence. A follow therefore starts against a stopped entry (backfill from
the file, then wait), renders a state line where a run ends or begins, and keeps
the same stream — in the CLI's one plain `tail -f` follower and the desktop's
log panels alike. Two deliberate exceptions stay process-scoped,
because there the process *is* the subject: the attach that follows
`play`/`launch`, and the rcon console (which can only drive a live server
anyway). The rejected alternative was a reconnect loop in each front-end — it
re-derives lifecycle logic per client and still drops the lines either side of
the gap.
