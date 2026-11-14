# A pack is one provider kind that replaces `format`, and export formats are an open set

*Applies to: [Import & export](../architecture/transfer.md#pack-formats-from-plugins), [Plugins](../architecture/plugins.md#providers), [Content & modpacks](../architecture/content.md#modpacks), [provider operations](../plugins/providers.md#pack-formats)*

> Superseded in part: the `.mrpack` reader and writer left core too, so core reads and writes only Hestia's own archive. `.mrpack` (the `mrpack` plugin), CurseForge packs and Prism / MultiMC instances are the official plugins' pack formats, `hestia` is the one reserved id, and `instance.export.formats` lists hestia plus the registered writers. The `pack` kind, the vetting and the refuse-whole rule below stand.

Three things stood between the CurseForge pack and a plugin. The `format` provider kind ([0104](0104-an-archive-format-plugin-maps-files-the-host-copies.md))
was instance-only and carried one manifest text, so it could not say "these are the files, here is the tree that
overlays them"; `ExportFormat` was a closed wire enum, so a plugin's writer could never be reached from
`instance.export`; and core's `content::pack` named CurseForge by literal — its `manifest.json` marker, its
`manifestType` probe, its single named overrides tree and its source id.

**One kind, `pack`, replaces `format`.** A pack is an archive that carries a game version, a loader and files: a Modrinth
or CurseForge pack, or another launcher's instance (a Prism instance is a pack whose files are all in one override
tree). `proto::provider::pack` has four operations, asked through `provider.invoke`
([0106](0106-the-plugin-api-is-mechanisms-not-features.md)):

- `info` — `{ name, members, extension }`. `members` are the archive members it reads, as names or globs whose `*` stands
  for a run of characters inside one path segment (`*.cfg`, `.minecraft/*.json`). `extension` is non-empty only for a
  format that can write.
- `claims` — `{ files }` to `{ claimed }`, optional (unsupported claims). The host first finds the shallowest directory
  holding a member for every declared pattern, so a pack nested in a folder is found and one buried deeper never
  outranks it; the probe settles what presence cannot (any zip may carry a `manifest.json`).
- `read` — the member names and the *text of the declared members only*, answering the pack (`name`, `gameVersion`,
  `loader`, `files`), its override trees per side (`shared`, `client`, `server`, in write order) and, optionally,
  `resolve { source, manifest }`: a manifest a content source's `resolve-manifest`
  ([0122](0122-a-source-declares-its-setting-and-one-vet-checks-every-download.md)) turns into the files. The pack names
  itself and its game version and loader; the source supplies the files and fills only what the pack left blank.
- `write` — the instance, the game directory's files and the content pool (each item with the path it lives at and
  whether a manifest can refer to it), answering manifests to store, `{ entryPath, archivePath }` copies and warnings.

**The host fetches only what the single vet allows.** Every file a read names goes through `Policy::vet_artifact` — the
rule of 0122, not a second one: https on a granted host and a well-formed checksum, or `unhashed` from a host the grant
lists, or an empty URL for a file its author withholds. Files a source resolves are vetted by that source's own policy.
The files are then *downloaded* under the policy of whichever vetted them (`Archive::policy`), where before a pack's
files were fetched with none. The guest's identity fields are discarded: `source`, `projectId` and `versionId` on the pack
and on each file are the host's, set by the delegation or by recognising the file's URL like any `.mrpack` file.

**A pack is untrusted input and refused whole.** Member patterns must be plain relative paths and may not match hestia's
marker or the `.mrpack` index, so a plugin cannot claim a native archive; at equal depth core formats win. An override
tree must be a plain relative directory (no `.`, `..`, root, drive, backslash or trailing slash), at most eight per side;
each extracted member is still checked against escape before the first write. A path that is absolute, empty or contains
`..` refuses the answer, as does a file list over 50,000, a delegation that also lists files, a pack with no game version,
and a delegation to a source that is not registered. Declared members are capped at 8 MiB each and 32 per archive and must
be UTF-8; a violation is `ArchiveInvalid`. An export's copies may name only paths the export holds, no archive path may be
written twice, manifests are capped at 8 MiB, and warnings are bounded. The reserved ids `hestia` and `mrpack` cannot be
registered. A pack's mods and files are installed by the modpack flow, which creates the entry and records provenance
([0011](0011-modpack-decomposes-into-existing-parts.md)), so a plugin pack never lands files in an entry itself.

