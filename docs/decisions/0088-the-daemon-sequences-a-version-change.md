# The daemon sequences a version change, its restart and its modpack

*Applies to: [Servers & instances](../architecture/entries.md#version-updates), [Content & modpacks](../architecture/content.md)*

Changing an entry's game version used to be a recipe each front-end ran for
itself. The CLI stopped a running server, polled `server.status` until it
reported stopped, ran the update, and started the server again; the desktop
stopped an instance session and polled `instance.list` before relaunching it.
Whether a modpack moved with the version was two reads (`modpack.status`, then
`modpack.check_update` for the target) and a choice between two jobs, made in
the CLI's `pack_move` and again in the desktop's version dialog, with their own
defaults and messages. A client killed between the stop and the start left the
server down.

**A server update carries `restart`.** The job stops the server when it runs,
waits for the process to exit (`ProcessSupervisor::stop_and_wait`, the same wait
a `server.restart` makes), updates, and starts it again whichever way the update
ended. A start that fails is a `ServerNotRestarted` warning on the result
([0026](0026-degraded-outcomes-ride-on-the-result.md)); a stop that outlasts the
wait abandons the update. The job holds the entry's `Update` claim
([0086](0086-an-entry-has-one-claim.md)), taken live when `restart` is set —
the job stops the server itself — and stopped otherwise, where a running server
is refused, as before.

**An instance launch carries `replace`.** `All` or one session id: the launch job
stops those sessions and waits them out before it prepares the game. The account
check runs first, so an expired sign-in fails before anything is stopped. A
restart is therefore one launch job, with its progress and cancellation, not a
`stop`, a poll and a `launch`. It stays check-only like any launch: the new
session is not an exclusive holder of the entry.

**A version change carries a pack policy, `move` (default) or `keep`.** The
daemon resolves the pack version built for the target game version
(`modpack.check_update(gameVersion)` is that same computation) and runs one job:
a pack update when the entry runs a source-published pack with a build for the
target, the plain version change otherwise. A pack with no build for the target
stays put, with a `PackNotMoved` warning. `instance.update` became a job for this,
since moving a pack downloads its content; it holds the instance's `Update` claim
for as long as it runs. `check_update` keeps its
`gameVersion` as the read-only preview the version dialog shows before submit; it
no longer drives a choice.

Rejected: leaving the sequence in each client and sharing a helper. Every new
front-end would write it a third time, and a client that dies mid-sequence still
strands the entry. An `instance.restart` request/response beside `server.restart`
was also rejected: an instance start is a job that needs an account and reports
progress, which a plain call cannot.
