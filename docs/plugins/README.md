# Writing a plugin

A plugin is a WASM component the daemon runs in a sandbox. It can call the daemon's channels, hear its events, adjust or
refuse an operation at a hook point, add commands, pages, settings and themes, and fill an open registry (content
sources, flavors, pack formats, Java distributions). It reaches the network, declared programs and local endpoints only
as far as the user approved at install. This guide is for the author; how the host works is in
[architecture/plugins.md](../architecture/plugins.md).

## Quickstart

You need a Rust toolchain, the `wasm32-wasip2` target and a running daemon (`hestia daemon start`). The authoring verbs
are hidden until developer mode is on (desktop: Settings > General > Advanced):

```bash
hestia config set developer true
rustup target add wasm32-wasip2
hestia plugin new hello        # ./hello/: a cargo workspace, plugins/hello/, CI workflows
cd hello
hestia plugin dev              # build, install in place, approve once, rebuild on every change
hestia plugin pack             # dist/hello-0.1.0.hplugin
hestia plugin add dist/hello-0.1.0.hplugin
```

- `new <id>` outside a repository creates one around the plugin; inside one it adds `plugins/<id>/`. The id is 2 to 40
  lowercase letters, digits or dashes and may not be a CLI verb.
- `dev` stages `dist/dev/<id>/`, installs it in place, streams the plugin's log and rebuilds when `src/`, `Cargo.toml`,
  `plugin.toml`, `assets/` or `themes/` change. A failed build keeps the previous one running. A manifest that asks for
  more than you approved leaves the plugin at `needs approval`; run `dev` again.
- A dev plugin is a plugin like any other: same grant, same checks, no automatic approval or updates. While developer
  mode is on, the bare name of one of its providers (`fabric` for `fabric.fabric`) resolves to it unless a signed
  official plugin has that name. Turning developer mode off disables it.
- `--sdk <dir>` (or `HESTIA_SDK`) on `dev`, `check`, `test`, `pack` and `publish` builds against a local checkout of
  `crates/sdk` instead of the released crate.
- `pack` builds the component, writes the `api` requirement into the packaged manifest and zips `plugin.toml`, the
  component, `assets/` and `themes/`. `--sign <key>` also writes the `.minisig`.
- `add` takes a `.hplugin`, a folder, an https link or an id from an added repository, shows the permissions and
  installs on your approval (`-y` approves).

The scaffold is the smallest plugin: a `plugin.toml` with `id`, `name`, `version`, `description` and `wasm`, and a
`Plugin` whose `init` logs a line. Every command is in [cli.md](../cli.md).

## The pages

| Page | Covers |
|---|---|
| [manifest.md](manifest.md) | every `plugin.toml` field: identity, programs, settings, contributions |
| [permissions.md](permissions.md) | the permissions, their scopes and what each grants |
| [api.md](api.md) | what a plugin can do at run time: state, settings, HTTP, downloads, programs, local sockets |
| [commands.md](commands.md) | contributed commands |
| [events.md](events.md) | events, schedules and the launch, start and install hooks |
| [providers.md](providers.md) | content sources, flavors, pack formats and Java distributions, with a compiled example of each |
| [ui.md](ui.md) | pages built from `UiNode` trees, and the sandboxed webview |
| [themes.md](themes.md) | restyling the app with a stylesheet |
| [testing.md](testing.md) | unit-testing plugin code with the SDK's mock host |
| [publishing.md](publishing.md) | `check`, `pack` and `publish` from CI |
| [repository.md](repository.md) | the shape of a plugin repository, its index, keys and trust |
| [official.md](official.md) | the plugins Hestia ships |
| [other-languages.md](other-languages.md) | writing a plugin without the Rust SDK |

## Examples

[prytaneum/plugin-examples](https://github.com/prytaneum/plugin-examples) holds `hello`, `static-index`, `tunnel-demo`,
`launch-flags`, `dashboard`, `discord-presence` and the `theme-ember-night` theme. The provider examples in
[providers.md](providers.md) are compiled by the engine's tests. A feature with no compiled example says so.

## Licensing

A plugin may use any license. What it compiles against (`crates/proto`, the SDK `crates/sdk` and its WIT package) is
`MIT OR Apache-2.0`; the rest of Hestia is `GPL-3.0-only`.
