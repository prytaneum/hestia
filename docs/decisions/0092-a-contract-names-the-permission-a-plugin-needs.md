# A contract names the permission a plugin needs, and an unnamed one is closed

*Applies to: [The socket boundary](../architecture/wire.md#proto--the-contracts)*

A plugin's `host.call` can reach any daemon channel ([0091](0091-plugins-are-wasm-components.md)).
Without a per-channel rule, every channel a later change adds — including one
that returns a secret — would be reachable by default.

**A contract declares the permission it needs.** `Contract::PLUGIN` and
`Topic::PLUGIN` are an `Option<proto::plugin::Permission>`, `None` by default,
mirroring `CHANGES` ([0085](0085-a-mutating-call-names-what-it-changed.md)). The
host calls a channel for a plugin only when the constant is `Some` and the user
granted that permission. `None` means unreachable, so a new contract is closed
until someone classifies it.

**A permission is `domain:action`**, the shape of Slack's `channels:read` and
Tauri's `fs:allow-read`. The domain is the channel prefix it opens, so a reader
maps a permission to its channels at a glance:

| Domain | Actions | Opens |
|---|---|---|
| `entry` | `read`, `write` | `server.*`, `instance.*`, `entry.*`, `sync.*`, `screenshot.*` |
| `process` | `read`, `control`, `spawn` | `process.list`/`status` and its topics; stopping, starting and the console; the programs the manifest declares |
| `content` | `read`, `write` | `content.*`, `modpack.*` |
| `account` | `read` | profiles only, never tokens |
| `config` | `read`, `write` | `config.*` |
| `java` | `read`, `manage` | `java.list`/`releases`; `java.install`/`uninstall` |
| `network` | `connect` | outbound HTTP to the hosts the manifest lists |
| `ui` | `css` | a theme's stylesheet |
| `launch` | `patch` | an allowlisted patch from a launch or server-start hook |

Each has a one-line description for the install prompt.

**Two tiers.** `Permission::tier()` is `Sandboxed` or `Unrestricted`. The
unrestricted permissions each reach native code, because the game runs what they
write: `entry:write` and `config:write` (per-entry and `defaults.jvm-args` take
`-javaagent:`; `server.files.write` places jars), `content:write` (`content.add`
places jars), `process:control` (the console runs operator commands) and
`process:spawn` (runs programs). The install prompt states them as full access
to the computer, the way Deno documents `--allow-run` and `--allow-ffi`.

**Every existing contract is classified.** A classified contract's shape is
therefore part of the plugin API: changing it bumps `API_VERSION`.

**Always `None`, whatever the domain:**

| Channel | Why |
|---|---|
| `process.start` | Arbitrary program, argv and env; plugins spawn only through `process:spawn` |
| `process.logs`, `entry.logs`, `process.output` | An old client prints its session id to stdout |
| `server.files.read`, `server.properties` | `server.properties` holds `rcon.password` until these redact it |
| `entry.config.get`, `entry.config.list` | A server's config falls through to `server.properties`, which holds `rcon.password`, until these redact `properties::SECRET` |
| `config.get`, `config.list` | `content.curseforge-key` is returned until it is redacted or leaves core |
| `account.login.*`, `account.switch`, `account.remove`, `skin.*`, `cape.*` | Sign-in, tokens and the account's own profile |
| `update.*`, `daemon.stop`, `daemon.status`, `plugin.*` | Self-update, daemon lifecycle and plugin management |
| `events.subscribe`, `job.list`, `job.cancel`, the `job` and `changed` topics | Another client's subscriptions and jobs; a plugin cancels only what it started |
| `content.inspect`, `instance.import.inspect`, `minecraft.ping`, `cache.*` | Reach a path, an address or the whole cache outside any grant |
| `app.info`, `host.info`, `health.ping`, `net.status`, `net.state` | No permission fits; opened when a plugin needs one |

Two tests in `crates/proto/tests/permission.rs` hold this. One fails when a
contract names no permission and is not on the denied list, so a new contract
forces a decision. The other pins the secret-bearing and privileged channels
above to `None`, so reclassifying one fails loudly.

Rejected:

- **Allow by default, deny a list.** A new secret-bearing channel is exposed
  until someone remembers the list.
- **Deriving the permission from the channel name** (`*.list` is a read). The
  misclassification [0085](0085-a-mutating-call-names-what-it-changed.md)
  rejected: `server.players.set`, `skin.equip`.
- **A permission per channel.** An install prompt nobody can read.
- **One verb set for every domain** (only `read`/`write`). `process:write` for
  the console and `network:write` for outbound HTTP read as something else; a
  descriptive verb says what the user is approving.
