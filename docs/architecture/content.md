# Content & modpacks

*[← Architecture](../architecture.md)*

Content is everything you add to an entry that Mojang did not ship: mods,
plugins, resourcepacks, shaders, datapacks — and modpacks, which are all of the
above bundled with a loader and a version.

Three layers, deliberately separate:

```mermaid
flowchart TD
    SRC["<b>sources</b> — content/<br/>search · project · versions · resolve<br/><i>plugin-provided (Modrinth, CurseForge: official plugins); the trait is the seam</i>"]
    INST["<b>install</b> — content/install.rs<br/>managed dir + content.json provenance + data/ mirror"]
    PACK["<b>modpacks</b> — content/pack · modpack<br/>the opened archive, and which files a pack owns"]

    SRC --> INST
    PACK --> INST
```

## Sources

`ContentProvider` is the trait: search with pagination, project detail, version
resolution filtered by loader and game version, modpack resolution, URL
recognition, the kinds a source catalogues, and whether it is configured enough
to serve. Core ships no source: `modrinth` and `curseforge` are official plugins
([0133](../decisions/0133-a-provider-has-one-id.md)), and a
fresh Hestia with none installed has an empty source list. Stateless.

The registry is runtime-mutable: `Content::register(owner, standing, provider)`
adds a source and `unregister(owner)` removes everything that owner added. A
source's id always reads `<owner>.<id>`; a bare name (`modrinth`) is resolved by
the roster ([0133](../decisions/0133-a-provider-has-one-id.md)), which also stores and
reports the full id (`modrinth.modrinth`) of the source that served.

With no source registered every content channel still answers: `content.sources`
is empty, and a search, lookup or install that names no source fails
`NoContentSource` (one that names an id nobody registered fails
`ContentSourceUnknown`). The CLI says no source is installed and the desktop's
browse page shows an empty state pointing at the Plugins page.

