# Plugin tooling builds quickly, builds against a local SDK, and reports every problem

*Applies to: [Plugins](../architecture/plugins.md), [the plugin author guide](../plugins/README.md), [Build & run](../contributing.md#build--run)*

Porting the first official plugins cost most of its time in the loop around the code. A debug build of the daemon
compiled wasmtime and Cranelift without optimisation, so compiling a plugin took about a quarter of an hour. A plugin written against an unreleased SDK needed a hand-written `[patch]` in its `Cargo.toml` that had to be
removed before a release. One invalid plugin in a repository stopped `check`, `pack` and `publish` for all the others, and a manifest
reported its first mistake and no more.

**The dev profile optimises the compiler a plugin passes through.** `[profile.dev.package.*]` in the workspace
`Cargo.toml` sets `opt-level = 3` for `wasmtime`, the `cranelift-*` crates and the few crates under them (`regalloc2`,
`wasmparser`, `gimli`, `object`, `hashbrown`, `smallvec`, `bumpalo`, `rustc-hash`). They rebuild only when the lockfile
moves, so the cost is paid once and a plugin compile in a debug daemon is no longer the bottleneck. This sits beside
[0102](0102-one-feature-resolution-for-dev-builds.md), which keeps wasmtime compiling once per profile.

**`--sdk <dir>` builds against a local SDK without editing the plugin.** `dev`, `pack` and `publish` take the folder of
the `hestia-sdk` crate (or `$HESTIA_SDK`; the flag wins), check it is that crate, and hand cargo a `patch` on its command
line for each source the repository's plugins take `hestia-sdk` from (`crates-io`, or the git URL a manifest names).
`Cargo.lock` is put back as it was after each run. Nothing the author keeps in the repository changes, so there is
nothing to forget to remove.

**`dev` rebuilds on a change.** It polls the modification time and size of `src/`, `assets/`, `themes/`, `Cargo.toml`
and `plugin.toml` every half second with the standard library, rebuilds once the edits have been quiet for half a
second, and reports the build's errors where they happen. A failed build leaves the previous one running and the watch
going.

**`check`, `pack` and `publish` work per plugin and say everything.** A repository still has to follow its one shape
([0094](0094-a-plugin-repository-has-one-shape.md)) as a whole, but then each plugin is processed on its own: `check`
lists every plugin as `ok` or with every problem found (`Manifest::validate` returns all its errors, not the first), and
exits non-zero if any selected one failed, and `pack` and `publish` build every valid plugin and fail the run at the end
naming the invalid ones. An invalid plugin gets no new build in the index and keeps the builds already listed, so a typo
in one plugin never withdraws another's release.

Rejected:

- **A `[patch]` section the CLI writes into the plugin's manifest.** It edits a file the author owns, and an interrupted
  run leaves it behind.
- **A file-watching crate.** Mtime polling needs no dependency and behaves the same on Linux and Windows; half a second
  is below what a compile takes.
- **Failing fast.** It is quicker to report and slower to fix: an author with three mistakes in a manifest learned of them
  one run at a time.
