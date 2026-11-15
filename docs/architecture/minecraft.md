# Minecraft providers

*[← Architecture](../architecture.md)*

The `minecraft` subsystem answers three questions: **which distributions can you
run**, **which versions of each**, and **what exactly does launching one
require**. Everything downstream — provisioning a server, launching an instance —
consumes its answers.

It holds no state of its own, but it does write regenerable files under `meta/`:
the catalogue readers keep the last good copy of each response in
`meta/catalogue/` (the offline fallback). `ResolveRequest` carries the `meta` root
and the cache for a flavor that keeps what it resolves from.

## Flavors

A *flavor* is a distribution of Minecraft, and an **edition** (`java` or
`bedrock`) comes before it: there is one registry, and a lookup is by `(side,
edition, id)`, so Bedrock's `vanilla` is not Java's. A flavor declares which
sides it serves, and a lookup for the other side fails as an unknown flavor.
Instances are Java only
([0134](../decisions/0134-the-edition-is-the-first-axis.md)). The vanilla flavor
of each edition is the only built-in; every other distribution comes from a
plugin, and a plugin's flavor is a Java one. The official plugins `fabric`,
`neoforge`, `paper` (Paper and Folia) and `spigot` (Spigot and CraftBukkit)
register as `fabric.fabric`, `paper.paper` and so on; `fabric` alone is the
shorthand the roster resolves ([0133](../decisions/0133-a-provider-has-one-id.md)).

| Edition | Flavor | Server | Client | Loads | Notes |
|---|---|:--:|:--:|---|---|
| java | `vanilla` | ✓ | ✓ | — | Mojang's own, built in |
| bedrock | `vanilla` | ✓ | | — | Mojang's Bedrock Dedicated Server, built in; a native executable, no Java |

The registry is runtime-mutable: `Minecraft::register(owner, standing, provider)`
and `unregister(owner)`, with the same ownership rules as content sources (the
built-ins are permanent; any other id reads `<owner>.<id>`).

A provider names itself in one `FlavorInfo` (`id`, `name`, `summary`, the
content its loader consumes as `loads`, the prerequisites it needs on this
machine as `requires`, and its `sides`), lists the game versions it supports, and
*resolves* a request into a server or client launch profile.

Two things follow, and both exist so that shipping a flavor is a daemon-side
change alone:

- **A flavor describes itself on the wire.** `Flavor` carries the `summary` a
  picker renders and the `accepts` set an entry of it would have — no front-end
  keeps a table ([0008](../decisions/0008-flavor-declares-accepted-content.md)).
- **A flavor states what it needs before you commit to it.** A flavor that
  builds on this machine can need a program such as `git`; the catalogue
  resolves missing prerequisites at build time, each with a name you would
  recognise and where to get it ([0006](../decisions/0006-flavor-states-its-requirements.md)).

`entry.flavors` answers one `EditionCatalogue { edition, rules, flavors, nameKey }`
per edition the side offers (a server two, an instance one). The edition's
property `rules` and the `server.properties` key a server is listed under
(`nameKey`) ride on it, so a picker is built from the answer alone. What differs
*between* editions is not in the providers: it is in the engine's `edition/`
module, a closed `match` that each flow asks
([0134](../decisions/0134-the-edition-is-the-first-axis.md)).

Adding a flavor is a plugin. The content flows never change, because what an
entry accepts is composed from the provider's `loads` plus what the side reads
for itself.

