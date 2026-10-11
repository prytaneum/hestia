# A managed document carries its own schema version, and an unreadable one is set aside

*Applies to: [The engine](../architecture/engine.md)*

Every store in the data home decoded its file the same way — `serde_json::from_str(&text).unwrap_or_default()`, or
`.ok()?` inside a directory scan. That is fine while one build writes and reads
the file, and silently destructive the moment two do. A `config.json` a newer
hestia had written came back as `Settings::default()` and was saved over on the
next `config set`; a `server.json` that failed to decode made the server vanish
from `server list`, with the record still on disk and nothing said. There was no
version to key on, so there was also no way to *add* one later without the same
failure mode eating the attempt.

So a user-owned document now carries a top-level `schemaVersion`, and reading
one goes through `engine::schema`. Three things fall out of it.

**The version is derived from the migration chain, not declared beside it.** A
`Document` names itself and lists its `MIGRATIONS`; `version()` is
`1 + MIGRATIONS.len()`. Adding a migration is appending one function, and the
constant that could disagree with the list does not exist. A step rewrites
`serde_json::Value`, never the Rust type: the struct only ever describes the
newest schema, so a step that deserialized would need rewriting every time the
struct moved, and the chain would stop being a record of the shapes that
actually existed. A file with no stamp is not corrupt — it is the first schema —
so it enters at the baseline and every step applies.

**Per document, not per data home.** A single version file at the root would
speak for files it never saw written, and the interesting cases are exactly
those: a `server.json` restored from a backup, an entry directory copied between
machines, the record travelling inside an exported archive. Only a
self-describing document answers them. It also lets each document move on its
own cadence, which is the normal case — the skin library has no reason to bump
when a server record does.

**An unreadable document is renamed aside, not treated as absent.** Absence and
"cannot read this" produced the same `None` before, and the caller's next write
landed on top. Now the file becomes `<name>.unreadable-<stamp>` and the store
starts from defaults: the daemon keeps running, and nothing the user had is
destroyed. Failing the whole daemon instead was rejected for the reason
[0026](0026-degraded-outcomes-ride-on-the-result.md) rejects hard-failing a
degraded operation — one stale file would brick a launcher that is otherwise
completely usable. A rename that itself fails is only possible in an unwritable
directory, where the write being guarded against cannot happen either.

Quarantines do not belong to the request that happened to trigger one — a
`server.list` does not cause it — so they cannot ride out on that result the way
an ordinary degraded outcome does. They accumulate in one process-wide sink and
are read back where the user is already looking: `daemon.status` carries what
this daemon has set aside, and an import carries the ones under the entry it just
landed. Threading a diagnostics handle through every store constructor was the
alternative, and it costs more than it explains for something loaded from free
functions several layers below the aggregate.

Writes go through a temp file renamed into place, which the codebase already
required of downloads, backups and installs but not of its own records. For
`accounts.json` the owner-only mode is set on the temp file *before* the rename,
so the tokens are never briefly world-readable — which the previous
write-then-chmod could not promise.

## What is not versioned

Derived state: process records and tombstones, installed
Java runtime records, the download cache. A document nothing is lost by
discarding needs deleting and regenerating, not a migration path, and that is
already what happens when one fails to read. Desktop preferences stay out too —
they are schema-less by design and the front-end owns their keys
([0049](0049-desktop-prefs-live-in-the-data-home.md)).

## Archives

Both archive formats hestia writes are documents under the same rule. The
`.hestia` manifest's hand-rolled `formatVersion` became the shared
`schemaVersion`, and it carries the instance record **stamped** rather than
inlined as a typed field, so an archive migrates its record through exactly the
chain a record on disk would. The documents an import lands are then brought
forward before anything reads them, and whatever had to be quarantined rides out
on the import result.

A backup archive had no marker and no version at all — a restore untarred
whatever it found — so it gained `hestia.backup.json` as its first entry, checked
before any of the archive reaches the staging tree. A backup carries only the
server's `data/`, which holds no managed document, so there is nothing in one to
migrate; the manifest exists so a restore of an archive from a newer build is
refused by name rather than scattered into the game directory.

An archive is not ours to rename aside, so unlike a file in the data home a
schema failure there is refused: `ArchiveTooNew` for one from the future,
`ArchiveInvalid` for anything else.
