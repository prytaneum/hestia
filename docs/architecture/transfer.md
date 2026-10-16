# Import & export

An instance moves in and out of the launcher as **one file**. This is what
instances have instead of backups, and the shape is different on purpose: a
server is infrastructure that must be recoverable *in place*, so it is archived
on a schedule under its own entry root; an instance is something you play,
share, and carry between machines, so its archive is one portable file you write
deliberately.

Four formats come in, two go out.

| Format | Marker | In | Out | What it is |
|---|---|---|---|---|
| **hestia** | `hestia.instance.json` | ✅ | ✅ | the entry directory minus content a source can serve again, + the instance's record — the profile is resolved on the importing host |
| **`.mrpack`** | `modrinth.index.json` | ✅ | ✅ | Modrinth's pack format: mods as *references*, everything else in `overrides/` |
| **Prism / MultiMC** | `instance.cfg` | ✅ | — | another launcher's instance directory (`mmc-pack.json` + `.minecraft/`) |
| **CurseForge pack** | `manifest.json` | ✅ | — | CurseForge's pack format: mods as project/file *ids*, which need the CurseForge source to resolve |

## Detection, not declaration

`hestia instance import <file>` takes a file and nothing else. Every one of these
formats is a zip and people rename them, so the format is decided by the **marker
file inside**, following Prism's own approach — someone handed an archive should
not have to know which launcher made it.

The **shallowest** marker wins, and the order hestia, pack, Prism only breaks a
tie. Depth has to come first: a pack index a Prism instance happens to ship
inside its `config/` is a file that instance *uses*, not what the archive *is*.

One marker is not specific enough to settle it by presence: any archive may
carry a `manifest.json`. So a CurseForge candidate only counts when the manifest
is read and its `manifestType` is `minecraftModpack`. Prism itself matches that
name alone; the content check is the stricter reading of the same idea
([0076](../decisions/0076-a-pack-file-is-identified-wherever-it-came-from.md)).

The two pack formats are not instance formats: whether a zip is a modpack, in
which format and at what root, is `content::pack`'s one answer, and detection
ranks that single candidate beside hestia and Prism. A pack is then handed,
already open, to the modpack flow.

## The seam

```mermaid
flowchart TD
    FILE([an archive]) --> DETECT{detect by marker}
    DETECT --> FMT["<b>Format</b><br/>marker · read · land"]
    DETECT --> PACK["<b>content::pack</b><br/>.mrpack · CurseForge"]
    FMT --> BP["<b>Blueprint</b><br/>descriptor + recipe"]
    BP --> RR["Recipe<br/><i>a loader to look up, and the identity<br/>some formats carry</i>"]
    RR --> CREATE[["the flow registers the instance"]]
    PACK --> MODPACK[["modpack install → creates its own entry"]]
    CREATE --> LAND["<b>Format::land</b><br/>files onto disk"]

    classDef seam fill:#eef,stroke:#88a
    class FMT,BP seam
```

An instance format answers three questions and nothing else: **which archives
are mine** (a marker), **what does this one say it is** (parse a manifest into a
[`Blueprint`]), and **where do the files go** (given an instance that now
exists). It never creates the instance — that is the launcher's job, and the
blueprint's `Recipe` carries what the launcher needs to build it: one resolve path
for every format, with the instance's identity riding along when the archive has
one. Another format costs the flow nothing.

### Adding a format

One module under `crates/engine/src/transfer/` and one line in `detect`
(`transfer/mod.rs`) — the same shape as adding a content platform
([0010](../decisions/0010-one-content-provider-trait.md)). An ATLauncher importer
would implement `Format`, return a `Recipe`, and reuse `transfer::pool::adopt`
for the loose jars its game directory arrives with.

## What an archive carries

The exclusion rules are shared by every exporter
(`transfer/exclude.rs`) — "which files *are* the instance" is one question with
one answer, and two writers deciding it separately is how a format quietly ships
somebody's crash reports.

Left out, following Prism's own export defaults plus two of hestia's own:

| Skipped | Why |
|---|---|
| `logs/`, `data/logs`, `data/crash-reports` | regenerated, and large |
| `data/.cache`, `data/.fabric`, `data/.quilt` | loader caches, rebuilt on launch |
| `.DS_Store`, `thumbs.db`, `session.lock` | OS turds and a transient world lock |
| `*.part`, `.discard-*` | something was mid-write |
| `instance.json` | the record travels in the manifest instead |
| the `data/` **mirror** of a pool item | the managed copy is the record; the mirror is re-made at every launch ([0013](../decisions/0013-managed-dir-of-record.md)) |

**Saves are in.** An instance without its worlds is not the instance. A caller
can still leave any path out — `--exclude data/saves`, or unchecking it in the
desktop's tree — and `instance.export.contents` answers that tree, derived from
the *same file plan the export writes* so the two cannot disagree.

Symlinks are **followed**, unlike the server backup's tar: `data/saves` is
routinely a link into the shared `sync` store, and an archive carrying the link
rather than the worlds would have no game data in it.

## The hestia archive

The entry directory minus whatever a source can serve again, plus a manifest that
is also the marker:

