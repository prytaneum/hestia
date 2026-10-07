# A local file is identified by its hash

*Applies to: [Content & modpacks](../architecture/content.md#local-files-are-inspected-not-trusted)*

A jar imported from disk used to be recorded with no provenance — source
`file`, no project, no version — so it could never update, showed as a bare
filename, and had to be embedded whole in an `.mrpack` export. Most such jars
are ordinary platform releases the user happened to download by hand.

An import now asks the sources which of the files they published.
`ContentProvider::identify` takes the files and answers with the version each
one is, keyed by sha1; `Content::identify` runs it across every available
source (first to recognise a file wins), bulk-fetches the matched projects, and
`content.add` records a recognised file as that release. It is then an ordinary
tracked item: it has a title and icon, `check_updates` sees it, and an export
links it instead of embedding it. `content.inspect` runs the same lookup for
one file, so the desktop's review step names the match before anything is
installed.

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
