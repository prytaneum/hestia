# Instances are gated on a signed-in account, in the router

*Applies to: [The daemon](../architecture/daemon.md)*

A user cannot use — let alone play — Minecraft they do not own, and a stored
account already proves ownership (sign-in resolves the game profile). So the
whole instance surface is refused until an account exists: `Router::route`
checks a channel prefix (`instance.*`, plus the instance-only `sync.*`) and
answers a new `unauthorized` code before dispatch when
`accounts().has_account()` is false. The gate lives at the router rather than in
each handler for two reasons: it is a whole-domain lockdown (a per-handler
`require_account` across ~30 handlers is error-prone and easy to forget on the
next channel), and prefixing covers `instance.content.*` and
`instance.modpack.*` in their own service modules without touching them.
Catalogue reads shared with servers (`content.*`) stay open; only the
instance-prefixed channels are gated. Every front-end inherits the gate for
free — the CLI surfaces the
`unauthorized` message, and the desktop pairs it with a route-guard redirect, a
library sign-in overlay, and a first-run prompt.

## Amended: a verb both sides share is gated by its target

The prefix gate covered `instance.content.*` and `instance.modpack.*` only
because those verbs were forked per side, one `server.*` channel and one
`instance.*` channel for each. They are now one channel per verb (`content.add`,
`modpack.update`, …) whose params carry an `EntryTarget { kind, entry }`, and a
router that matches on the channel string cannot see which side the payload
names. Prefixing `content.*` instead would have locked the catalogue (`search`,
`project`, `versions`, …) that servers and signed-out users still need.

So the gate for those verbs moves into `guards::entry_for`, the single place a
handler turns a target into an entry. Its `Instance` arm checks
`accounts().has_account()` and answers `sign_in_required` *before* resolving the
entry, so a signed-out caller is told to sign in rather than that the instance
does not exist; its `Server` arm never looks at accounts. Every handler of the
shared verbs resolves through it, so the gate cannot be forgotten per handler,
which was the original objection to gating inside handlers.

The router's prefix gate is unchanged and remains the sole owner of the
instance-only channels (`instance.*`, `sync.*`), including
`instance.modpack.install`. The `content.*` catalogue reads and every `modpack.*`
channel stay out of that prefix list. The daemon's end-to-end test pins both
halves: an instance target on a daemon with no account is refused with
`sign_in_required`, a server target is not.
