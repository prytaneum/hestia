# A Bedrock console is a reattachable stdin endpoint

*Applies to: [The engine](../architecture/engine.md), [Servers & instances](../architecture/entries.md); see also [0055](0055-the-console-is-rcon-not-a-pipe.md)*

BDS has no RCON: commands go in on stdin and replies come out on stdout.
[0055](0055-the-console-is-rcon-not-a-pipe.md) rejected a stdin pipe because it
exists only between a parent and the child it spawned, which a daemon restart
ends — and [0034](0034-workloads-outlive-the-daemon.md) says a server must
outlive the daemon. RCON stays for Java. Bedrock needs a stdin that a later
daemon can reach.

**The supervisor gives a process a stdin endpoint it keeps for its whole life,
and the record stores where it is.** `process/stdin/` is one interface with a
platform implementation each:

- **Unix: a FIFO** in the process directory, opened read-write by the child, so
  it never sees end-of-file while the daemon is away. A daemon opens it for
  writing on demand.
- **Windows: a relay.** A named pipe dies with the process that created it, so
  the daemon spawns `hestiad stdin-relay --pipe <name> -- <program> …`. The
  relay parents the real program, owns its stdin and serves the pipe the daemon
  reconnects to. It is a `hestiad` subcommand, not a new binary.

An adopted process reattaches from its record after a daemon restart, exactly as
log tailing does. A line is one plain line of at most 1 KiB.

**A reply is read from the captured log, not from the pipe.** Stdin has no
request/reply framing, so the console records the log's length, writes the line
and returns what lands after it once the log has been quiet for 250 ms, capped
at 2 s and 64 KiB. Replies to `list` and `save query` are complete well inside
that.

**A graceful stop writes a line.** `ProcessSpec.stop` is `Signal` or
`Line(String)`; Bedrock uses `Line("stop")` because BDS saves its world on
`stop`, then falls back to the existing signal-then-kill path. A signal alone
was seen to exit without saving.

**Rejected.** A stdin held by the daemon, which loses the console at every
restart and breaks 0034. Killing BDS with the daemon. Parsing per-command
completion markers, which BDS does not emit consistently. A second console
API: the pipe answers the same `command` and `usage` calls, so players, backups,
tasks and the console page work unchanged.

**Not yet verified.** The Windows relay compiles behind `#[cfg(windows)]` and
has not been run on a Windows machine.
