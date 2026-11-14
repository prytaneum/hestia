# Writing a plugin

A plugin is a WASM component the daemon runs in a sandbox. It can call the daemon's channels, hear its events, adjust or
refuse an operation at a hook point, add commands, pages, settings and themes, fill an open registry (content sources,
flavors, pack formats, Java distributions), and reach the network, declared programs and local endpoints, but only
through what the user approved at install. The design is in [architecture/plugins.md](../architecture/plugins.md); this
guide is for the author.

## Quickstart

Needs a Rust toolchain and the `wasm32-wasip2` target. `new`, `pack` and `dev` build locally; `dev` and `add` need a
running daemon (`hestia daemon start`).

The authoring verbs are hidden until you turn on developer mode (in the desktop: Settings > General > Advanced). With
the daemon running:

```bash
hestia config set developer true
```

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
- `dev` builds the plugin, stages `dist/dev/<id>/`, installs that folder in dev mode and streams the plugin's log. It
  then watches `src/`, `Cargo.toml`, `plugin.toml`, `assets/` and `themes/` and rebuilds when they change, so saving a
  file is enough; a build that fails is reported and the previous build keeps running. A manifest that asks for more
  than you approved leaves the plugin at `needs approval`; run `dev` again.
- A dev plugin is a plugin like any other: its providers are `<plugin>.<id>`, you approve the same grant, every
  permission is checked, and it gets no automatic approval and no updates. The bare name of one of its providers
  (`fabric` for `fabric.fabric`) resolves to it while developer mode is on, unless a signed official plugin provides
  that name. Developer mode is required, and turning it off disables the dev plugin.
- `--sdk <dir>` (or `HESTIA_SDK`) on `dev`, `check`, `test`, `pack` and `publish` builds against a local checkout of the SDK crate
  (`crates/sdk` of a Hestia checkout) while it is unreleased or changing. Nothing in your plugin is edited, so there is
  no `[patch]` to write or to remember to remove.
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
| [official.md](official.md) | the twelve plugins Hestia ships: what each provides, its permissions and settings, how they arrive |
| [manifest.md](manifest.md) | every `plugin.toml` field and what validation refuses |
| [permissions.md](permissions.md) | the permissions, the tiers, declared programs and local endpoints |
| [commands.md](commands.md) | contributed commands: arguments, scopes, progress, cancel, how the CLI and desktop run them |
| [http.md](http.md) | outbound HTTP, URLs, status errors and hashes |
| [downloads.md](downloads.md) | fetching a file to disk through the daemon: checksums, the cache, progress and cancel |
| [events.md](events.md) | subscribing to daemon events, delivery rules, the call budget, schedules and connection events |
| [ipc.md](ipc.md) | talking to local programs over a socket or pipe, and hearing when they speak |
| [settings.md](settings.md) | reading and writing the plugin's own settings, secrets included |
| [data.md](data.md) | the plugin's own files, and updating one atomically |
| [hooks.md](hooks.md) | adjusting or vetoing a launch, a server start or a content install |
| [providers.md](providers.md) | adding content sources, flavors, pack formats and Java distributions, with a compiled example of each |
| [processes.md](processes.md) | running declared programs: `exec` against `spawn`, agents that are downloaded and hashed |
| [paths.md](paths.md) | what a guest sees as its filesystem: the mounts, their access, and how a path is checked |
| [ui.md](ui.md) | pages built from `UiNode` trees, with an example of every node |
| [webview.md](webview.md) | the sandboxed `webview` node and its `postMessage` bridge, with a helper to copy |
| [themes.md](themes.md) | restyling the app with a stylesheet |
| [repository.md](repository.md) | the one shape of a plugin repository, its index and its keys |
| [updates.md](updates.md) | how installed plugins are checked for newer builds, and what is never checked |
| [testing.md](testing.md) | unit-testing plugin code on your machine with the SDK's mock host |
| [publishing.md](publishing.md) | `check`, `pack` and `publish` from CI, on GitHub or any host |
| [other-languages.md](other-languages.md) | writing a plugin without the Rust SDK |

## Examples

Example plugins live in [prytaneum/plugin-examples](https://github.com/prytaneum/plugin-examples)
([0103](../decisions/0103-example-plugins-live-in-their-own-repository.md)): `hello`, `static-index`, `tunnel-demo`,
`launch-flags`, `dashboard`, `discord-presence` and the `theme-ember-night` theme. Every Rust sample in this guide names the file it comes from, and the provider examples in
[providers.md](providers.md) are compiled by the engine's tests. A feature with no compiled example is documented from its
manifest and the SDK signature only, and says so.

## Licensing

A plugin may use any license. What a plugin compiles against is permissively licensed so that holds: `crates/proto`, the
SDK (`crates/sdk`, crate `hestia-sdk`) and its WIT package are `MIT OR Apache-2.0`. Everything else in Hestia is
`GPL-3.0-only`. See [0091](../decisions/0091-plugins-are-wasm-components.md) for the reasoning.
