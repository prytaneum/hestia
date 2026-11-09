# The edition is the first axis, and one module owns what each edition does

*Applies to: [Minecraft providers](../architecture/minecraft.md), [Servers & instances](../architecture/entries.md)*

Bedrock Dedicated Server differs from a Java server at nearly every seam: its
runtime, console, player files, properties, backups, ports, worlds and the
content it loads. The first cut tagged each flavor with an edition and let the
flows check it, which put a `match` on the edition in every flow and left a
flavor id such as `vanilla` ambiguous the moment two editions shipped one.

**The edition is chosen first, and a flavor is chosen within it.** On the wire,
`entry.flavors` answers `editions: [EditionCatalogue { edition, rules, flavors,
nameKey }]`; `entry.versions`, `entry.loaders` and `server.create` carry the
edition beside the flavor id. The engine's registry is keyed by `(side,
edition, id)`, so Bedrock's flavor is `vanilla` under `bedrock` and Java's is
`vanilla` under `java`. Instances are Java only. A flavor's id is unique only
within its edition.

**`ServerRecord.edition` is stored at create and is the one source of truth.**
Nothing derives an edition from a flavor id afterwards.

**One engine directory, `edition/`, owns every per-edition decision**: property
rules and managed keys, the player lists and their commands, console transport,
how the supervisor runs the process, status, backup pause and resume, ports,
worlds and what content an entry accepts. `edition/mod.rs` is a closed `match`
over a two-variant enum, delegating to `java.rs` and `bedrock.rs`. A flow asks
`edition::…` and never matches on an edition itself, so an edition-level feature
is one change in one place and a provider only describes how its distribution
is fetched. The heavy parts (the stdin console, the `save query` parser, the
little-endian NBT reader) stay where they are and are called from there.

`EditionCatalogue.nameKey` names the `server.properties` key a server is listed
under (`motd` or `server-name`), so a front-end labels the field from the daemon
rather than from its own table.

**Rejected.** A separate `bedrock` entry domain, which would duplicate every
server channel, CLI verb and desktop page. An open `Edition` trait: there are
two editions and the set changes with the game, not with plugins, so a closed
enum keeps the compiler checking every site. Keeping the edition on the flavor
alone, which left the lookups ambiguous and the record dependent on a registry.
