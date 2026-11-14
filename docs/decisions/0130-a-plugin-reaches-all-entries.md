# A plugin's entry access is every entry

*Applies to: [Plugins](../architecture/plugins.md#permissions), [plugin permissions](../plugins/permissions.md)*

The scoped grant of [0117](0117-a-grant-is-permission-to-scope.md) can narrow a permission by any kind of scope, and a
plugin's access to servers and instances is the obvious candidate: a plugin that manages one server could be granted
that server alone. It is not. `entry:read`, `entry:write`, `process:read` and `process:control` reach every server and
instance, and no scope kind names an entry.

The features that justify the plugin API read across entries: the players plugin lists the whitelist and operators of
every server, and core's own features are not entry-bound. Scoping by entry would turn each into a prompt that lists
entries by id, ids the user did not choose and a plugin cannot know before the user creates them. A grant also outlives
its entries: a server created next week would either fall outside it or need an approval for something the plugin never
asked about. Access to all entries keeps the prompt to what the plugin does, not where.

`entry:read` has one narrowing, and it is about files, not entries. `files` lists paths of a server's directory that
`server.files.read` may return, matched exactly, with `server.properties` never allowed because it holds the RCON
password ([0092](0092-a-contract-names-the-permission-a-plugin-needs.md)). The scope says which files of any server, not
which servers.

Rejected:

- **Entry-scoped grants.** Precise, but the grant would name things the user cannot recognise at install, and every new
  entry would reopen the question.
- **A `files` scope that lists server and path together.** The same problem, with a longer string.

Revisit it if a plugin appears whose whole job is one server and whose users want that stated. A new scope kind is a
variant and an entry rule ([0117](0117-a-grant-is-permission-to-scope.md)); nothing in the grant would need to change.
