# Writing a plugin

*[← Architecture](../architecture.md)*

A plugin is a WebAssembly component that Hestia loads in a sandbox. It can only do what its user approved: what it may
reach is read from the component itself, shown in one prompt, and enforced on every call. This guide covers what exists
today; [the plugin kernel](../architecture/plugins.md) explains how the host works.

What exists today:

- The API package `hestia:kernel` (version `0.1.0`): logging, settings, secrets, notices and confirmations, background
  jobs, downloads, programs and local sockets, plus `wasi:http`. Built-in plugins will add packages for servers,
  instances and content; none ships yet, so a plugin cannot extend or read them.
- Rust through the `hestia-sdk` crate, which the CLI scaffolds. A TypeScript example that exports only `lifecycle`
  builds with `jco` (`examples/typescript/`); other languages can build against the WIT in `crates/sdk/wit/kernel/` with
  their own world, untested.
- The CLI tooling: `hestia plugin new`, `check`, `world`, `dev`, `test`, `pack` and `publish` ([the command
  reference](../cli.md#writing-a-plugin)).

## Getting started

You need a Rust toolchain; the project pins `wasm32-wasip2` in its `rust-toolchain.toml`.

```bash
hestia plugin new tidy        # a project in ./tidy
cd tidy
hestia plugin test            # cargo test, against the SDK's mock host
hestia plugin check           # build the component, then check manifest, component and plugin.grant
```

`new` writes:

| File                       | What it is                                                                    |
|----------------------------|-------------------------------------------------------------------------------|
| `plugin.toml`              | the [manifest](manifest.md): id, name, version, later scopes and commands     |
| `plugin.grant`             | the committed [lockfile](grant.md) of what the build asks for                 |
| `Cargo.toml`, `src/lib.rs` | a `cdylib` on `hestia-sdk` (`lifecycle` feature) with one plugin and one test |
| `rust-toolchain.toml`      | stable plus the `wasm32-wasip2` target                                        |
| `.github/workflows/ci.yml` | installs `hestia`, then runs `plugin test` and `plugin check`                 |

The generated `src/lib.rs` is the smallest plugin:

```rust
use hestia::log::{write, Level};
use hestia::{Error, Lifecycle};

struct Plugin;

impl Lifecycle for Plugin {
    fn init() -> Result<(), Error> {
        write(Level::Info, "tidy started");
        Ok(())
    }
}

hestia::export!(Plugin);
```

Each export is a cargo feature of `hestia-sdk` (`lifecycle`, `commands`, `settings-changed`, `on-schedule`,
`on-job-done`). `hestia::export!` exports exactly the ones enabled, so a plugin that does not need `commands` does not
carry it and is not asked to approve it.

To run it for real, turn on developer mode and let `dev` install and reload it:

```bash
hestia config set plugins.developer true
hestia plugin dev             # needs the daemon; rebuilds and reloads on every save, streams the plugin's log
```

## The guide

| Page                                    | Covers                                                              |
|-----------------------------------------|---------------------------------------------------------------------|
| [The manifest](manifest.md)             | identity, scopes, commands, schedules, dependencies, limits         |
| [Permissions](permissions.md)           | what each import asks for, scopes, optional permissions, denials    |
| [Running work](commands.md)             | exports, commands, schedules, jobs, settings, notices, time budgets |
| [Testing and `dev`](testing.md)         | the mock host, `plugin test`, `world`, `dev`, developer diagnostics |
| [The grant lockfile](grant.md)          | `plugin.grant`, `plugin check`, why a widening fails the build      |
| [Packing and publishing](publishing.md) | `pack`, `publish`, repositories, versioning, updates, revocation    |
