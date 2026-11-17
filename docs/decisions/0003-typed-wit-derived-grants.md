# The API is typed WIT, one import interface per permission, and the grant is derived

*Applies to: [The plugin kernel](../architecture/plugins.md#packages-and-the-grant)*

**Decision.** The plugin API is WIT, one package per providing plugin, all at one version line. Every permission is
exactly one import interface, so a permission the grant lacks links as a stub that answers `denied(<permission>)`. The
grant (permissions, requirements, contributions) is read from the component's imports and exports; the manifest carries
only what code cannot say: scopes, optional marks, commands, schedules and `[dependencies]`.

**Why.** Every language gets types, and the internal wire stops being public. A component imports only what it calls and
exports only what it implements, so a manifest cannot ask for less or more than the code uses, and the prompt can say
what a plugin does. A function with no interface is unreachable by construction, so there is no deny list to maintain.

**Rejected.** `call(channel, json)`: untyped, and it ties plugins to the internal wire. One broad `hestia:api` package or
a dozen broad permissions with checks inside functions: a backup plugin would ask for everything. Declaring permissions
in the manifest and cross-checking the code: two sources of truth that can disagree.
