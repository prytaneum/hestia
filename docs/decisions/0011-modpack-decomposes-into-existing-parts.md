# A modpack is three things at once, and each goes where it already belongs

*Applies to: [Content & modpacks](../architecture/content.md)*

Installing a pack could have been a store of its own — a `modpack/` tree, its
own mirror, its own update logic. It is not, because a pack decomposes cleanly
into things this codebase already has:

- its **loader and game version** become the entry's flavor and version, so
  creating from a pack is the ordinary create with those filled in (which is
  also why `instance|server create --modpack` and `modpack install` are one code
  path);
- its **index files under a flat managed load dir** become ordinary pool items
  tagged `modpack:<project>`, so the launch-time mirror, the backup heal,
  `content list`, per-item enable and per-item update all work on them
  unchanged;
- **everything else it ships** — its `overrides/`, plus any index file outside a
  managed dir — is written straight into `data/` and recorded in `modpack.json`.

Only the third needed anything new, and only for one reason: those files are
configs and keymaps *the user edits*. So the record stores the sha1 each was
written with, and an update replaces a file the pack still owns while leaving a
tweaked one exactly as found, reporting which through
`WarningInfo::ModpackOverridesKept`. They are not given a managed copy of their
own: unlike a jar, a config lives inside the backup archive already, so a
restore covers them and a second copy would double every config on disk to
re-solve a solved problem. Both references agree on this much (Modrinth's
launcher and Prism both extract overrides in place and track their hashes); the
divergence is that hestia's *mods* are pool items rather than pack-owned files,
which is what makes a pack's mod individually updatable.

**The server side is new ground.** Both references are client-only — Modrinth
handles `overrides/` and `client-overrides/` and skips `server-overrides/`
outright — so the server half follows the format spec rather than a precedent:
`env.server` decides which index files are wanted, and `server-overrides/` takes
the place of `client-overrides/`. The shared tree is written first so a side
tree wins where both name a path, which is what having two trees means.

**A pack's mods are identified for free.** A pack index names each file by URL
and hash alone, with no project or version id — which would make a 150-mod pack
list as 150 anonymous filenames and leave `content update` nothing to work with.
But a platform's own CDN URL carries both ids
(`cdn.modrinth.com/data/<project>/versions/<version>/…`), so `parse_file_url`
recovers them at no cost, and one bulk `projects` call fills in every title and
icon. Both are provider-trait methods, so CurseForge slots in behind the same
seam. A file no source recognises is recorded as `source: "file"` — it
installs, it is simply not updatable, exactly like a local import.
[0076](0076-a-pack-file-is-identified-wherever-it-came-from.md) later widened
this: the URL names its own source, a hash lookup covers what no URL does, and
a jar in an override tree joins the pool.

**What a pack cannot do:** it cannot be installed into an entry whose flavor or
game version differs from what it pins (`ModpackEntryMismatch` names both sides
— the entry's profile is resolved and neither can change in place), and a pack
pinning a loader with no hestia flavor is refused by name rather than quietly
installed as vanilla. The flavor check is the *registry*, not a match arm: a
pack's loader name **is** hestia's flavor id, so adding a flavor needs no edit
in the modpack flow.

**A pack update carries the game version with it**, because that is what
updating a pack means — a pack that bumps 1.21.1 → 1.21.4 is the common case,
and refusing it would leave `modpack update` useful only for the rare
same-version bump. So it runs the entry's existing version-update flow (a
server's automatic pre-update backup included) behind the same explicit
`allow_downgrade` gate. A loader change still refuses: the flavor is baked into
the resolved profile.
