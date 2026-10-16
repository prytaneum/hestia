# Config values pass one typed schema, and property values come from a curated table

*Applies to: [Servers & instances](../architecture/entries.md#records-and-settings)*

Every `config set` checked its own value its own way. The JVM keys and the backup
keys each had a parser and a `Reason` variant per rule, five of those variants
were never raised, and `server.properties` — by far the largest set of keys —
checked only that the key existed. `max-players abc` or `view-distance 900` was
written to the file and the game fell back silently. The create form restated
some of the Minecraft rules the daemon never enforced, so the CLI wrote what the
form refused.

Now every value passes one schema: a `KeyRule` is a key and a `KeyKind` —
boolean, integer with a range, choice with its values, memory, interval, JVM
arguments. `engine::keys::validate` is the one function that applies a kind, and
a refusal is `ErrorInfo::ConfigInvalid { key, reason }` with a generic `Reason`:
not a boolean, not a whole number, out of range with its `min` and `max`, not one
of a listed set — plus the memory, interval and JVM-argument format reasons that
were already there. A new rule is a row in a table, not a parser and a variant.

`server.properties` values come from a daemon-owned table of the properties whose
type is stable across versions (`engine::properties`). **This amends
[0025](0025-properties-schema-is-generated.md) for values only.** Which keys
exist for a server is still the schema derived from running that server; the
table never decides that. A key the table does not know — a property a new
Minecraft or Paper adds — is accepted as a string and logged at debug level, so
the table can lag the game without ever blocking the user. The table is the
cost 0025 declined to pay for keys: it can go stale, but staleness here only
means a value goes unchecked, never that a key is refused.

The rules ride on the result of `server.flavors`, `instance.flavors` and the two
`config.list` channels, and the create form builds its player-count and port
limits from them rather than hardcoding Minecraft's. The reason is a wire-visible
enum, so the front-end words it generically: a reason that carries data arrives
as `{ out_of_range: { min, max } }` and is resolved through the same token tables
as any other reason.

## Rejected

- **Per-key checks for a few properties.** It leaves the long tail unvalidated
  and keeps one place per key to forget.
- **Inferring types from the schema's defaults alone.** A default of `10` says
  `view-distance` is a number, not that it stops at 32, and a default of
  `easy` does not list the other difficulties.
- **Refusing a key the table does not know.** That would revive the stale-list
  failure 0025 was written to end.
