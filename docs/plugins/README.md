# Writing a plugin

A plugin is a WASM component the daemon runs in a sandbox. It can call the daemon's channels, hear its events, adjust or
refuse an operation at a hook point, add commands, pages, settings and themes, fill an open registry (content sources,
flavors, archive formats, Java distributions), and reach the network, declared programs and local endpoints, but only
through what the user approved at install. The design is in [architecture/plugins.md](../architecture/plugins.md); this
guide is for the author.

## Quickstart

Needs a Rust toolchain and the `wasm32-wasip2` target. `new`, `pack` and `dev` build locally; `dev` and `add` need a
running daemon (`hestia daemon start`).

```bash
rustup target add wasm32-wasip2
hestia plugin new hello        # ./hello/: a cargo workspace, plugins/hello/, CI workflows
cd hello
hestia plugin dev              # build, install in place, approve the grant once, reload on every rebuild
hestia plugin pack             # dist/hello-0.1.0.hplugin
hestia plugin add dist/hello-0.1.0.hplugin
```

- `new <id>` outside a repository creates one around the plugin; inside one it adds only `plugins/<id>/`. The id is 2 to
  40 lowercase letters, digits or dashes and may not be a CLI verb ([manifest.md](manifest.md#identity)).
- `dev` stages `dist/dev/<id>/`, installs that folder in dev mode and streams the plugin's log. After editing, rebuild
  with `cargo build --release --target wasm32-wasip2`; `dev` picks the new component up within a second. A manifest
  that asks for more than you approved leaves the plugin at `needs approval`; run `dev` again.
- `pack` builds the component, writes the `api` requirement into the packaged manifest and zips `plugin.toml`, the
  component, `assets/` and `themes/`. Add `--sign <key>` to also write the `.minisig`.
- `add` takes a `.hplugin`, a folder, an https link, or a plugin id from an added repository, shows the permissions it
  asks for and installs on your approval (`-y` approves).

The scaffold is the smallest plugin: a `plugin.toml` with `id`, `name`, `version`, `description` and `wasm`, and a
`Plugin` whose `init` logs one line. The
[`hello`](https://github.com/prytaneum/plugin-examples/tree/main/plugins/hello) example is the same shape. The full
command reference is in [cli.md](../cli.md#launcher-plugins).

## The guide

| Page | Covers |
|---|---|
| [manifest.md](manifest.md) | every `plugin.toml` field and what validation refuses |
| [permissions.md](permissions.md) | the permissions, the tiers, declared programs and local endpoints |
| [commands.md](commands.md) | contributed commands: arguments, scopes, progress, cancel, how the CLI and desktop run them |
| [events.md](events.md) | subscribing to daemon events, delivery rules and schedules |
| [hooks.md](hooks.md) | adjusting or vetoing a launch, a server start or a content install |
| [providers.md](providers.md) | adding content sources, flavors, archive formats and Java distributions |
| [processes.md](processes.md) | running declared programs: `exec` against `spawn`, agents that are downloaded and hashed |
| [ui.md](ui.md) | pages built from `UiNode` trees, with an example of every node |
| [themes.md](themes.md) | restyling the app with a stylesheet |
| [repository.md](repository.md) | the one shape of a plugin repository, its index and its keys |
| [publishing.md](publishing.md) | `check`, `pack` and `index` from CI, on GitHub or any host |
| [other-languages.md](other-languages.md) | writing a plugin without the Rust SDK |

## Examples

Example plugins live in [prytaneum/plugin-examples](https://github.com/prytaneum/plugin-examples)
([0103](../decisions/0103-example-plugins-live-in-their-own-repository.md)): `hello`, `static-index`, `tunnel-demo`,
`launch-flags`, `dashboard`, `discord-presence` and the `theme-ember-night` theme. Every Rust sample in this guide names the file it comes from. A
feature with no compiled example is documented from its manifest and the SDK signature only, and says so.

## Licensing

A plugin may use any license. What a plugin compiles against is permissively licensed so that holds: `crates/proto`, the
SDK (`crates/sdk`, crate `hestia-sdk`) and its WIT package are `MIT OR Apache-2.0`. Everything else in Hestia is
`GPL-3.0-only`. See [0091](../decisions/0091-plugins-are-wasm-components.md) for the reasoning.