A plugin adds a source with `contributes.providers = [{ kind = "content", id }]`
and answers the operations of `proto::provider::content` through `provider.invoke`
(the SDK's `Plugin::provide`, routing each with `.on::<Op>(…)`). The manifest must also hold `content:read`,
`network:connect` and at least one network host, or it does not validate. When
the daemon enables the plugin it asks each contributed source to describe itself
and registers a `PluginContentProvider` as `<plugin>.<id>`; disabling, removing
or replacing the plugin unregisters it. The adapter calls the guest with a 30 s
deadline (`PROVIDER_TIMEOUT`), stamps its own id on everything returned, and maps
a guest failure to `PluginCallFailed` naming the plugin, never a raw message. The
guest only describes files; the engine downloads them, and the adapter drops any
version file whose URL is not https on one of the plugin's granted hosts, using
the same host matching as `wasi:http`. The `static-index` example in
[prytaneum/plugin-examples](https://github.com/prytaneum/plugin-examples/tree/main/plugins/static-index) is a
source that serves an index compiled into the component. Content already installed from a
source that is gone keeps its record, still lists and still loads;
`content.list` names it in `unavailableSources` (from
`Content::missing_sources`), the CLI marks its rows `unavailable`, the desktop
badges them and drops their project link and version actions, and a request
through the source is refused as unknown.

Every platform response is mapped into the normalized `proto::content` types at
this boundary, so a front-end never sees a platform's raw shape
([0010](../decisions/0010-one-content-provider-trait.md)).

A provider also recognises its own site's project and version page URLs, so a
pasted project link installs exactly like a slug. A source that has no datapack
project type says so with `datapacks_as_mods`, and a datapack request then accepts
its `Mod` projects.

> **A source is only offered when it can serve.** An API key makes
> *availability* a property of a source rather than a constant (the CurseForge
> plugin declares one). The trait has `available()`: `content.sources` lists the
> sources that can serve and those waiting on a plugin setting (with
> `requiresSetting` naming the plugin and key), and `content.*` refuses a
> named-but-unconfigured one with `ContentSourceUnavailable` (carrying the same
> `RequiredSetting`, so the desktop links to where it is edited and the CLI says
> how to set it, rather than the "unknown source" a filtered registry would have
> to claim). A secret is never logged.
>
> Two shapes follow from several sources. A `ContentSource` now carries the `kinds` it
> catalogues, because "which sources can I browse mods on" is a daemon fact and a
> front-end must not keep its own table (the same no-drift rule `accepts` follows
> on the entry views). And a `ContentAddItem` may name its **own** source,
> overriding the batch's: with two sources a user's selection is naturally mixed,
> and a spec-wide source would have forced the front-end to split a batch it has
> no other reason to split — a URL item already resolved its own source this way.

> **A blocked file fails its item, and nothing else.** A platform may let an
> author opt out of third-party distribution; the source then lists the file with
> no download URL. The `ContentDownloadBlocked` refusal says so and names the source,
> the rest of the batch installs, and local-file import is the documented way
> through — Hestia deliberately does not reconstruct a CDN path to download it
> anyway. The check lives in the one download step every flow shares
> (`flows/download.rs`) rather than the provider, because an artifact with no URL
> is unusable whoever produced it — and a blocked file is routine in a pack: a
> pack listing one still installs, minus that file, naming it.

## What an entry accepts

Not a table anywhere — composed from two independent facts: what the **flavor's**
loader consumes, and what the **side** reads for itself.

| | mods | plugins | resourcepacks | shaders | datapacks |
|---|:--:|:--:|:--:|:--:|:--:|
| server on a modloader flavor | ✓ | | | | ✓ |
| server on a plugin-platform flavor | | ✓ | | | ✓ |
| vanilla server | | | | | ✓ |
| instance on a modloader | ✓ | | ✓ | ✓ | ✓ |
| vanilla instance | | | ✓ | ✓ | ✓ |

A refusal carries the accepted set rather than a sentence, and `ServerInfo` /
`InstanceInfo` carry `accepts` on the wire so no front-end keeps its own copy
([0008](../decisions/0008-flavor-declares-accepted-content.md)).

## The install model

Content is written to a **managed directory in the entry root** — Hestia's
namespace — with its provenance in `content.json`, then hardlinked (or copied)
into the game directory.

```mermaid
flowchart LR
    SRCF["source / URL / local file"] --> MANAGED["&lt;entry&gt;/mods/sodium.jar<br/><b>source of truth</b>"]
    MANAGED --> IDX["content.json<br/>kind · source · project · version · sha1 · enabled · origin"]
    MANAGED -->|"hardlink, else copy"| MIRROR["data/mods/sodium.jar<br/><i>what the game loads</i>"]
    MIRROR -.->|"backup restore swaps data/"| GONE["mirror lost"]
    GONE -.->|"reconcile at next start"| MIRROR
```

The managed copy — not the one in `data/` — is the source of truth, which pays
off three ways:

1. **Restore heals itself.** A backup restore swaps `data/`, but the managed dirs
   live outside it and are on the backup exclude list, so a reconcile pass
   re-mirrors them at the next start. Archives stay world-focused. Server mods
   and plugins also survive a restore through the backup's preserve list, which
   carries them over from the live tree; datapacks, which live inside the world
   archive, rely on the mirror.
2. **Provenance survives**, so `update` knows each item's project and current
   version.
3. **A hand-dropped jar in `data/` is surfaced as untracked**, never silently
   adopted. A file placed by hand in the managed directory itself is neither
   indexed nor reported: that directory is Hestia's namespace.

([0013](../decisions/0013-managed-dir-of-record.md))

Managed directory names are the game's own load-dir names (`mods/`, `plugins/`,
`resourcepacks/`, `shaderpacks/`), so the mirror is symmetric.

Every write to the index goes through one `Pool`, opened after the downloads
finish: `upsert` for an install or a pack item, `replace` for an update, a source
move or an update from a file. Downloads share one batched step
(`download_batch`) and one version lookup (`target_version`). The pool saves the
index first and deletes the files it superseded after, so a crash leaves an
orphan file rather than an index naming one that is gone.

### Resolution and dependencies

A platform install picks the newest compatible version — filtered by the entry's
game version and, for mods, its loader — and resolves required dependencies
breadth-first. A direct URL or local file import records `source: "file"` with no
version to update against: no source updates it, and a newer build is supplied
as a file instead ([0079](../decisions/0079-a-local-item-updates-from-a-file.md)).

### Who owns which field of a record

An `InstalledContent` mixes three independently-owned groups, and no flow owns
all of them: an install has a project and a version but no prior record, an
update has a record and a version but no project, a modpack re-supply has both
over an item the entry is already holding a particular way.

| Group | Fields | Who writes it |
|---|---|---|
| `Project` | id, slug, title, icon | the install; carried unchanged by everything after |
| `Release` | source, version, filename, sha1, url | an install, an update, a re-supply |
| `Holding` | origin, `enabled`, per-world disables, world targeting | the entry alone — never anything upstream |

`content::record::assemble` is the only place those fields are written, so a
flow cannot reset a group it does not own: an update is `repin(item, release)`
and a modpack re-supply is `rehold(item, holding)`. The assembler is exhaustive,
so a new field on the record does not compile until it has been classified
([0064](../decisions/0064-a-record-is-mutated-not-rebuilt.md)).

This matters beyond bookkeeping: the launch-time mirror reads `enabled` and the
per-world disables straight off the record, so a group lost in a rebuild does
not merely misreport an item — it puts a disabled one back where the game loads
it.

### Beyond add and remove

| Operation | Behaviour |
|---|---|
| `enable` | flips a flag in `content.json`. Enforced at the single point it needs — the launch-time mirror keeps a disabled item out of `data/`, so it can never be resurrected by a restore. A datapack's mirror is per world, so disabling drops the mirror in the chosen worlds |
| `check_updates` | a *separate* on-demand call, not baked into `list`, so `list` stays fast and offline |
| `update` / `remove` | take a *batch* of references — one job, one call. Named items are resolved before the first one moves, so a batch naming something that does not exist changes nothing; an empty batch means every item of the kind (`update`) and is refused (`remove`). Past that an update is per-item like an install: every item resolves, the moves download side by side under one progress extent, and an item that cannot move comes back as a failure while every one that did is recorded |
| `update` with a `path` | moves the one named *local* item onto that file, keeping its enabled flag and worlds; a tracked or pack-owned item is refused. The old mirrors go first, so a rebuild under the same name is what loads next ([0079](../decisions/0079-a-local-item-updates-from-a-file.md)) |
| `set_version` | re-pins one item to a chosen published version — the update path with an explicit pin instead of "newest". With a `source`, the version comes from that source instead: the installed file's hash finds the project there (`matches` lists which sources recognise it), and the record moves to it. A local file adopts a source the same way ([0075](../decisions/0075-a-local-file-is-identified-by-its-hash.md)) |

All of them refuse a running or busy entry: open jars lock on Windows, and
changes only take effect at the next start anyway
([0014](../decisions/0014-enable-update-check-and-pin.md)).

### Local files are inspected, not trusted

`content.inspect(path)` reads the archive's central directory and classifies it,
returning the detected kind, validity and a reason. Detection is
**loader-agnostic** — a mod is any loader's manifest (`fabric.mod.json`,
`quilt.mod.json`, `META-INF/mods.toml`, `neoforge.mods.toml`, …) — and a datapack
is told from a resourcepack by the `data/` vs `assets/` tree under a shared
`pack.mcmeta`.

