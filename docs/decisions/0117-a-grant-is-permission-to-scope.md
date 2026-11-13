# A grant is permission to scope, driven by one table

*Applies to: [Plugins](../architecture/plugins.md#permissions), [plugin permissions](../plugins/permissions.md)*

Every plugin port added a list. The grant was twelve flat fields (`network`, `unhashed`, `ipc`, `files`, `programs`,
`launch_properties`, and one each for hooks, events, schedules, secret settings and theme CSS) and each was enforced its
own way: one by the engine gate, one by a handler asking the plugin store, one inside an import. `widening_over` was a
hand-kept field list, so a field added to `Grant` and forgotten there widened nothing, which is a silent failure of the
approval prompt. The manifest had grown the matching flat keys (`network`, `unhashed`, `ipc`, `files`, `[launch]`).

**A grant is `permission → scope`.** A manifest's `[permissions]` is a table; each permission is a key and carries a
`PermissionScope`, a sorted set of entries for each `ScopeKind` it accepts. `{}` is the one spelling of "no scope".

```toml
[permissions]
"entry:read" = { files = ["whitelist.json", "ops.json"] }
"content:read" = {}
"network:connect" = { hosts = ["api.adoptium.net"], unhashed = ["meta.fabricmc.net"] }
"process:spawn" = { programs = ["git"] }
"ipc:connect" = { endpoints = ["discord-ipc-*"] }
"launch:patch" = { properties = ["my.flag"] }
```

**One table drives everything.** `Permission::spec` gives each permission its tier, the kinds it accepts and whether it
needs an entry of the first. Validation reads it (a kind the permission does not take, a missing required scope, then
each entry against its kind's rule, all collected), `Grant::widening_over` walks the maps and set-differences each kind
(a new permission, or any new entry, widens; a removal does not), `Grant::approval_digest` hashes the canonical grant,
and the CLI and desktop prompts loop over permissions and their kinds. A new kind is covered by every one of them
without a line of per-field code; a permission without a `spec` arm does not compile, and `tests/scope.rs` fails one
missing from `Permission::ALL`, a kind no permission and no contribution carries, or a kind that does not widen.

**Contributions are scope too.** The approved things that are not permissions (hooks, events, schedules, secret
settings, themes) live in `Grant::contributions`, a `PermissionScope` over the kinds `hooks`, `events`, `schedules`,
`secrets` and `themes`, derived from the manifest and never declared. They widen, hash and render by the same code. A
schedule entry reads `poll: 300s`, parseable by the host.

**A program is granted by naming its id.** `[[programs]]` stays the definition list. `process:spawn` names the ids it
may run, and the grant holds each named program's whole definition on one line as its entry, so a changed argument
pattern, environment name, `PATH` dependency or hash is a different entry and widens. `Grant::programs` keeps the
definitions the host runs; it is a lookup, not a second source of truth. A manifest naming an id it does not define is
refused. Programs defined but not named stay unreachable (a `PATH` dependency of another program is one).

**A contract declares the narrowing of its channel.** `Contract::NARROWED_BY` names the kind of the `PLUGIN` permission
that narrows it and `Contract::touches` reads the entries one call reaches. The router keeps the extractor beside the
permission, and the plugin gate checks each touched entry against the caller's scope, so `server.files.read` no longer
has a handler asking the plugin store. A narrowed channel whose payload does not decode is refused. The `plugin_api`
snapshot records `narrowedBy`.

**Closed by default.** An entry of a permission that is not held is unreachable (the accessors read through the
permission), an unknown scope kind in a manifest does not parse, and a permission that requires a scope is refused
without one: `network:connect` with no `hosts` and `ipc:connect` with no `endpoints` are errors rather than quiet
no-ops. The old top-level lists are gone, and a manifest still carrying one is ignored like any unknown key, granting
nothing.

Rejected:

- **Keeping the flat lists and tidying them.** Widening would stay a field list somebody maintains by hand.
- **A scope per permission as its own struct.** Typed, but then every consumer matches on the permission to find the
  field, and widening and rendering would again be per-permission code.
- **`true` as well as `{}`.** Two spellings of one thing.
- **Programs granted implicitly by declaration.** The grant could not say which of the declared programs a plugin may
  run, and defining a program would silently be granting it.

Supersedes how [0092](0092-a-contract-names-the-permission-a-plugin-needs.md) (`files`),
[0101](0101-plugins-spawn-only-declared-programs.md) (`[[programs]]` as the grant) and
[0114](0114-an-unhashed-download-is-declared.md) (`unhashed`) spell their manifest keys and `Grant` fields.