```
cozy.hestia (a zip)
├── hestia.instance.json    the manifest: format version, who wrote it, the record
├── content.json            the pool index — provenance survives the trip
├── modpack.json            the pack the instance runs, when it runs one
├── mods/ resourcepacks/ shaderpacks/ datapacks/   local-only files
└── data/                   the game directory: config, options.txt, saves
```

The archive carries **what the instance is, not how a host runs it**. The record's
flavor, game version and loader version are authoritative; its profile (client
jar, libraries, natives, JVM arguments) was resolved for the exporting OS, so the
manifest is written without it and import resolves it on the importing host,
keeping the record's name, Java settings, sharing, playtime and timestamps. That
needs the version catalogue,
which falls back to the cached one offline. The **id is not honoured**: an
imported instance is a new entry and gets a fresh one, so the same archive can be
imported twice.

Content is **referenced, not carried**, when it can be: an item with a URL and a
SHA-1 (`install::referenceable`, the rule the `.mrpack` export applies too) keeps
its record in `content.json` and leaves its bytes out. A local file, or one a
source does not distribute, is embedded. Import then makes the list true
(`flows/restore.rs`): an item whose file is missing is downloaded to its managed
path through the content-addressed cache, verified against its SHA-1, and a
failure unwinds the import so it can be retried. An item that is neither present
nor referenceable is dropped from the index and named in `ImportContentMissing`.

The export picker lists every file, referenced ones included, flagged
`referenced` and counted as nothing in the total. **An exclusion removes the item,
not just its bytes**: `content.json` is rewritten through the same rules as the
files, so an import never lists what the archive does not have
([0081](../decisions/0081-the-archive-carries-identity-and-references.md)).

The marker is deliberately not called `instance.json` — a marker is a claim about
what an archive *is*, so it has to be specific enough that finding it inside
someone else's zip means something.

## The `.mrpack` export

A lossy projection, by nature: a pack names its mods by URL and hash, so only
pool items hestia knows the origin of survive as references. A local-file import
has no URL to write and rides along inside the archive instead, with a
`ExportFilesEmbedded` warning saying so — the export installs correctly either
way, it is just no longer a pack Modrinth would accept for publishing.

Each referenced file carries `sha1` (what the pool stores) **and** `sha512`
(what the format wants), hashed at export. A disabled item is exported as
`optional` rather than dropped, so whoever installs the pack gets the same
choice rather than a silently shorter mod list.

## Importing another launcher's instance

A Prism/MultiMC instance names a game version and a loader in `mmc-pack.json`
rather than resolving them, so the import resolves them exactly as
`instance create` does — and an archive pinning a loader hestia has no flavor for
is refused **by name** (`ArchiveUnsupported`), never silently installed as
vanilla. `instance.cfg` contributes the display name and, where that instance
overrode its launcher's defaults, its memory and JVM arguments.

Its `.minecraft/` becomes `data/`, and the loose jars in it are **adopted into
the content pool** (`transfer/pool.rs`): moved to the managed directory, indexed,
and mirrored back. They arrive with no provenance, so the flow then asks the
sources about them by hash — the lookup a local-file import makes — and records
each recognised jar as that release. `ImportFilesUntracked` names only what no
source publishes, the files that can never be updated. A
`.disabled` suffix becomes hestia's `enabled` flag, which is the same fact
spelled the way this launcher spells it.

Only an archive the kind loads is adopted — a `.jar` in `mods/`, a `.zip` or
`.jar` in `resourcepacks/` and `shaderpacks/`, whatever its case. A load
directory also holds what mods write beside a pack (Iris's `<pack>.zip.txt`
options, a Respackopts `<pack>.zip.rpo`, a patcher's log); those stay in `data/`
unindexed, as a folder pack does.

## An import is a create

It registers a new instance and then fills it, so it owns the discipline every
other create has: **an import that fails partway removes the entry it started**.
A registered instance whose files never landed is worse than no instance — it
lists, it cannot launch, and nobody asked for it.

An export is guarded like a backup: the instance must be stopped and racing no
content, pack or transfer job, since all of those write the tree it reads. While
one runs, the things that would rewrite that tree underneath it — a launch's
content mirror, a content install, a rename, a remove — are refused. An import
guards nothing, because the entry it would conflict with is the one it creates.

## Where an archive goes

`instance.export` takes an absolute destination: a file, a directory (a name is
generated inside it), or empty for `<data_home>/exports/`. **Relative paths are
refused** — the daemon is a separate process and does not share a client's
working directory, so a relative path would mean something different on each
side of the socket. The CLI resolves against its own cwd before sending.

The desktop registers `.hestia` with the OS, so a double-clicked archive reaches
the app — as a launch argument, or through single-instance when a window is
already up — and opens the import dialog on it.

## Related decisions

- [0057 — An archive format is a module, not a branch](../decisions/0057-an-archive-format-is-a-module.md)
- [0081 — The archive carries the instance's identity and references](../decisions/0081-the-archive-carries-identity-and-references.md)
- [0013 — The managed dir is the record](../decisions/0013-managed-dir-of-record.md)
- [0011 — A modpack decomposes into existing parts](../decisions/0011-modpack-decomposes-into-existing-parts.md)
- [0021 — Backups follow docker-mc-backup](../decisions/0021-backups-follow-docker-mc-backup.md)
