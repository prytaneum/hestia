# Plugins spawn only the programs they declare

*Applies to: [The engine](../architecture/engine.md), [plugin permissions](../plugins/permissions.md)*

Tunnelling services (playit.gg, Tailscale) work through a native agent, and the v1 sandbox gives a plugin neither
processes nor local sockets. `process:spawn` lets a plugin run native code, so it is an unrestricted permission and the
install states it as full access to the computer. What follows narrows what that access can reach.

**Every program is declared up front.** `[[programs]]` names an `id`, a `command` and the `args` patterns it may be
called with: a literal token matches itself, `*` any one token, and a trailing `**` the rest, Zed's `process:exec`
model. The `env` it may be given is a list of names; a call passing any other is refused, and names that load code or
redirect lookups (`LD_*`, `DYLD_*`, `PATH`, `*JAVA*OPTIONS`) are rejected at install, so no declaration unlocks them.
All of it is part of the grant, so widening it on an update asks again.

**A `PATH` program is pinned at approval.** Its name is resolved to an absolute path when the grant is approved; that
path is shown at install, stored on the record, and is the only one that program ever runs from. A later `PATH` change
or a binary planted earlier on `PATH` cannot substitute another. If the file disappears the call fails until the plugin
is approved again.

**A data-directory program is copied before it is checked.** A `command` with a `/` is a file inside the plugin's
`data/` and declares a `sha256` per target triple. The host copies it into `plugins/<id>/bin/`, which the guest cannot
reach, hashes the copy and runs only the copy, so the guest cannot swap the file between check and exec, and rewriting
`data/` afterwards changes nothing. The host sets the exec bit; the guest needs no chmod.

**There is no shell.** Arguments are passed as given, the working directory is `data/`, and the environment starts
empty plus the declared names. A shell (`sh`, `cmd`, `powershell`, …) cannot be declared as a program. Program ids and
exit statuses are logged; arguments and values never are, because they carry auth keys.

**Spawned agents are supervised and owned.** `spawn` runs under the process supervisor with the owner `plugin:<id>`:
its output is captured, it is listed by `process.list` without its arguments, and its id persists, so `list()` finds it
again after the daemon restarts. A plugin can only see, stop or read the logs of its own processes; any other id reads
as absent. Like every workload it outlives the daemon ([0034](0034-workloads-outlive-the-daemon.md)), but **disabling or
removing the plugin stops its processes**: the user turned the owner off, and an agent left running would hold a tunnel
open with nothing to manage it.

**Local sockets stay closed.** The Tailscale CLI and the playit agent cover both services, so `wasi:sockets` is not
opened for this; it is revisited only when a real plugin needs a socket.

Rejected: any program by name (a grant could not say what it allows), a shell with a command string (one injection away
from anything), trusting the `PATH` at call time, hashing a file the guest can still write, and stopping a plugin's
processes when the daemon exits (it would contradict 0034 for one kind of workload).

**Amended 2026-11-10 (13.09): a program may take as long as its call, and is a tree.** `exec` is no longer capped at five
minutes: its ceiling is what the current call has left (a flavor `build` or a plugin command has an hour, any other call
its own shorter deadline), with `timeout-ms` still applying below it. It observes the call's cancel and, on cancel,
timeout or drop, kills the child's whole process tree through the engine's tree termination (a process group on Unix, a
kill-on-close job object on Windows), never only the direct child. `exec` also takes `env`, checked against the declared
`env` exactly as `spawn` is. `PATH` still cannot be passed; instead a program may declare `path = ["git"]`, naming other
`PATH` programs of the same manifest, and the child's `PATH` is built solely from their pinned directories (so the grant
shows what it can reach, and `spawn` gets the same). An `exec` argument may contain `${build}`, `${vanilla}` or `${data}`,
replaced after the pattern check by the host path of that mount of the call, so a grant stays host-independent; a mount
the call lacks is refused. A flavor build also holds an exclusive file lock on `meta/flavors/<id>/.lock`
([0110](0110-a-flavor-builds-in-its-own-area.md)).

Rejected: a larger fixed cap (a BuildTools run has no honest bound), signalling only the child (a grandchild keeps
running and holds the build directory), and letting a plugin pass `PATH` (it would reach any binary on the machine).
