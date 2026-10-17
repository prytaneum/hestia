# A job is cancelled by asking, at safe checkpoints — never by disconnecting

*Applies to: [The socket boundary](../architecture/wire.md)*

Ctrl-C killed the CLI while the daemon ran the job to completion: a JDK landing
minutes after the user stopped waiting, with no way to abort a download, an
assets materialize or a backup from any front-end. The tempting fix — have the
daemon cancel a job when its requesting client disconnects — is exactly the
coupling the supervisor design removed, and would kill a legitimate background
install the moment a terminal closed. So cancellation is an **explicit act**,
like stopping a workload: one `job.cancel { id }` channel, keyed by the job id
the job's own events already carry, so a front-end cancels the run it started
whatever kind it was. The CLI turns a terminal interrupt into that request
(`commands::cancellable`), which is the only reason Ctrl-C now stops anything.

Inside, cancellation is **cooperative and checkpointed**, never a kill:
`engine::Cancel` is a flag, and `engine::Job` carries it alongside the progress
reporter — the two travel to the same places because a step that reports
progress is exactly a step that can be stopped between reports. The checkpoints
are the boundaries the staging discipline already created: per chunk in a
download, per file in a library/asset batch and in a backup archive, and between
pipeline phases. Stopping at one leaves precisely what a network failure at the
same point would have left, so the existing failure paths do the cleanup — a
cancelled Java install stages and never renames, a cancelled create discards its
record exactly as a failed one does, a cancelled backup leaves a `.part` that
`Engine::recover()` reclaims. Nothing new had to learn how to tidy up.

A cancelled job is **not** an error. It settles on its own `cancelled` state of
the `job` topic ([0083](0083-a-job-is-its-contract-and-reports-on-one-topic.md)),
surfaces as `IpcError::Cancelled` / `JobCancelled` rather
than a daemon error, and is logged at info. A front-end that rendered
cancellation as a failure would be blaming the user for what they asked for.
