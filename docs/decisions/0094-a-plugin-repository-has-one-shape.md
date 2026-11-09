# A plugin repository has one shape, and it is also the store it declares

*Applies to: [Front-ends](../architecture/frontends.md), [repository spec](../plugins/repository.md)*

Hestia installs, updates and lists plugins from repositories. Without one fixed shape, each of those would need
heuristics (scanning release assets, discovering manifests) that break on the next unusual repository.

**A repository is a cargo workspace with `plugins/<id>/plugin.toml` per plugin, always.** One plugin or many looks the
same, so nothing branches on the count. The workspace `[workspace.metadata.hestia]` declares what the repository is to
its users: `name`, a 1 to 4 character `label` for the store badge, `website` and the minisign `key`. No second manifest
file exists at the root. `hestia plugin check` enforces the shape, and `pack` and `index` run it first.

> Superseded in part by [0111](0111-a-repository-publishes-to-its-own-branch.md): the listing below is published to an orphan `repo` branch as `plugins.json`, not to a rolling release, and a build needs no tag.

**The listing is built from source.** CI regenerates `hestia-index.json` on every push to the default branch and
publishes it at one stable URL; a build is attached to a release only when its author tags `<id>-v<version>`.
Source fields come from the default branch and `builds` accumulates, so an untagged version is listed as
`unreleased` and cannot be installed. Each build carries the `api` requirement `pack` wrote, and the host picks the
newest build its API satisfies (the role of Obsidian's `versions.json`).

**The store view.** Browsing (8.02) shows the repository as one store: its label, name and website, then its plugins.
Trust (8.06) is by the declared `key`: the index and packages are signed, and a repository is the key it declares. A
repository is added by its index URL, as Mihon extension repositories are.

**Rejected.** Discovering plugins by scanning release assets or a root manifest (heuristics per repository); alternative
layouts behind flags (every consumer would adapt); one tag for the whole repository (it rebuilds every plugin on any
release).
