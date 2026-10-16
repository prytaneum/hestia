# Spigot and CraftBukkit are compiled here, because no one may ship them

*Applies to: [Minecraft providers](../architecture/minecraft.md)*

Mojang's takedown means neither jar legally exists as a download: SpigotMC
publishes **BuildTools**, which clones the four upstream repositories,
decompiles the vanilla server, applies the CraftBukkit and Spigot patch sets and
compiles the result on the user's own machine. So these flavors take the
`install` hook NeoForge already established for a jar that has to be built, and
the same shape holds — the profile *names* the jar the launch plan runs
(`spigot-<version>.jar`) as `ServerJar::Built`, which is exactly what tells
`Engine::materialize_server` there is nothing to fetch. Deliberately no third-party
mirror fallback: the mirrors redistribute what the takedown covers, publish no
checksums, and would silently change the trust story mid-create.

**One build serves both flavors and every entry on that version.** A BuildTools
run is minutes of decompilation and maven over a few hundred megabytes of
clones, and it emits `craftbukkit-<v>.jar` *and* `spigot-<v>.jar` from the same
work — so building per-server would pay that cost again for a jar already on
disk. The work tree is therefore shared (`meta/spigot/`, with the outputs under
`jars/<version>/`), which is why `InstallRequest` grew a `meta` root beside the
entry-scoped `root`: an instance install already writes to `meta/`, and a
server's did not have it. The jar is copied from there into the entry's `data/`,
so a server still owns its own copy and the existing backup exclude/carry-over
rule (keyed on `jar.filename()`) needs no change.

**The catalogue is a filter, not a listing.** The hub indexes its version
metadata by Jenkins build number as well as by game version, so all but a few
dozen of the four thousand names it publishes are build numbers. Filtering
against Mojang's manifest is what leaves the game versions behind — the inverse
of Paper, where an unlisted name is kept as a snapshot. There is one build per
game version rather than a stream of them, so neither flavor has a loader
version to pin. The Java major comes from the hub's class-file range, narrowed
to a runtime the launcher can actually install so a mismatch fails at resolution
rather than at the Java step.

**The build is a supervised workload, not a bare child.** It drives git, maven
and a decompiler JVM, so it runs through `ProcessSupervisor::run` like a server
does: its output is captured to a file (`hestia process logs
build-spigot-<version>` reads it live), a cancel reaches the whole tree, and a
build that outlives a daemon restart is re-adopted rather than orphaned. Its id
is derived from the game version, so two creates racing on one version — or a
create after a restart — join the build already running instead of starting a
second.