The detected kind is a **suggestion, not a verdict**: `content.add` hard-rejects
only what genuinely cannot be single-file content (an unreadable archive, or a
modpack) and otherwise honours the requested kind, so a review-step override
installs where you asked
([0015](../decisions/0015-local-imports-are-inspected.md)).

A local file is also **identified**. `ContentProvider::identify` hands each
source the files and gets back the version each one is — however that platform
recognises a file — and a recognised file is recorded as that
release rather than as source `file`, so it updates and exports like anything
installed from a source. The bytes installed are still the local ones, its
dependencies are not resolved, and a lookup that fails (offline, unknown file)
leaves the import untracked rather than failing it. Every source is asked, and
the add item's `source` says which match to track (`file` for none);
`content.inspect` runs the same lookup, which is how the review step lists the
matches to choose from before install
([0075](../decisions/0075-a-local-file-is-identified-by-its-hash.md)).

A file left untracked has no project to borrow an icon from, so `content::icon`
reads the one the archive carries: the `icon` a `fabric.mod.json` or
`quilt.mod.json` names, the `logoFile` of a `mods.toml`, `neoforge.mods.toml` or
`mcmod.info`, and otherwise a root `pack.png` — which covers resource packs,
datapacks and the shader packs that ship one. The lookup is the same for every
kind; a plugin's `plugin.yml` has no icon field, so a plugin has one only when
its jar happens to carry one of those. The image is decoded under size limits,
shrunk to a 64px PNG and stored on the record as a `data:` URI, so it travels in
`content.json`, over the socket and inside an exported archive with no second
file to keep in step. The same read runs wherever an untracked record is made: a
local import, a modpack's override jars, and an imported instance's loose files.

A shader pack and a plugin carry no icon at all, so an import kept untracked is
still looked up by hash, and where the archive has no icon of its own the record
borrows the one of the project a source knows it as. Only the icon is borrowed:
the item stays source `file`, with no project id and nothing to update against.
A file no source recognises and that embeds nothing keeps the placeholder.

