# Writing a plugin without the Rust SDK

A plugin is a WASM component, so any language with a component toolchain can in principle produce one. The Rust SDK
(`hestia-sdk`) is the only supported path. **No other language is built or tested here**; this page states the contract
and points at the toolchains, and every step beyond it is yours to verify.

## The contract

The ABI is one WIT package, `hestia:plugin@1.0.0`, in [`crates/sdk/wit/`](../../crates/sdk/wit) (`MIT OR Apache-2.0`).
Its version is the plugin API version; the manifest's `api = "^1.0.0"` is a caret requirement on it.

```wit
world plugin {
    import log;
    import host;
    import process;
    import progress;
    import settings;
    import ipc;
    import data;
    import download;
    import messages;
    export lifecycle;
    export commands;
    export events;
    export settings-changed;
    export ui;
    export hooks;
    export provider;
}
```

- Imports: `log`, `host` (`call` and `run-job` reach daemon channels), `process`, `progress`, `settings`, `ipc`, `data`
  (named locks), `download` and `messages`.
- Exports: `lifecycle.init`, `commands`, `events`, `settings-changed`, `ui` (`render`, `action`), `hooks` and
  `provider.invoke`. Hook points, provider kinds and event topics are data in `proto`, not new exports.
- Payloads are JSON strings shaped like the matching `proto` types. Errors are the `plugin-error` variant in both
  directions: `not-found`, `invalid`, `denied`, `unsupported`, `offline`, `upstream`, `rate-limited`,
  `sign-in-required`, `not-configured`, `missing-requirement`, `busy` and `failed`. For hooks, its message is the veto
  reason.
- `deps/` holds the WASI 0.2 packages the world builds on. The host runs components on wasmtime with WASI 0.2; WASI 0.3
  is not targeted, so point a toolchain that defaults to 0.3 at 0.2. The component must be a component, not a core
  module.

## Calling the host without Rust

Where Rust has `hestia::call::<C>` with typed params and results, `host.call(channel, payload)` takes a channel name and
a JSON string and returns a JSON string, unchecked. The shapes come from the generated TypeScript in
[`frontend/src/api/types/`](../../frontend/src/api/types): `channels.ts` maps every channel to its params and result,
every job to its progress and done payloads and every topic to its payload, with the individual types in `generated/`.
Fields are camelCase. Only channels that name a permission are open to a plugin ([permissions.md](permissions.md)).
Provider operations ([providers.md](providers.md)) and page trees ([ui.md](ui.md)) use the same generated types.

## Building and installing

`hestia plugin pack` and `publish` assume a cargo crate, so a plugin in another language has no tested route into a
repository. Installing works:

- Write `plugin.toml` by hand with `wasm = "plugin.wasm"` and `api = "^1.0.0"`; a component without `api` is refused.
- Put the manifest and the component (plus `assets/` and `themes/`) in a folder and run `hestia plugin add <folder>`.
  The permissions it asks for are shown for approval like any other. A `.hplugin` is a zip of the same files.

## Toolchains

Untested here; check each project's current documentation.

- **JavaScript and TypeScript:** `jco componentize --wit crates/sdk/wit --world-name plugin --out plugin.wasm plugin.js`
  ([jco](https://github.com/bytecodealliance/jco)). Its bundled starting points may default to WASI 0.3; select 0.2.
- **Python:** `componentize-py --wit-path crates/sdk/wit --world plugin componentize app -o plugin.wasm`
  ([componentize-py](https://github.com/bytecodealliance/componentize-py)).
- **Go:** [componentize-go](https://github.com/bytecodealliance/componentize-go) or TinyGo's `wasip2` target; neither is
  verified against `hestia:plugin`.

Before committing to one, build a component that exports only `lifecycle` and check that `hestia plugin add <folder>`
loads it. The [component model book](https://component-model.bytecodealliance.org/language-support.html) covers each
language.
