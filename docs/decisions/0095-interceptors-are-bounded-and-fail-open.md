# Interceptors are bounded and fail open, so a hook is customisation and never a control

*Applies to: [The engine](../architecture/engine.md)*

A plugin may intercept an operation just before it happens at a hook point: `before-launch`, `before-server-start` and
`before-content-install` ([0091](0091-plugins-are-wasm-components.md)). A point is data in `proto::hook`, asked through
the one `hooks.intercept` export, so a new point changes neither the ABI nor these rules
([0106](0106-the-plugin-api-is-mechanisms-not-features.md)). The operation belongs to the user, so a
misbehaving plugin must not be able to break it, and a plugin must not be mistaken for a guard on it.

**A hook fails open.** `Hooks::run` calls each plugin under a 2 s deadline, on a fresh store with the same wall-clock
and epoch limits as every other call. A timeout, a trap or a refused re-entry skips that plugin and adds
`WarningInfo::PluginHookFailed { plugin, hook }` to the operation's result
([0026](0026-degraded-outcomes-ride-on-the-result.md)); the launch or install goes ahead without it. Repeated traps
disable the plugin as for any call.

**Only a veto stops the operation.** A plugin that answers with a veto ends the run at that plugin, later plugins are
not called, and the operation fails with `ErrorInfo::PluginVetoed { plugin, hook, reason }`. A veto is the plugin
saying no on purpose, which is why it is an error and a crash is not.

**Hooks run in plugin-id order, and patches fold in that order.** The id is stable across reinstalls and updates,
unlike install order, so the same set of plugins gives the same launch. Only enabled, loaded plugins whose grant lists
the hook are called.

**No hook is a security control.** Because a hook fails open, a plugin that is slow, broken or simply disabled lets
the operation through. So nothing documents or presents a hook as a policy, a scanner or an enforcement point; the
content hook is a convenience to warn about a known-incompatible mod, not a malware check. Whatever must hold
regardless of plugins is enforced by the host itself (the launch-patch allowlist, the sandbox, the permission gate).

**Each hook names the permissions it needs**, checked when the manifest is validated and part of the grant:
`launch:patch` and `account:read` for launch and server-start, `content:read` alone for content install. All are
sandboxed tier, so a hook plugin is not presented as full access to the computer.

**The three hooks** share that machinery and differ in what they may answer:

| Hook | Called | May answer |
|---|---|---|
| `before-launch` | once per instance launch, after the plan is assembled | an allowlisted patch, or a veto |
| `before-server-start` | once per server start, before spawn | the same patch with its game arguments ignored (a server has no window), or a veto |
| `before-content-install` | once per file about to land in an entry — a platform install and each required dependency, a local import, an update or version change, and each managed file of a modpack (one bundled in its overrides is checked as it is unpacked and removed if refused) | a veto of that file only, never a write |

A refused file fails its own item and the rest of the batch installs, and a plugin that is skipped leaves a warning on
the result; the veto's reason is prefixed with the file name
(`fabric-api.jar: …`) so the user can tell which dependency was refused. The hook receives names, ids and the sha1, not
the file's bytes. It is a convenience check and **not a security scanner**: it fails open like every hook.

**Where the claim stands** ([0086](0086-an-entry-has-one-claim.md)). An instance launch holds no claim, so its hook never
stalls one. A server start and a content install do hold the entry's claim while the hook runs, because the claim is
what keeps the entry stopped (or untouched) between the check and the act; the cost is at most 2 s per plugin on that
entry alone.

**Rejected.** Failing closed on a timeout or trap (one broken plugin could then block every launch, and the user would
have to find and disable it before playing); ordering by install time (a reinstall would reorder patches).