## Datapacks are the exception

A datapack loads from *inside a world*, not a flat directory, so its mirror is
per world. It still has a managed copy, `<entry>/datapacks/<file>`, like every
other kind: an instance with no world yet can take one, a world created later
receives the packs installed before it, and a server backup restore — which swaps
the world, and with it any datapack installed since — is healed by the launch-time
sync. Mods and plugins survive a restore through the backup's preserve list; a
datapack, being inside the archive, relies on the mirror.

| | Server | Instance |
|---|---|---|
| Mirrored to | `data/<level-name>/datapacks/` | `data/saves/<world>/datapacks/` |
| Which world | the one, from `server.properties` | named — repeatable `--world`, or an interactive multi-select; none named means every world, now or later |

One index entry per pack carries the worlds it loads in, so a removal clears
every mirror unless narrowed. The client-side support flag is waived for
datapacks: a datapack runs on a world's server side, including a client's
integrated server ([0016](../decisions/0016-datapacks-are-world-of-record.md)).

## Modpacks

A pack installs into a new or existing server or instance from a project, a page
URL, or a local pack file. `pack.rs` owns the opened `Archive` (its root and which
override trees to extract), deliberately apart from the platform that serves the
pack and from the format that read it — a pack picked off disk has no source and
is read the same way. The zip itself is
read by the one reader import and export share (`transfer/archive.rs`), which
validates every path before the first write and hashes only when asked; the
install flow receives the archive already open, whether it came from a download
or from `instance import`.

**A pack archive is read by a plugin.** The launcher reads no pack format itself:
every format is a plugin's `pack` provider ([0125](../decisions/0125-a-pack-is-a-provider-kind-and-export-formats-are-open.md)), which hands the
`Archive` the install flow extracts from the override trees it names, so the flow
extracts any of them without being told where the archive came from. A pack that
names its files by id rather than URL delegates its manifest to the content
source that owns it (`ContentProvider::resolve_manifest`), so it installs whenever
that source can serve and is refused as `ContentSourceUnavailable` — naming the
setting to set — when it cannot. The files are fetched under the policy of whoever
vetted them: the plugin's grant for a file it names, the source's for a delegated one.

**A pack from a source goes through the downloader.** The version the flow picks
already carries the archive's artifact, so it is fetched like any other file: into
a uniquely named file under the data home's `tmp/`, with progress, cancellation
and the download cache. The archive is read from there and the file is removed at
once; `reclaim_temp()` empties the directory at start. A zip with no pack manifest
is refused as `NotAModpack`, and an artifact with no URL as
`ContentDownloadBlocked`.

A pack decomposes into three things Hestia already has, and each goes where it
belongs:

```mermaid
flowchart TD
    PACK["a pack archive"]
    PACK --> A["loader + game version"]
    PACK --> B["files under a managed load dir<br/><i>from the index or an override tree</i>"]
    PACK --> C["the rest of overrides/ and everything else"]

    A --> A1["become the entry's flavor and version<br/><i>create --modpack and modpack install<br/>are one code path</i>"]
    B --> B1["ordinary pool items tagged modpack:&lt;project&gt;<br/><i>individually listable, enableable, updatable</i>"]
    C --> C1["written into data/, recorded in modpack.json<br/>with the sha1 each was written with"]
```

Only the third needed anything new, and for one reason: those files are configs
and keymaps **you edit**. So an update replaces a file the pack still owns and
leaves a tweaked one exactly as found, reporting which through
`WarningInfo::ModpackOverridesKept`. A *first* install writes over whatever is on
disk — nothing there is yours yet
([0011](../decisions/0011-modpack-decomposes-into-existing-parts.md)).

**Taking a pack out is two different things**, so `modpack.remove` is told
which. The default deletes the pack's pool items and the override files it still
owns. With `keepContent` only the pack goes: `modpack.json` is cleared and its
pool items lose their origin tag, so they become removable and syncable like
anything the user installed, and the override files simply stop being recorded.

**Whether there is an update is asked, not assumed.**
`modpack.check_update` resolves the pack's newest published version
through the same pick an unpinned update makes, and answers with both version
numbers plus `updatable` — so what a front-end offers and what an update would
install cannot disagree. A pack read off disk is first asked after by hash: an
archive a source published is recorded as that release, and updates like one
installed from the source. One no source recognises has no catalogue behind it
and answers with nothing to check, the same reason `fetch_update` refuses it.

