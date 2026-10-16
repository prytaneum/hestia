# Installed content is managed-dir-of-record, mirrored into `data/`

*Applies to: [Content & modpacks](../architecture/content.md)*

A mod is written to the entry root's `mods/` (hestia's namespace) with its
provenance in `content.json`, then hardlinked/copied into `data/mods/` (what the
game loads). The managed copy — not the one in `data/` — is the source of truth,
which pays off three ways: (1) a backup restore swaps `data/` but the managed
dirs live outside it, so `mods/`/`resourcepacks/`/`shaderpacks/` are added to
the backup exclude/preserve set and a `sync` pass re-mirrors them at the next
start/launch (`server_launch_plan`, `prepare_instance`) — restore heals itself
and archives stay world-focused. For a server's mods and plugins the preserve
list alone already carries them over from the live tree; the mirror is what
heals a datapack, which lives inside the world archive
([0016](0016-datapacks-are-world-of-record.md)); (2) provenance survives, so `update` knows each
item's project and current version (Prism keeps the same metadata in packwiz
TOML sidecars — same idea, one index file); (3) a hand-dropped jar in
`data/mods/` is surfaced as *untracked* rather than silently adopted. Installs
run through a `ContentManager` mirroring `BackupManager` (job id, per-entry
in-flight key, `content.progress|done|error` topics) and are refused on a
running entry (open jars lock on Windows; changes only apply at the next start)
or during a backup/update.
