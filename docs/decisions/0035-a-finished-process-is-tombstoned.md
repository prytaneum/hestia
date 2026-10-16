# A finished process is labelled, not merely unrecorded

*Applies to: [The engine](../architecture/engine.md)*

Two promises here could not both hold: "a terminal state keeps its logs for
post-mortem" and "a startup sweep deletes recordless dirs". A finished process
leaves *exactly* a recordless dir — the record was dropped and the logs left —
so the sweep destroyed the post-mortem at the next restart, and the guarantee
survived only until then. The observable symptom was the odd one: a killed
process lingered in `process.list` (in memory only) until an unrelated daemon
restart, and that same restart deleted its logs. There was no state in which the
list was clean *and* the logs existed.

The bug was the sweep's criterion. "Has no `record.json`" was standing in for
"is a stray", but it is also the normal resting state of a finished process. So
the end is now **explicit on disk**, the disk-is-the-registry discipline used
for java runtimes, backups and server records: on exit the process's
`record.json` is rewritten, through a temp file renamed into place, with an
`ended` section (state, exit code, when it ended) and keeps its spec, which is
where the log paths are derived from. The sweep then deletes only directories
with **no readable record** — a true stray, hand-made, half-written or
unparseable — and `FINISHED_KEEP` prunes the oldest finished ones, because "keep
the logs" without stated retention means "grow forever" (count-based, as
`backup prune` is).

An earlier shape wrote a second file (`exit.json`, a tombstone) beside a live
record that held the launch token unmasked, on the grounds that an `on-failure`
restart relaunched from it. That restart policy was never set by anything
shipped, and a restart used the in-memory spec anyway, so the token was on disk
for no reader. Both went: the record is written with its args masked, the
restart policy and its respawn branch are gone, and one file serves both ends of
the process's life. If automatic restart is wanted later it belongs in the
server start path as an explicit per-server setting, not a generic spawn flag.

`process.list`/`status`/`logs` read terminal entries from the records rather
than from memory, so what the daemon reports about a finished process no longer
depends on whether it has restarted since — including its logs, which is the
whole reason for keeping the directory. A process that died while unsupervised
is ended by `recover()` at the moment it is noticed. The rejected alternatives
were exempting process dirs from the sweep (they then accumulate forever, which
is what the sweep exists to prevent) and adding a `process.clean` verb (asking
the user to resolve a contradiction the daemon should not have).
