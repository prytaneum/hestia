# A flavor description declares its loader, its interchange keys and, optionally, only version ids

*Applies to: [Plugins](../architecture/plugins.md#providers), [Minecraft providers](../architecture/minecraft.md), [provider operations](../plugins/providers.md#flavors)*

A flavor was known to the rest of the launcher by its id alone. A content query filtered by the flavor id as if it were the
platform's loader name, a pack reader matched a loader to a flavor by the same id, and the `.mrpack` and Prism formats
each had a field of their own on the description (`mrpackKey`, `prismUid`), so a third format had nowhere to say how it
names a loader. A flavor's `versions` had to repeat Mojang's catalogue, kind and order for every id it listed.

**The description says what the flavor is called outside.** `loader` is the name a content platform gives the flavor's
loader and `compatible` the further names whose content it also runs; absent, the loader is the flavor's id.
`interchange` is an open map `{ format-id: key }` that replaces the two fixed fields: `mrpack`, `prism`, or the id of a
pack format a plugin contributes. Reading a pack asks the format's id for the flavor that owns the key; writing asks
for the key of the entry's flavor. The built-in flavors declare through the same fields.

**The host uses them where it used the id.** Picking a content version accepts a version carrying any of the loader and
its compatible names; a source query that names a single loader carries the declared one, and a client that sends a
flavor id as the loader is answered with that flavor's declared name. A pack's loader resolves to the flavor of that
id first, else to the flavor declaring it.

**`versions` may be bare ids.** The reply is a list of ids or a list of complete versions, never a mix. Bare ids are
typed and ordered by the vanilla catalogue the host already holds; an id the catalogue does not list is dropped, since
the host cannot build it on vanilla. The host never trusts the guest's own order for ids.

**An addition may override the Java major.** `ClientAddition` and `ServerAddition` take an optional `javaMajor`, which
replaces vanilla's. It is refused unless it is a major the launcher can provide.

**A declaration is untrusted.** A loader or format id is at most 64 characters of `a-z`, `0-9`, `-`, `_`, `.`; at most 8
compatible loaders and 16 interchange entries; a key is at most 128 characters without control characters; a version
list at most 4096 entries of at most 64 characters. A flavor that breaks one is not registered. An interchange key one
flavor already claimed stays with it, and the roster lists the launcher's own flavors first, so a plugin flavor cannot
capture the loader of another.

**Rejected.** Compatible loaders on the wire queries (`SearchQuery.loader`, `VersionQuery.loader` stay single): the
provider contract and both front-ends would change, so browsing filters by the declared loader and compatible names
apply to the host's own version picking. Mixing bare ids and complete entries in one list: it would need a merge order
the guest could abuse.
