# A pack file is identified wherever it came from

*Applies to: [Content & modpacks](../architecture/content.md#modpacks)*

A pack's mods were tracked only when two things lined up: the pack came from a
source, and the file was named by the index with that source's own CDN URL.
Everything else landed anonymous. A `.mrpack` read off disk has source `file`,
so not one of its URLs was parsed — a hundred Modrinth mods listed as bare
filenames, none updatable. And a jar shipped in `overrides/mods/` was written
straight into `data/` as a pack-owned file, outside the pool altogether.

Three changes, one rule — a file is whatever a source says it is, however the
pack carried it:

- **The URL names its source.** `Content::parse_file_url` asks every source
  that can serve instead of the pack's own, and the bulk lookups run once per
  source the files turned out to belong to.
- **What no URL identifies is asked after by hash.** Override jars and
  off-platform index files go through `Content::identify` once they are on
  disk — the lookup [0075](0075-a-local-file-is-identified-by-its-hash.md)
  added for local imports. Where several sources publish a file, the pack's own
  source wins, so a pack's pool stays on the source the pack updates from; with
  none, the first match.
- **A managed-kind override is a pool item.** `overrides/mods/x.jar` is
  extracted to the managed directory and mirrored, tagged with the pack's
  origin like an index file. The override record keeps what it was for:
  configs the user edits.

The archive itself gets the same treatment: a local `.mrpack` a source published
is recorded as that release, so the pack updates too.

- **Best-effort throughout.** Offline or unrecognised, a file installs
  untracked exactly as before.
- **An override jar loses the keep-if-edited rule.** That rule exists for
  configs; a jar is replaced or removed with the pack, as index files already
  were.
- **An override wins a name the index also supplies**, because the override
  trees are written last.

**Rejected:** identifying everything by hash and dropping URL parsing. The URL
costs nothing and works before the download; the hash lookup needs the bytes
(CurseForge fingerprints them), so it is the fallback, not the rule.

The same rule reaches the two other ways bytes arrive without a source:

- **A CurseForge zip read off disk installs.** Its manifest names files by id,
  so the archive hands it to the platform (`ContentProvider::resolve_manifest`)
  and the answer is an ordinary resolved pack. It needs CurseForge configured,
  and says so when it is not. The pack itself stays without an update path:
  CurseForge's fingerprint endpoint matches mod files but not pack archives
  (checked against the live API), so the zip cannot be tied to its release.
- **A Prism/MultiMC import's jars are looked up after they are adopted.** The
  format module stays a parse and a file copy; the flow runs the hash lookup
  over the pool it produced, and `ImportFilesUntracked` names only what no
  source publishes.

- **`instance import` recognises a CurseForge zip.** Its marker,
  `manifest.json`, is a name any archive may carry, so detection reads it and
  only a `minecraftModpack` manifest is claimed. It takes the same route a
  `.mrpack` does: the modpack flow, handed the archive already open.

**What other launchers do**, checked against their sources:

- Prism Launcher detects by filename alone, in the order
  `modrinth.index.json`, Technic's `bin/modpack.jar`, `manifest.json`,
  `instance.cfg`. A local CurseForge zip is resolved through the API, gets an
  empty managed-pack id (no update path), and blocked files are listed for the
  user to fetch by hand. Override jars are not identified at import.
- The Modrinth app reads `.mrpack` only, and hashes the local file to find its
  version on Modrinth.

Hestia matches both where they agree with it — an unpublished CurseForge zip
has no update path, a blocked file fails alone, a local `.mrpack` is found by
hash — and goes further in two places: it checks the manifest's content before
claiming it, and identifies override jars.

There is no migration for an entry whose pack was installed before this: the
project carries none yet.
