# An archive format is a module, not a branch — and the launcher matches on the recipe

*Applies to: [Import & export](../architecture/transfer.md)*

Instances needed a way in and out: a file you can hand someone, keep a copy of,
or carry to another machine — and, since instances have no backups, the only
thing standing between a person and losing their worlds. Three formats had to
come in on day one (hestia's own, `.mrpack`, Prism/MultiMC) and two go out, with
CurseForge and ATLauncher obvious later additions.

The shape that suggests itself is a `match` on the format, in the flow that
creates the instance: read a manifest, branch on which kind it was, create,
extract. It works, and it rots exactly where the growth is. Each new format
touches the detection table, the descriptor reader, the create branch, and the
extraction branch — four places, none of which are about that format, and each
of which is where two formats' assumptions get quietly interleaved. Prism's own
importer is a single 700-line task class per this shape; the parts that are
genuinely per-format are a small fraction of it.

**A format is a module and a line in a registry.** It answers three questions —
which archives are mine (a marker file), what does this one say it is (parse a
manifest into a `Blueprint`), where do the files go (given an instance that
already exists) — and it is consulted through a `&'static dyn Format` in one
list, the same way a content platform is
([0010](0010-one-content-provider-trait.md)). It knows nothing about the engine.

What makes that possible is the second half: **a format never creates the
instance**, because creating one is where the engine, the network and the entry
store all come in. Instead the blueprint carries a `Recipe` naming which of
three routes the launcher takes —

- `Record` — the instance's record travels in the archive (hestia's own); its
  profile is resolved again on the importing host
  ([0081](0081-the-archive-carries-identity-and-references.md));
- `Resolve` — the archive names a game version and a loader, resolved exactly as
  `instance create` resolves them (Prism, and any launcher that describes an
  instance rather than shipping one);
- `Pack` — the archive is a modpack, and the modpack flow already creates an
  entry from one, fetches every file from its source and records the provenance
  ([0011](0011-modpack-decomposes-into-existing-parts.md)).

Those three are a **closed set** — they are the genuinely different ways a
launcher can be told what to build — while formats are an open one. The flow
matches on the recipe, so the fourth format costs it nothing: CurseForge is a
`Format` impl picking `Resolve`, reusing the pool adoption Prism already needs.

The same split decides where the shared parts live. What an archive leaves out is
one question with one answer (`transfer::exclude`), because two exporters
deciding it separately is how a format quietly ships somebody's crash reports —
and the tree the desktop shows for excluding things is derived from *the same
plan the export writes*, since a listing that disagreed with the archive would be
worse than no listing at all.

**What was rejected.** A trait whose `import` method takes `&Engine` and does the
whole job: it makes each format a small launcher, puts network and entry-store
concerns behind three different implementations, and breaks the rule that a
cross-subsystem flow is an `impl Engine` block in `flows/`
([architecture](../architecture.md#conventions-that-hold-everywhere)). Also
rejected: deciding the format from the file extension. Every one of these formats
is a zip and people rename them; someone handed an archive by a friend should not
have to know which launcher made it, so the marker inside the file decides —
shallowest first, so a pack index that a Prism instance merely *ships* cannot
masquerade as the pack itself.
