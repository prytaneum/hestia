# A local file is identified by its hash

*Applies to: [Content & modpacks](../architecture/content.md#local-files-are-inspected-not-trusted)*

A jar imported from disk used to be recorded with no provenance — source
`file`, no project, no version — so it could never update, showed as a bare
filename, and had to be embedded whole in an `.mrpack` export. Most such jars
are ordinary platform releases the user happened to download by hand.

An import now asks the sources which of the files they published.
`ContentProvider::identify` takes the files and answers with the version each
one is, keyed by sha1; `Content::identify` runs it across every available
source, bulk-fetches the matched projects, and `content.add` records a
recognised file as that release. It is then an ordinary tracked item: it has a
title and icon, `check_updates` sees it, and an export links it instead of
embedding it. `content.inspect` runs the same lookup for one file and returns
every source's match, so the desktop's review step names what was resolved
before anything is installed.

- **Which source tracks a file is the user's choice.** The same jar is often on
  both platforms, and they are not interchangeable — updates, changelogs and
  what an export links all follow the source. So every source is asked even
  after one answers, the review step preselects the first match and offers the
  rest, and the item's `source` carries the pick: empty for the first match, a
  source id for that one, `file` to keep it untracked. A source that was picked
  and no longer recognises the file at install leaves it untracked rather than
  falling through to another.

- **The provider is handed the file, not a digest.** Modrinth indexes by sha1
  (`POST /version_files`); CurseForge by its own fingerprint, a MurmurHash2
  over the bytes with whitespace stripped (`POST /fingerprints`). A
  digest-shaped seam would have fitted one of them. sha1 stays the key answers
  come back under, since that is what the index already records.
- **The local bytes are what is installed.** The hash proves they are the
  release, so nothing is downloaded again; only the record changes.
- **Dependencies are not resolved.** Dropping a file means "this file". Pulling
  in what it requires is what installing from a source is for.
- **A failed lookup never fails an import.** Offline, rate-limited or simply
  unknown, the file installs untracked exactly as before, and the reason is
  logged.
- **The lookup is repeated at install.** The daemon does not take the client's
  word for what a file is; the match shown in review is advisory, like the
  detected kind ([0015](0015-local-imports-are-inspected.md)).

Prism Launcher and the Modrinth app both identify local files this way — bulk
by hash, metadata only, no dependencies — so a file recognised here is one they
would recognise too.

**Rejected:** matching by filename or by the mod id in the jar's manifest. Both
name a project at best, never the exact release, and a renamed or repackaged
jar would be tracked as something it is not.