A plugin adds one as a `flavor` provider (`proto::provider::flavor`, asked through
`provider.invoke`), wrapped in a
`PluginFlavorProvider` (`engine/src/plugins/providers/flavor.rs`) and registered
as `<plugin>.<id>` when the plugin is enabled. A flavor decides the code the game
runs, so it needs the unrestricted `flavor:provide` permission, and the guest
only describes: every profile it returns is vetted before the engine acts on it.
Each artifact must carry a sha1 and sit on one of the plugin's network hosts,
every path must stay relative, the Java major must be one the launcher knows,
and the jar must be a download or named as built. One refusal fails the whole
resolve. A plugin flavor builds what its profile names as built through the
optional `build` operation, whose plan the host runs from the `install` hook in an area of its own and
places ([0110](../decisions/0110-a-flavor-builds-in-its-own-area.md),
[0124](../decisions/0124-a-build-is-a-plan-the-host-runs.md), [Plugins](plugins.md#provider-surfaces)). The profile types live in
`proto::profile` so the SDK can build them.

Two pieces the vanilla flavor uses are shared with plugins
([0119](../decisions/0119-downloads-are-host-side.md)). `minecraft/maven.rs` is the one parser of
`group:artifact:version[:classifier][@ext]`: the path under a repository, the filename and the URL come from it, and
a part with a character a Maven name lacks, or made only of dots, does not parse. `minecraft/rules.rs` is the one
evaluator of Mojang `rules` (the last applicable rule decides, no rules allow, a rule naming `features` is skipped, and
`os.name` and `os.arch` are compared against the host's `windows`/`osx`/`linux` and `x86`/`x86_64`/`arm64`;
`os.version` is not evaluated). Vanilla's manifest libraries and arguments, and a plugin's, are filtered at resolve
time, so a persisted profile is for the machine that resolved it.

## Profiles — what a launch needs

`resolve()` turns *(flavor, version, optional loader version)* into a
**`ServerProfile`** or **`InstanceProfile`**. A server profile names its jar
(`Download(artifact)`, or `Built { filename }` when the install hook produces it),
how it launches (`Jar`, or `ArgsFile` for a flavor whose install generates one, with a file per OS; a flavor layered over vanilla may name it too),
the required Java major and the flavor's recommended JVM args. That is a
`ServerRuntime::Java`; a Bedrock profile is `ServerRuntime::Native { archive,
executable, env }` — a zip, an executable and the environment it needs
(`LD_LIBRARY_PATH=.` on Linux) — with no Java major and no JVM args, so nothing
can apply Java settings to it
([0135](../decisions/0135-a-server-runtime-is-java-or-native.md)). A plugin's
flavor may only answer the Java runtime. An instance
profile carries the client jar, libraries, asset index and arguments. The
profiles are engine types, stored on the entry records; no channel hands one to a
client.

```mermaid
flowchart LR
    REQ["flavor + version<br/>+ loader version?"] --> RES["resolve()"]
    META["minecraft/meta/<br/>mojang · bedrock"] --> RES
    RES --> PROF["ServerProfile / InstanceProfile"]
    PROF --> INST["install hook<br/><i>only if the jar must be built</i>"]
    PROF --> MAT["materialize<br/><i>ensure files on disk</i>"]
    INST --> MAT
    MAT --> PLAN["launch → LaunchPlan<br/>program · args · cwd"]
```

Manifest parsing lives in `minecraft/meta/` — Mojang's piston-meta, deserialized
in its own JSON shape rather than pretending it is ours.

**Mojang's manifest is the ordering ground truth.** Whether one version is newer
than another — which decides whether an update is a downgrade — is judged by
position in Mojang's catalogue, never by parsing version strings. It also
supplies release/snapshot status that most upstreams never state.

## The launch pipeline

Three modules sit over the profiles:

**`materialize`** idempotently ensures profile pieces on disk, skipping what is
already there:

| Piece | Where it lands |
|---|---|
| single jars | the entry's `data/`, or `meta/versions/` |
| libraries | Maven layout under the shared `meta/libraries/` |
| assets | content-addressed — `meta/assets/indexes/<id>.json` + `meta/assets/objects/<hh>/<hash>` |
| natives | unpacked into `meta/natives/<version>/`, from jars fetched into the libraries root |

Two shapes belong to the pre-1.19 era. A **native** is a library the JVM loads
through `java.library.path` rather than the classpath: those manifests ship one
classifier jar per OS, and `materialize` unpacks the host's into the natives
directory (the era after them ships the same binaries as ordinary rule-gated
libraries, so the list is simply empty). An asset index flagged `virtual` or
`map_to_resources` belongs to a client that reads assets *by path*, so the hashed
store is mirrored to a named tree — under `meta/assets/virtual/<id>/`, or the
instance's own `resources/` for the oldest — and `${game_assets}` points the
game at it.

Downloads are verified through `Downloader` wherever upstream publishes a hash, with a bounded number of
concurrent fetches. Because the roots are shared, a second instance on the same
version costs almost nothing.

**`launch`** is pure assembly of a **`LaunchPlan`** (program, args, cwd):
classpath joining and Mojang `${placeholder}` substitution for auth, paths and
names. No I/O — which is what makes it unit-testable, and it is unit-tested.
The per-entry memory and `jvm-args` settings it receives (`JavaSettings`, whose
memory check queries the host) live in the engine's `jvm` module, outside it.

**`rcon`** is a minimal RCON client — connect, authenticate, then commands whose
multi-packet replies it reads to the end. It is the server console's transport,
chosen because it is re-establishable state that survives a daemon restart,
where a stdin pipe is not
([0055](../decisions/0055-the-console-is-rcon-not-a-pipe.md)). The engine's
`console` module keeps one connection per server and reads command completions
out of the server's `help`
([0077](../decisions/0077-the-console-keeps-its-connection.md)). A Bedrock server
has no RCON; its console is the supervisor's stdin endpoint
([0136](../decisions/0136-a-bedrock-console-is-a-reattachable-stdin-endpoint.md)).

**`world.rs`** reads a save's own `level.dat` (gzipped NBT, via `fastnbt`) so a
world can describe itself rather than being reduced to a folder name
([0022](../decisions/0022-a-world-describes-itself.md)).

**`log4j.rs`** generates the per-session logging config each instance launch runs
under — Log4Shell-safe by construction
([0039](../decisions/0039-per-session-log4j-config.md)).

**`ping.rs`** speaks Server List Ping to a managed Java server on loopback or to a
host from an instance's multiplayer list. **`servers.rs`** reads and edits that
list (`servers.dat`, uncompressed NBT the running game rewrites on exit, keeping
tags Hestia does not know). **`quickplay.rs`** reads the Quick Play log a 1.20+
client writes, which names the world, server or Realm a session last joined.

## Bedrock Dedicated Server

`minecraft/bedrock/` is the one Bedrock provider. Installing unpacks Mojang's zip
over the entry's `data/`, keeping the files a user owns, and copies the zip's own
`server.properties` aside as the properties schema. The version list is Mojang's
download links merged with the Bedrock-OSS feed, with preview builds as
snapshots, and downloads are pinned to `www.minecraft.net`
([0137](../decisions/0137-the-bds-catalogue-merges-mojang-and-the-community-feed.md)).

BDS 1.26 serves only NetherNet, so Hestia writes `transport=nethernet` before
every start and there is no RakNet status ping; a server's status comes from its
console ([0138](../decisions/0138-bedrock-pins-nethernet.md)). Its worlds are
LevelDB with a little-endian `level.dat`, read by a small native NBT reader in
`bedrock/nbt.rs` rather than `fastnbt`, which is big-endian Java only.

## Flavors that build their own jar (plugin-provided)

A flavor cannot always download the thing it runs (a NeoForge client's patched
jar, a Spigot server nobody may redistribute), so providers have an **`install`
hook** rather than the launch flows branching on a flavor name. It is idempotent
on the built jar's presence, and each step is a cancellation checkpoint. Core
implements none: a plugin flavor's `build` operation returns a plan the host runs
in an area of its own, and the official `neoforge` and `spigot` plugins use it
([0004](../decisions/0004-neoforge-builds-its-own-jar.md),
[0007](../decisions/0007-spigot-is-compiled-locally.md) record the original
in-core design).

A flavor may recommend JVM flags in its profile. Those flags are the **last**
fallback, beneath the entry's own `jvm-args` and the launcher-wide
`defaults.jvm-args` — and `server info` names which layer supplied the flags a
process is actually running with
([0009](../decisions/0009-flavor-recommends-jvm-flags.md)).

## Decisions

- [0004 — NeoForge's game jar is built, not downloaded](../decisions/0004-neoforge-builds-its-own-jar.md) (now the `neoforge` plugin)
- [0005 — A Paper build is a loader version, and Mojang orders the catalogue](../decisions/0005-paper-build-is-a-loader-version.md) (now the `paper` plugin)
- [0006 — A flavor states what it needs, before the user commits to it](../decisions/0006-flavor-states-its-requirements.md)
- [0007 — Spigot and CraftBukkit are compiled here](../decisions/0007-spigot-is-compiled-locally.md) (now the `spigot` plugin)
- [0008 — What an entry takes is a property of its flavor](../decisions/0008-flavor-declares-accepted-content.md)
- [0009 — A flavor may recommend JVM flags; the user still outranks it](../decisions/0009-flavor-recommends-jvm-flags.md)
- [0134 — The edition is the first axis, and one module owns what each edition does](../decisions/0134-the-edition-is-the-first-axis.md)
- [0135 — A server runtime is Java or native, and a native one never meets Java](../decisions/0135-a-server-runtime-is-java-or-native.md)
- [0137 — The BDS catalogue merges Mojang's links with the Bedrock-OSS feed](../decisions/0137-the-bds-catalogue-merges-mojang-and-the-community-feed.md)
- [0138 — Bedrock pins NetherNet, and status comes from the console](../decisions/0138-bedrock-pins-nethernet.md)
