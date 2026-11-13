# A plugin error is its own typed enum, mapped to and from ErrorInfo by the host

*Applies to: [Plugins](../architecture/plugins.md#errors)*

A plugin's calls failed as `failed(string)`. A host call, a provider operation, an `http` request and an `exec` all
flattened their cause into a message, so a plugin could not tell "sign in first" from "the upstream said 503", a missing
`git` from a refused program, or "offline" from "refused". The other direction was just as blunt: the host learned that
a provider had failed to reach its upstream only because the http hooks set a flag (`unreached`) when the last request
ended in a transport error, and a provider call that failed *after* such a request was reported offline whatever the
guest had meant.

**One WIT variant, `plugin-error`, in both directions.** Host calls, provider operations, commands, hooks, ui, process,
ipc and `lifecycle.init` all return it; `hooks.intercept` returns it instead of a string. The cases are the ones a plugin
can branch on and the host can attribute: `not-found`, `invalid`, `denied`, `unsupported`, `offline`, `upstream`
(status, retry-after, message), `rate-limited`, `sign-in-required`, `not-configured` (the setting key),
`missing-requirement` (name, optional url), `busy` and `failed`.

**The host maps it in one place per direction** (`engine/src/plugins/error.rs`). `GuestError` is the Rust mirror of the
variant, so the generated type never leaves the sandbox module.

- *Down*: `From<&ErrorInfo> for GuestError` is an exhaustive `match`, with no catch-all, so a new daemon error forces a
  decision about what a plugin sees. `HostCalls` now carries the `ErrorInfo` itself instead of three strings; the daemon
  no longer classifies by `code()`. A program that is not installed (not pinned at approval, or missing from `data/`)
  is `missing-requirement`.
- *Up*: `GuestError::into_info` turns what a plugin answered into an `ErrorInfo`. A provider's `offline`, `upstream` and
  `rate-limited` are attributed to the provider's `Service`; `sign-in-required` is `PluginSignInRequired`, naming the plugin; `not-configured` is
  `PluginSettingInvalid`; everything else is `PluginCallFailed` with the message. `unsupported` still means "not
  implemented" and keeps an optional operation's fallback. A hook's error still vetoes, its message being the reason, and
  a hook that traps or times out still fails open.

**The guest says `offline` itself.** The SDK maps the connection-class `wasi:http` codes (`connection-timeout`,
`destination-unavailable`) to `offline`, the refusal codes to `denied`, and the rest to `failed`. `unreached`,
`note_failure` and `CallError::Unreachable` are gone; the host keeps only the shared reachability signal the request
already fed.

Rejected:

- **Exposing `ErrorInfo` to plugins.** It has a hundred variants of daemon internals, shaped for the front-ends. Plugins
  would bind to all of them, and each daemon refactor would become a plugin API change. A small enum the host maps keeps
  the plugin API stable while the daemon moves.
- **Keeping the `unreached` heuristic.** The host guessed from a side channel what the guest knew first-hand, and
  guessed wrong whenever a plugin made one failed request and then failed for another reason.
- **A `status` on every upstream failure.** The daemon's own `Upstream` error has no status, so `upstream.status` is
  optional; a guest that made the request fills it in.
