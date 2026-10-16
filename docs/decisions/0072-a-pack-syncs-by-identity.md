# A pack syncs by identity, not by file — and only where it fits

*Applies to: [Instances](../architecture/entries.md#sync--shared-settings-across-instances)*

Resource packs are something people expect to follow them between instances,
and they are not a file two copies of which can be merged. A
pack is an installed *thing*: a project, at a version, enabled or not, in a load
order. So the unit shares that identity and nothing else.

A shared pack is a project id, the source it came from, and its file's hash. A
pass over a participating instance installs a pack it does not have through the
ordinary content pool — the same managed directory, provenance and mirror every
other install uses ([0013](0013-managed-dir-of-record.md)) — and propagates
enable, disable and removal by identity. **A version change never propagates**:
one instance updating a pack is not a statement about the others, which may be on
a game version that release does not support.

**Compatibility is a gate, not a failure.** A pack reaches an instance by being
installed the ordinary way, so the pool's own resolution decides it: the newest
release of that project whose game versions and loaders match the instance. An
instance nothing resolves for is skipped silently — it is not an error, and it
must not be reported as one every launch. The stronger test is the pack's own
`pack_format` in its `pack.mcmeta` against the format the client reads, which is
what a pack installed from a file — with no platform metadata at all — has to be
judged by; that check belongs with the selection, where a pack that resolves but
cannot load must still be kept out of the load order.

**A pack the instance did not choose does not travel.** Content carrying a
modpack origin belongs to that pack's configuration, not to the player's
library, so it is never lifted into the shared selection — the same rule that
keeps a modpack's `options.txt` from redefining everyone's settings.

Load order lives in `options.txt`'s `resourcePacks` key, which is never shared as
a raw string: it names files by path, and a path means nothing on an instance
that does not hold that pack. Enablement travels as part of a pack's identity —
disabling one disables it everywhere — while the order each instance loads them
in stays that instance's, until the selection is reconciled per pack the way the
key would have to be: each shared pack mapped to the path it has locally, with
entries for packs only that instance holds left where they are.

**An instance's agreement records what it holds, not what was offered.** A pack
that did not install here — nothing resolved for it, or the source was
unreachable — stays in the library and out of that instance's agreement, so the
next pass offers it again instead of reading its absence as a removal. A pack the
instance agreed to and no longer has leaves the library without any action on
that instance.

**Datapacks are not synced** ([0016](0016-datapacks-are-world-of-record.md)).
A datapack loads from inside a world, so syncing one cannot mean writing into
every save an instance holds — a world is the player's, and a pack appearing in
one they did not ask for is a change to how that world runs. Sharing only the
library would leave enable and remove between instances that each installed the
pack by hand, so there is no unit until an explicit "apply the library to this
world" action exists. Modrinth ships the equivalent switched off for the same
reason: propagating into existing worlds is the only behaviour that would make
the toggle mean anything, and it is the one that cannot be undone.

**Rejected:** copying pack files between instances directly. It would duplicate
the pool, lose provenance, and leave the copies unupdatable — the content pool
already knows how to install a project by identity. **Also rejected:** syncing
versions along with identity, which would drag every instance onto whichever one
updated last, including instances that cannot run that release.
