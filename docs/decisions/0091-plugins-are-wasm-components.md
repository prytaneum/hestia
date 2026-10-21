# Plugins are WASM components on wasmtime, with WIT as the one ABI

*Applies to: [The engine](../architecture/engine.md), [The socket boundary](../architecture/wire.md)*

Hestia needs third-party extensions that run inside the daemon without being able
to read the user's tokens, escape their own directory or take the daemon down.

**A plugin is a WASM component, run by wasmtime's component model.** Guests
target WASI 0.2 (`wasm32-wasip2`). The host is async and lives in the engine,
so a guest can make blocking calls without ever holding a tokio worker. A plugin
is compiled once into a cached `InstancePre`; every call gets a **fresh
`Store`** from it, so no call can see another's memory.

**The ABI is one WIT package**, `hestia:plugin` in `crates/sdk/wit`. Its version
is the only API version: `proto::plugin::API_VERSION` is typed from it and a test
fails on drift. It starts at 1.0.0, as the wire protocol and document schemas
start at 1. Compatibility is caret semver: a manifest's `api = "^N.M.P"` loads
on a host `N.m.p` at or above `N.M.P`; anything else is refused at install
instead of failing at instantiation on a missing import.

**A breaking change bumps the major; an additive one bumps the minor.**
`crates/proto/tests/plugin_api.rs` snapshots every plugin-reachable contract and
topic, the types they name and the WIT files into `tests/golden/plugin-api.json`.
A changed snapshot without the bump it needs fails and says which kind it is.
Additive: a new channel or interface, a new optional or defaulted field.
Breaking: a removed or renamed field, a changed type, a removed channel, a new
required field, a new enum, WIT variant case, record field or flag, a new WIT
export or a new function in an exported interface. Regenerate the snapshot
afterwards with `HESTIA_UPDATE_GOLDEN=1`. The host never serves two versions.

**The boundary carries proto JSON as strings.** `host.call` and `host.run-job`
take and return JSON, so every existing contract crosses unchanged and the host
does not mirror `proto` in WIT. Type safety therefore comes from the SDK's proto
types, not from WIT records. What the component model buys is WASI, permission
stubs at the linker, and bindings for any guest language.

`lifecycle.init` runs once on enable, on a store that is dropped afterwards. It
is for one-time setup in the plugin's `data/` directory, not for state held in
memory. There is no key/value interface: a plugin persists to its preopened
`data/` directory.

## Threat model

| Asset | Defended by |
|---|---|
| Account tokens, RCON passwords | A guest reaches the daemon only through `host.call` / `host.run-job`, limited to contracts whose `Contract::PLUGIN` is `Some` and granted; the check fails closed. Secret-bearing channels (`server.files.read`, `server.properties`, `config.get`/`config.list`, `process.logs`, `entry.logs`, `process.output`, sign-in and token channels) stay `None` ([0092](0092-a-contract-names-the-permission-a-plugin-needs.md)). No environment access; network access only to granted hosts |
| The user's files | WASI filesystem is empty except the plugin's own preopened `data/` directory |
| Native code execution | The WASM sandbox bounds the guest itself: no syscalls but the WASI imports the linker supplies, and no dylib is ever loaded. Permissions in the unrestricted tier ([0092](0092-a-contract-names-the-permission-a-plugin-needs.md)) grant native code execution by design, and the install prompt states them as full access to the computer |
| The daemon's availability | An epoch tick yields a spinning guest, a wall-clock timeout bounds each call, `StoreLimits` and a pooling allocator cap memory globally, and at most 4 calls run concurrently per plugin. A timed-out or cancelled call's store is dropped and never reused |
| Capabilities (what a call may do) | A channel call is checked against its contract's `PLUGIN` permission by the host before dispatch. A gated interface the user did not grant is linked as a stub whose every function returns `permission-denied` |

The trust boundary is guest code against the daemon. The data home is one trust
domain: someone who can already write it can replace the daemon too, so a local
writer of the data home is out of scope.

**Compiled components are cached under `meta/plugins/`** as
`<wasm-sha256>-<config-hash>.cwasm`, beside a `.sha256` of the artifact itself,
so a daemon start does not recompile every enabled plugin. The config hash is
wasmtime's `precompile_compatibility_hash`, which covers its own version and the
engine settings, so a wasmtime bump or a config change misses the cache. The
`.wasm` hash is verified first; a missing, truncated or mismatching artifact, or a
failed `deserialize`, is deleted and compiled afresh. `Component::deserialize` is
`unsafe` because it trusts its input; the artifact hash guards corruption only.
A writer of the data home could already edit grants, so it is out of scope.
wasmtime's own global cache directory stays off: the cache is part of `meta/`,
which is one unit to reclaim ([0054](0054-meta-root-for-materialised-files.md)).

**Licensing.** A plugin compiles `proto`, the SDK and the WIT bindings into its
own component, which under GPL-3.0-only would make every plugin a derivative
work. Those three are `MIT OR Apache-2.0`: `crates/proto` and `crates/sdk`
(which holds the WIT package) each carry `LICENSE-MIT` and `LICENSE-APACHE`.
Everything else, Hestia itself, stays GPL-3.0-only. `proto` holds no code ported
from a GPL source (the Modrinth-derived code is frontend-only).

**Rejected:**

- **Native dylibs**: no sandbox, and an ABI that breaks per compiler version.
- **Extism**: its strength is many host languages, worth nothing with one Rust host; it trades typed WIT contracts and linker-level capability denial for a bytes-in/bytes-out ABI.
- **Out-of-process plugins over the socket**: sandboxing would rest on the OS per platform, and every plugin would be one more long-lived process to supervise.
- **Embedded scripting**: one language for every author, and a sandbox we would have to build.
- **WASI 0.3 now**: ratified, but most guest toolchains besides Rust (TinyGo, componentize-py, componentize-js) still target 0.2; the host can move later.
- **One long-lived instance per plugin**: re-entry deadlocks, and a cancelled call leaves the instance unusable.
