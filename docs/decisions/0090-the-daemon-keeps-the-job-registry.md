# The daemon keeps the job registry

*Applies to: [The socket boundary](../architecture/wire.md), [The daemon](../architecture/daemon.md)*

Which jobs are running lived only in the desktop's job store, built from events
it happened to see. A reloaded window, or a job started from the CLI, was
invisible to it, and an event carried no hint of what the job was about, so a
client could not tie it to an entry.

**`Jobs::run` records a `JobInfo` while a job runs**: its id, channel, subject,
start time and last progress. `job.list` returns the running set, and every
`JobEvent` carries the `subject`. Any client then sees the same jobs, which is
the same "one core, many faces" the rest of the daemon follows.

**Rejected:** persisting jobs to disk, because a job dies with its daemon and
the right answer after a restart is an interrupted state, not a resurrected
job; and a client-side cache across reloads, because it cannot see jobs started
by the CLI.
