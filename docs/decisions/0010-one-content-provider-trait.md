# Content is normalized behind one trait, following Prism's `ResourceAPI`

*Applies to: [Content & modpacks](../architecture/content.md)*

Prism Launcher drives Modrinth and CurseForge through a strategy-pattern
`ResourceAPI` whose results are platform-agnostic structs, so its UI never
special-cases a platform; Hestia adopts the same shape (`ContentProvider` +
`proto::content`) — and the same split as its own `minecraft` registry, so the
codebase has one way of saying "pluggable upstream catalogue". Resolution is
deliberately separate from installation: a provider names projects, versions and
files, and never writes anything, because installing must compose with the entry
stores' layout and locking (`data/` vs the managed `mods/`/`resourcepacks/`
roots, the backup in-flight keys) — see [Installed content is
managed-dir-of-record](0013-managed-dir-of-record.md). The first design also
had a `modpack.resolve` channel returning a pack's file manifest; the install
flow superseded it, and it was removed.
