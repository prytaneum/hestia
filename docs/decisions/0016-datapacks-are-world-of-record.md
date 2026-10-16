# Datapacks are managed like every other kind, mirrored per world

*Applies to: [Content & modpacks](../architecture/content.md)*

A datapack loads from inside a world (`data/<level-name>/datapacks/` for a
server, `data/saves/<world>/datapacks/` for an instance), and this record first
argued it should live only there, with the world backup as its record. That
design could not hold three cases, and commit `43d3f81f` replaced it:

- an instance with no world yet could not take a datapack at all;
- a world created later never received the packs installed before it;
- restoring an older server backup swaps the world, and with it any datapack
  installed since. Mods and plugins survive a restore because the backup
  preserves them from the live tree; a datapack is inside the archive.

So a datapack has a managed copy in `<entry>/datapacks/<file>` and
`data/<world>/datapacks/` is its mirror, reconciled by the launch-time sync like
every other kind ([0013](0013-managed-dir-of-record.md)). One index entry per
pack carries the worlds it loads in; an empty selection means every world the
entry has, now or later. A server has one world (`level-name`, read from
`server.properties`); an instance has many, so an install names one or more
(repeatable `--world`, or an interactive multi-select over `instance.worlds`).
`remove --world` and `disable --world` narrow the selection rather than address
separate records, and disabling drops that world's mirror. Untracked datapacks
are reported per world.

The client-side support flag is waived for datapacks: they run on a world's
server side, including a client's integrated server, so a source marking a
datapack client-unsupported must not block installing it on an instance.

With `saves/` linked (linked sync), an instance's datapack mirror lives in the
*shared* world every instance opens, while its `content.json` provenance stays in
the installing instance, so other instances list it as untracked world data. The
world carries its own datapacks, and exactly one instance manages each.
