# The archive carries the instance's identity and references

*Applies to: [Import & export](../architecture/transfer.md#the-hestia-archive)*

A `.hestia` archive exported on Windows did not launch on Linux:
`UnsatisfiedLinkError: Failed to locate library: liblwjgl.so`. The manifest
carried the full `InstanceRecord`, including its resolved `InstanceProfile`, and
resolution filters Mojang's library rules by the **exporting** host. The carried
profile pinned `natives-windows` and a Windows-only `-XX:HeapDumpPath`; import
took it verbatim ("no network, no lookup"), so the Windows natives landed on the
Linux classpath and the Linux ones never did. Materialization ran correctly. It
was given the wrong list.

A second defect sat beside it. An export with `mods/` unchecked kept
`content.json` and dropped the files, so the import listed content it never had
and nothing fetched it.

Both come from one confusion: the archive mixed what the instance *is* with what
one machine needs to *run* it.

**The archive carries what the instance is, not how a host runs it.** Flavor,
game version and loader version are authoritative. Import validates the flavor
and resolves the profile again on the importing host (`Recipe::Record`), keeping
the record's name, Java settings, sharing, playtime and timestamps. There is no
manifest change, so an archive exported before this decision is repaired by the
import alone. The cost is that import now needs the version catalogue: it works
offline from the cached one and otherwise fails with the ordinary offline error.
The old promise, that an archive "cannot be broken by a version falling out of a
catalogue", went with it; a version Mojang withdraws now fails the import loudly
instead of producing a profile that cannot start.

**Content a source can serve again is not carried.** An item is *referenceable*
when it has a URL and a SHA-1 (`install::referenceable`, the rule `.mrpack`
export already applied through `mrpack::index_entry`, now shared by both
writers). Its bytes stay out of the archive. Everything else (a local file, a
CurseForge file with distribution disabled, a record with no hash) is embedded.
The archive is the settings, configuration, worlds and local files; a 141 MB
archive of mostly Modrinth mods re-exports at 15 MB.

**Import makes the content list true.** After the files land, every tracked item
either has its file, is downloaded to its managed path, or leaves the list.

- A referenced download goes through `materialize::ensure_artifact`, so the
  content-addressed cache answers first and the SHA-1 is verified: a same-machine
  import downloads nothing. It runs with the concurrency and progress of a
  modpack apply.
- A download that fails or mismatches its SHA-1 fails the import, and the existing
  unwind removes the instance, so the user retries rather than keeping an
  instance that is missing mods. A half-installed instance is worse than none.
- An item that is neither present nor referenceable (an archive that left its
  pool out, or was edited by hand) drops its record and reports the filenames as
  `ImportContentMissing`. The instance imports with the content list it
  actually has.

This also covers the second defect, and a Prism import, which lands nothing in
the pool before adoption, is unaffected.

**An exclusion removes the item, not only its bytes.** The picker stays, for
content too, exactly as in `.mrpack` export: every file is selectable, and a
referenced row shows that it downloads on import instead of a size
(`ArchiveEntry.referenced`, which adds nothing to its directory's total, so the
dialog's total is the archive's size). `content.json` is written by the exporter
rather than copied, filtered through the same rules the files use, so excluding
`mods/x.jar` drops the file and its record and excluding `mods` drops every mod.
`content.json` is derived, so it is not a pickable row. Both formats now mean
the same thing by an exclusion.

**One format, compact only.** There is no mode flag and no "complete" snapshot
that embeds every jar. `.mrpack` stays the interoperable export.

**What other launchers do**, checked against their sources:

- Prism / MultiMC stores component uids and versions in `mmc-pack.json` and
  filters libraries by the host's `RuntimeContext` when it builds the launch
  profile. Its zip export bundles mod jars.
- The Modrinth app and CurseForge app name the game and loader versions and
  reference mods by URL and hash, or by project and file id. Modrinth fails the
  install if one download fails.
- GDLauncher references addons by default, with a "Bundle Addons" toggle for
  offline installs.

None carries a host-resolved library list.

**Rejected:** carrying the resolved profile with a platform marker and
re-resolving at launch. It keeps two sources of truth for the profile and a
launch that can hit the network. **Also rejected:** Prism's model, storing
unfiltered rules in `InstanceProfile` and filtering at launch. It would also
cover copying a data home between operating systems, but it reworks `proto`,
every provider and launch assembly for a path nobody takes. **Also rejected:** a
separate embedded/reference payload model beside `content.json`: the pool index
already says which items are tracked, so a second list would only be able to
disagree with it.
