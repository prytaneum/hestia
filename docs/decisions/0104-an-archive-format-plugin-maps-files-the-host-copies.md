# An archive-format plugin maps files; the host copies them

*Applies to: [Import & export](../architecture/transfer.md), [plugin permissions](../plugins/permissions.md)*

Prism/MultiMC, `.mrpack` and CurseForge archives are modules behind `transfer::Format` ([0057](0057-an-archive-format-is-a-module.md)).
A plugin needs the same seam, but a format reads a zip the user chose and writes into an instance they will run, so the
guest must not hold either.

**The host owns every archive.** The `format` provider kind has four operations (`proto::provider::format`, asked
through `provider.invoke`, [0106](0106-the-plugin-api-is-mechanisms-not-features.md)): `info` (`{ name, kind, marker }`),
`read` (the manifest as UTF-8 text and the member names, answering a blueprint), `land` (the blueprint and the member
names, answering `{ archivePath, entryPath }` copies) and `write` (a blueprint and the instance's paths, answering
manifest bytes and a mapping). A manifest that is not text is refused before the guest is asked. The guest never opens the zip or the instance directory and
never sees a member other than its manifest.

**The blueprint is the core one.** `FormatBlueprint` carries what `Descriptor` and the recipe's JVM settings carry, so
the flow that creates the instance resolves the loader exactly as it does for a built-in format
([0057](0057-an-archive-format-is-a-module.md)) and does not know which kind it was handed.

**Every answer is vetted, and a bad one is refused whole.** Every `archivePath` and `entryPath` must be a relative path
with no parent or root component; an `entryPath` is relative to the instance's game directory (`data/`), never its
entry root, so a format cannot overwrite the instance record ([0018](0018-entry-root-versus-data-dir.md)); and the path that names an archive member must be one the host listed. A refused
`land` fails the import, which removes the instance it had started, so nothing partial is left. The manifest the guest
reads is capped at 8 MiB.

**A format is registered like a source or a flavor.** It is `<plugin>.<id>` in a roster owned by the plugin and removed
with it. Detection ranks it beside the core formats by the depth of its marker; core formats win a tie, and a plugin
cannot claim hestia's own marker, so a native archive is always a native archive.

**The permission is `format:provide`, unrestricted.** A blueprint chooses the game version, loader and JVM arguments of
an instance the user will launch, which is as close to code execution as `flavor:provide`. It needs no network hosts:
the host does the I/O. A format that must resolve content (a CurseForge manifest) uses `host.call`, which is gated by
`content:read` in the manifest like any other call.

Not done: modpack-kind formats, which need the modpack flow to take a plugin reader, and wiring `write` into
`instance.export`, whose `ExportFormat` is a closed wire enum. The calls exist and are vetted so the Prism, `.mrpack`
and CurseForge ports can use them.

Rejected: handing the guest the archive or an extraction directory (a hostile archive and a plugin would share a
filesystem), letting the guest write files itself (the path check would have to be repeated in every plugin), and a
blueprint type of the plugin's own (a second description of an instance to keep in step).
