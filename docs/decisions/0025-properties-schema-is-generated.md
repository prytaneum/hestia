# The properties schema is generated, not maintained — and it is not the file

*Applies to: [Servers & instances](../architecture/entries.md)*

`config set` validates a `server.properties` key against a *schema* derived from
the server binary itself, never against a curated key list. A hand-kept list is
a per-version maintenance liability (keys appear, disappear, and differ across
the versions Hestia launches; the list would silently rot). So the create job
runs the freshly downloaded server once in a **throwaway directory**
(`<entry>/.schema/`, discarded after): with no `eula.txt` there the gate makes
it emit a complete `server.properties` (every key + default for exactly that
version, mods included) and exit almost immediately, before binding ports or
generating a world. That pristine file is stored beside the record as
`schema.properties`, and its keys are what a `config set` is checked against;
the value is written into the game's live `data/server.properties`, which is
seeded with any schema key it lacks. Pre-1.7.10 servers have no EULA gate and
would boot for real, so the run is killed after a 60 s timeout. A version update
reruns it the same way and replaces the schema.

The run is deliberately *outside* `data/`, because the two things it used to
conflate are different: the schema is "the keys this version knows", the file is
"the values this server holds". Running in `data/` meant the server
round-tripped the existing file, so what came back was the current values, not a
key set — and **vanilla preserves keys it does not recognise** (verified against
1.21.1: an unknown key seeded before the run survived it). A key retired by a
version update therefore stayed in the file forever and, because the file was
the validation source, stayed settable forever. Now it stays in the file (it is
a value, and silently deleting lines the user or a mod may own is worse than the
drift) but is no longer in the schema, so it reads and lists while refusing
further writes. Deriving the schema separately also makes the no-schema fallback
explicit rather than accidental: schema generation is best-effort — a failure is
a *warning*, not a create failure — and a server with no `schema.properties`
accepts any unmanaged key rather than rejecting every one. `Servers::has_schema`
is that state, so a caller can report it rather than leaving the user to
discover that this server validates nothing.

**Amended by [0090](0090-config-values-pass-one-typed-schema.md), for values only.** Which keys exist is still decided by this schema; what a *value* may be comes from a daemon-owned table of known properties (type, range, allowed words), and a key the table does not know takes any string.
