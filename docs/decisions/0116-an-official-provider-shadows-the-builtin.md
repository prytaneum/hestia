# An official provider shadows the built-in of the same id

*Applies to: [Plugins](../architecture/plugins.md), [Plugin author guide](../plugins/README.md)*

> Superseded by [0133](0133-a-provider-has-one-id.md): there is no shadowing, no bare registration and no `devOfficial`.
>
> Superseded in part for content sources: core has no built-in Modrinth or CurseForge, so an official plugin's bare `modrinth` or `curseforge` id shadows nothing there; only an official owner may still register a bare id.

Core features are ported into official plugins one at a time (adoptium, modrinth, fabric, paper, ...), and core keeps
each until a later cleanup. [0099](0099-official-plugins-are-signed.md) lets an official plugin register a bare id, but
the roster refused it as a `Duplicate` of the built-in, so a port could not be exercised incrementally, and a dev
install was never official, so it could not be tested before it was published and signed.

**An official provider shadows a built-in of the same id and kind.** Registration succeeds; the roster holds both
entries and lists one per id, the plugin's, in the built-in's place. The built-in is the fallback: when the plugin is
disabled, removed, fails to load or is disabled after repeated traps, the built-in is the provider again immediately.
Unregistering needs no bookkeeping beyond dropping the plugin's entries; a trap-disabled plugin is passed over through
a liveness check each registration carries. This is generic in `Roster`, not per kind. Only the official key shadows: a
non-official plugin is always namespaced and a bare id stays refused, and a bare id already held by another plugin is a
`Duplicate`, so two official plugins cannot fight over one id.

**Developer mode can mark a dev install official, for registration only.** `plugin dev --official` records
`devOfficial` on the plugin; while developer mode is on, that record's providers register as an official plugin's do.
Nothing else reads the flag. The grant, its approval digest, every permission check and the pins are computed and
enforced as for any install, `PluginInfo.official` stays derived from the signer, and a dev install has no signer, so it
gains no automatic approval, no unattended update and no bundled path. Turning developer mode off already disables dev
installs, which unregisters them.

**An update keeps who approved the install.** `approvedBy` is no longer reset on update: an update needing no new
approval keeps it, one the user approves (a widened grant or a moved pin, with a matching digest) becomes `user`, and a
bundled install stays `official`.

Rejected: keeping `Duplicate` and testing a port only after it is published, which makes every port a blind release; an
operation-level fallback that routes a guest's `unsupported` to the shadowed built-in's implementation, which needs
every adapter to know its built-in for a case the whole-provider fallback already covers; letting any plugin shadow
behind a permission, which would let a third party replace core behaviour; and a dev flag that also approves the grant,
which would make developer mode a way around the approval prompt.

Risk: an official plugin can change core behaviour that way. It already could once registered under another id and
selected by the user; the difference is that the id is the default. Only the official key can do it, and the built-in is
one disable away.