**Export formats are an open set, like import.** `ExportFormat` is `Hestia` or `Provider(Provider)`, mirroring
`ImportFormat`. `instance.export.formats` lists what the daemon offers — hestia, the launcher's own `mrpack`, and every
registered pack that can write — with each format's extension, so the CLI and the desktop resolve a name against the
daemon instead of a hard-coded list. An unknown id is `ExportFormatUnknown`. The core `.mrpack` writer stays in core: it
is the pack format the launcher reads itself and is the reference the vocabulary was shaped from. The pool partition
both writers need (`transfer::pool::exported`) is shared.

**Core no longer names CurseForge in a pack.** `content::pack` reads the `.mrpack` index and holds the `Archive` every pack
installs from, whichever format read it; the CurseForge marker, probe and overrides logic leave it. CurseForge becomes a
plugin that declares `manifest.json`, claims the archive when `manifestType` is `minecraftModpack`, answers its name, game
version, loader and one override tree, and delegates its manifest to its own source.

## Not taken

- **Bolting packs onto `format`.** It would keep a second description of an instance (`FormatBlueprint`) beside
  `ResolvedModpack`, and an instance format that lands files itself, which is the step the modpack flow already does with
  provenance. One kind with one detection model is smaller.
- **A `format` kind kept beside `pack`.** Nothing `format` could say a `pack` cannot, and a second kind doubles detection
  and registration. The unreleased plugin API breaks instead ([0091](0091-plugins-are-wasm-components.md)).
- **Handing the guest the archive or an extraction directory.** A hostile archive and a plugin would share a filesystem;
  declared members as text is all a manifest reader needs.
- **Applying a read's JVM settings to an existing instance.** A read may carry `memory` and `jvmArgs`, the two settings
  Prism's importer carried. They are vetted by the rules the `memory` and `jvm-args` settings use (an invalid value is
  dropped with a warning, see the addendum), and they ride on the opened archive to the install, which passes them as
  the `config` of the instance it creates. An install into an existing instance, a pack update and a server install
  ignore them: a pack does not overwrite what the user tuned.

## Addendum, 2026-11-14: five gaps the ported formats exposed

Porting `.mrpack`, Prism and CurseForge onto the kind showed what a format could not say, and each closure is a general
field, not a format's name:

- **`PoolItem` carries `size` and `sha512`** for a referenceable file, hashed by the host in the one read, so a manifest
  that names a file by those can be written without the guest reading the pool.
- **`ClaimQuery` carries the text of the declared members**, under the read path's per-member bound and a total bound,
  so a format can look inside (a CurseForge `manifest.json` against any other) before it claims. A member that is not
  UTF-8 or would pass a bound is left out; the archive is not refused for it, since a claim only decides.
- **`ReadQuery` carries the loader keys** the registered flavors declare for the format's id, so a reader picks the
  component a flavor claims, as the Prism importer did. Keys only: which flavor owns one is the host's, and the loader a
  read answers is mapped to its flavor on inspect as on import.
- **`PackRead` may name an `icon`**, an archive path held to what an imported launcher icon is held to.
- **An invalid `memory`, `jvmArgs` or icon is dropped with a warning** on the result
  ([0026](0026-degraded-outcomes-ride-on-the-result.md)) instead of refusing the read, as the Prism importer did: a flag
  the launcher rejects is not worth losing the instance over. This replaces the whole-read refusal above for those
  three; every path, URL and tree is still refused whole.