**A version change moves the pack.** `server.update` and `instance.update` take a
pack policy (`move` by default, or `keep`). With `move` the daemon asks the same
`entry_modpack_update` for the pack version built for the target game version and
runs one job: a pack update when there is one, the plain version change plus a
`PackNotMoved` warning when the pack has no build for the target
([0088](../decisions/0088-the-daemon-sequences-a-version-change.md)). The
`gameVersion` of `modpack.check_update` stays as the preview a front-end shows
before the request.

**A pack file updates any pack.** `modpack.update` takes a `path` in place
of a version, and the file goes through the same update: the game-version move
behind the downgrade gate, the stale-item sweep, the keep-if-edited rule. It is
the only way an unpublished pack moves, and a published one may take it too — a
file no source recognises then leaves it unpublished. A pack file carries a name
and a free-text version but no id, so whether it is *the same pack* is the
project where a source recognises both and the name otherwise; a different one is
refused as `ModpackDifferentPack` unless `allowOtherPack` says it is meant.
`modpack.preview_update` answers both questions — same pack, downgrade —
before a job starts, so a front-end asks once instead of failing a job to find
out ([0078](../decisions/0078-a-pack-updates-from-a-file.md)).

**A resolved pack file carries its identity.** A pack that names its files by id
delegates to the source that owns them, and resolving already fetches the file and
project records, so every `ModpackFile` leaves the resolve with its source, ids,
version number and project. A pack that names files by URL and hash alone has
each file URL put to every source (`ContentProvider::recognise_file_url`), and the
source that answers is recorded. `hydrate` then asks only for what is still
missing (a file's version number and project), in one bulk call per source.
Best-effort: a pack still installs when the catalogue is unreachable, it just
reads less well.

What the pack does not identify — a jar shipped in an override tree, a file
hosted where no source recognises the URL — is asked after
by hash once it is on disk, the same lookup a local-file import makes; where
several sources publish it, the pack's own wins. A jar in `overrides/mods/` is
therefore a pool item like any other, in the managed directory rather than
written straight into `data/`. A jar kept as `<name>.jar.disabled` there is the same
item, recorded disabled and kept out of `data/` until it is enabled. Only a file no source recognises records as
`source: "file"`: installable, just not updatable
([0076](../decisions/0076-a-pack-file-is-identified-wherever-it-came-from.md)).

**A pack's `env.server` is a claim, not a fact.** Packs routinely declare
client-only mods as server-required, so a *server* install filters them using
`itzg/docker-minecraft-server`'s list, adopted verbatim. What is held back rides
on the result as `WarningInfo::ModpackFilesExcluded`, naming each file. The
correction is server-side only — applied to an instance it would strip out
precisely the mods the pack was installed for
([0012](../decisions/0012-pack-server-declarations-are-corrected.md)).

Where a pack's platform publishes no per-project side support, the correction is
not a refinement but the *only* signal: every required file reads as wanted by
both sides and the exclude table is what tells a server's mods from a client's.
The table's per-pack rules are keyed by the pack's source project slug, so a pack
from another platform matches the global rules but not its per-pack entry, and
`modpack.exclude-files` is the way to correct one.

**What a pack cannot do:** be installed into an entry whose flavor or game
version differs from what it pins, or pin a loader with no Hestia flavor. An
update *does* carry the game version with it — that is what updating a pack means
— behind the same explicit downgrade gate as any version move.

## Decisions

- [0010 — Content is normalized behind one trait](../decisions/0010-one-content-provider-trait.md)
- [0011 — A modpack is three things at once](../decisions/0011-modpack-decomposes-into-existing-parts.md)
- [0012 — A pack's `env.server` is a claim, not a fact](../decisions/0012-pack-server-declarations-are-corrected.md)
- [0013 — Installed content is managed-dir-of-record, mirrored into `data/`](../decisions/0013-managed-dir-of-record.md)
- [0014 — Enable/disable, update-check and pin extend the same model](../decisions/0014-enable-update-check-and-pin.md)
- [0015 — A local-file import is inspected, not trusted](../decisions/0015-local-imports-are-inspected.md)
- [0016 — Datapacks are managed like every other kind, mirrored per world](../decisions/0016-datapacks-are-world-of-record.md)
- [0075 — A local file is identified by its hash](../decisions/0075-a-local-file-is-identified-by-its-hash.md)
- [0076 — A pack file is identified wherever it came from](../decisions/0076-a-pack-file-is-identified-wherever-it-came-from.md)
- [0078 — A pack updates from a file, and leaves an entry two ways](../decisions/0078-a-pack-updates-from-a-file.md)
