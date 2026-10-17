# A job declares what differs; the runner owns the rest

*Applies to: [The daemon](../architecture/daemon.md)*

Ten job start sites — backup, restore, content, modpack, server create, server
update, instance launch, export, import, java install, download, self-update —
each spelled out the same skeleton: generate or take the job id, claim the
entry key, clone the engine/hub/cancellations, spawn, build a progress closure,
register a cancel token, match the result three ways, publish, log. Around
sixty lines apiece, differing in a handful of type names.

The cost was not the duplication. It was that a fix to the skeleton had to be
applied ten times, and was not: `coalesce_progress` — written because
per-file progress froze the desktop ([0056](0056-job-progress-paints-once-per-frame.md))
— had reached six sites. A backup, an export, a download and the self-update
still published an event per file or per chunk. The self-update had drifted
further: it had no `cancelled` topic, so cancelling one reported a failure.

**A job now declares only what differs.** `Spec` names its id, its prefix and
the claim it holds ([0086](0086-an-entry-has-one-claim.md)); the work is one
closure taking the engine and a `Reporter` and returning the start contract's
`Done`. `Jobs::run` (formerly `Runner::start`) owns everything else
— including building every event, since the contract fixes their shape
([0083](0083-a-job-is-its-contract-and-reports-on-one-topic.md)) — so
coalescing, cancellation, terminal classification and logging happen in exactly
one place and cannot be forgotten by the next job.

Two things fell out of it. Cancellation is classified from the error chain by
the runner, so the instance launch's own `LaunchFailure` enum — a second
implementation of "is this a cancel or a failure" — is gone. And a job kind that
admits any number of concurrent jobs says `claim: None` rather than claiming a
freshly generated id, which is what the modpack and transfer managers were
doing to mean the same thing.

The rejected alternative was a trait with a method per hook. It reads the same
at the call site and buys nothing: there is one implementation of the lifecycle
and there always will be, so the seam it would create is hypothetical. A struct
of what varies is the honest shape.
