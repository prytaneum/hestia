# Sync is a closed catalogue, and each unit names its own mechanism

*Applies to: [Instances](../architecture/entries.md#sync--shared-settings-across-instances)*

Sync shares a **closed catalogue**: membership is decided here, never by a path
the user supplies. What changed is that a unit is no longer required to be a
*file the launcher can merge*. That rule admitted exactly four units — the game
options, the multiplayer list, the command history and the creative hotbars —
and excluded the two things people ask for most, because neither is a file whose
two copies can be merged at all.

A unit now declares which of three mechanisms it is.

| Mechanism | Units | What is shared |
|---|---|---|
| A merged file | `options`, `servers`, `commands`, `hotbars` | the file's contents, settled against a baseline ([0065](0065-sync-reconciles-against-a-baseline.md)) |
| A selection | `resourcepacks` | which packs are installed, enabled and in what order — never the file, and never a version ([0072](0072-a-pack-syncs-by-identity.md)) |
| An aggregate | `screenshots` | nothing is copied; the instances' folders are read as one listing ([0073](0073-screenshots-aggregate-rather-than-sync.md)) |

The merged-file rule survives inside its own mechanism: a file that can only be
copied whole makes the second instance to write lose its change, so a *file*
unit still has to be one the launcher can settle piece by piece — per key, per
row, per slot, per line.

What a unit shares is still not all-or-nothing. The options merge resolves per
key, so a key can be pinned local launcher-wide or on a single instance, and a
pinned key is carried through untouched on both sides. `servers.dat` settles per
row for the same reason. A pack selection settles per pack, and reaches only the
instances a pack is actually compatible with.

A unit is **off until it is enabled with a named source**. The shared copy has to
start as someone's, and a launcher that picks for itself defines everyone's
settings from whichever instance happens to launch first. Enabling seeds from one
named instance and records every instance's current content as its baseline, so
the first pass settles the shared copy's way. With several candidates and no
source named, the daemon refuses and hands back the candidates.

**Rejected:** folder targets — `config/`, or an arbitrary path. A directory tree
has no merge, and the two directories that tempted people into asking are now
served by mechanisms that fit them: packs by identity, screenshots by reading
rather than copying. A modpack's config tree still belongs to the instance.
**Also rejected:** an arbitrary-path escape hatch beside the catalogue. It can
only work by whole-file copy, which is the lost-edit behaviour the catalogue
exists to avoid, and it invites paths (`mods/`, `saves/`) that must never be
shared.
