# An entry's tree has one claim, held for as long as a job or handler changes it

*Applies to: [The daemon](../architecture/daemon.md), amends [0029](0029-one-registrar-per-domain.md) and [0061](0061-a-job-declares-what-differs.md)*

"Is this entry busy" was answered by six per-manager in-flight sets, a table
mapping each side and intent to seven exclusion flags, and a second claim inside
the engine's backup flow. The check and the claim were two steps over different
sets, so a backup and a content job for the same server could both pass their
guards and both start; and the scheduler's backup bypassed the sets entirely.

**One map on the daemon `Runtime` (`runtime/claims.rs`) holds it:** entry key to
the `JobKind` that holds it. `claim(key, kind)` either returns a guard released
on drop or the kind of the job already there, from which `guards::busy` derives
the typed refusal (`BackupInProgress`, `ContentInProgress`, `UpdateInProgress`,
`ExportInProgress`, `Provisioning`, `EntryRunning`).

- A handler that changes an entry takes the claim in the guard call
  (`server_held`, `instance_held`, `entry_held` with a `Hold`) and moves it into
  the job it starts, so check-and-claim is one lock acquisition. A synchronous
  mutation holds it for the length of the handler. A start or restart holds a
  `Launch` claim across the spawn.
- A handler that only needs nothing to be rewriting the tree (`Intent::Idle`: a
  file edit, an instance launch) checks without claiming; `Intent::Read` checks
  nothing. An instance launch stays check-only because several sessions run at
  once.
- A server create has no entry id yet, so it holds a name-keyed claim; guards
  refuse an entry whose name is being created.
- The scheduler takes the same claim before a scheduled task or backup, so a
  scheduled run is visible to every handler. The engine has no claim of its own:
  the pre-update backup runs inside the update job, which already holds the
  entry, so a claim in the engine would refuse its own backup.

**Accepted as stricter:** no server start during an update, content or modpack
job; no server restart during a content or modpack job; no instance launch
during a content or modpack job.

`Runner` and the per-family managers became one `Jobs` value (`managers/job.rs`)
with a `start_*` function per family in that family's file; `Spec` carries the
held claim instead of a key.
