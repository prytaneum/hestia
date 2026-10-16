# NeoForge's game jar is built, not downloaded — so a flavor can install

*Applies to: [Minecraft providers](../architecture/minecraft.md)*

NeoForge publishes no metadata service and no patched jar. Everything comes out
of its installer jar on `maven.neoforged.net`, read in-process with the `zip`
crate as a `.mrpack` index is: `version.json` is the launch profile,
`install_profile.json` names a chain of ten small Java tools, and running that
chain locally produces `net.neoforged:neoforge:<v>:{client,server}` — the jar
the loader actually runs. This follows theseus's technique but reads the
installer directly rather than Modrinth's pre-processed copy, keeping the
upstream-direct rule every other flavor follows and taking libraries from
NeoForged's own maven with their own checksums. Two things follow from that
choice, both normalised away before theseus sees them: the data table's
`/data/*.lzma` binary patches are extracted from the installer, and substitution
is side-aware (theseus is client-only and reads each entry's `client` value).

Building a jar is not something a profile can express, so the providers grew an
**`install` hook** rather than the launch flows branching on a flavor name. It
is idempotent on the patched jar's presence — the chain is minutes of JVM work —
and each processor is a cancellation checkpoint, leaving exactly what a failed
processor would.

The catalogue needs no service either: a NeoForge version *is* its game version
plus a build number, under two schemes split by Minecraft's move to calendar
versioning (`21.1.244` → 1.21.1; `26.2.0.35-beta` → 26.2, a zero patch or hotfix
dropping). The rule reproduces Modrinth's published manifest across all
published versions — artifacts included, since an April Fools' build maps to a
version that does not exist. Filtering the result against Mojang's manifest
drops it: a mapping naming no real version is a failed derivation, not a version
to offer.

**Semver build metadata outranks that arithmetic, which is where Modrinth is
followed no further.** NeoForge builds against a snapshot during a release cycle
and says so in the metadata field — `26.1.0.0-alpha.15+pre-3` is for
`26.1-pre-3` — while its leading fields still spell the release. Reading only
the fields filed all fifteen of those under 26.1, as Modrinth's manifest does,
and two failures followed. They were merged over the *release's* `version.json`,
so a resolve handed the loader a base jar it was never built against. And they
outranked the release line's own builds: "newest stable" was implemented as the
newest without `-beta`, which an `-alpha` trivially satisfies, so an unpinned
create on 26.1 skipped all nineteen betas for `alpha.15+pre-3`. Stability is now
read as semver reads it — any prerelease identifier, so a future `-rc` needs no
edit — and the metadata names the version a build targets. The snapshots those
alphas belong to are real Mojang ids, so they join the catalogue as snapshot
versions rather than being discarded; before this they were the reason
NeoForge's catalogue was 21 entries of which *every one* was a release, and a
front-end's include-snapshots toggle had nothing to reveal.

**The 1.20.1 line is a second catalogue source, not a parsing case.** NeoForge
forked from Forge at 1.20.1 and still publishes that line under the artifact it
forked into — `net/neoforged/forge`, versioned Forge's way (`1.20.1-47.1.106`),
with its own installer filename and its own args-file directory. So `versions`
reads two `maven-metadata.xml` documents and concatenates them, exactly as
daedalus's `fetch_neo` does, and each build answers which artifact it belongs to
from its own string: the legacy line leads with the game version, which neither
modern scheme ever does. Every path a build needs — installer URL, group, server
args file — derives from that one answer, so there is no second code path, only
a second constant. The installer format is identical (spec 1, the same data
table, the same ten processors, the `PATCHED` coordinate read from the profile
rather than assumed), which is what makes the existing chain handle it
unchanged. Two published versions whose installer 404s are refused by name, the
same two daedalus carries. The loader version stays the **raw** string rather
than Modrinth's stripped `47.1.106`: their manifest can afford the display form
because it keeps `raw` in a second field, where hestia's profile has one, and
the raw string is the one that reconstructs every path.

**NeoForge alone has no stability preference — its `-beta` describes the game
version, not the build.** Every other flavor resolves to its newest stable
build; NeoForge resolves to its newest build, full stop. This follows
modrinth/code, where daedalus marks *every* NeoForge build `stable: false` and
the create flow's stable/latest/other selector is therefore permanently disabled
for it. The rule it replaced was hestia's own and looked reasonable — prefer a
build with no prerelease identifier — but NeoForge leaves an entire line on
`-beta` for its lifetime (26.2: 34 builds, not one release), so a preference
either changes nothing or pins the entry to a months-old build on the versions
where it does bite. Across the live catalogue the two rules agree everywhere
today; the simpler one is kept because the suffix is not the signal it looks
like.

A **server** has no launchable jar at all. Its install generates an argument
file naming the module path, system properties and launch target — far past what
a command line carries — so `ServerProfile` gained `ServerLaunch::ArgsFile` and the server
runs as `java @libraries/net/neoforged/neoforge/<v>/unix_args.txt nogui`
(`win_args.txt` on Windows). The path stays *relative* because the file names
its own libraries that way and is only valid from the data directory. That
reordered provisioning: `server.properties` is derived by running the server
once, which a flavor that builds its server cannot do until the install has
finished, and the install needs the jar provisioning fetched. The three are now
ordered by the flow — fetch, install, derive — rather than nested, and `update`
follows the same order; both call one `Engine::materialize_server`. The vanilla
server jar stays the profile's `ServerJar::Download` even though it is never
launched: it is the input the processors patch, and keeping it there is what
makes materialising fetch it.

**The schema run therefore ignores the argument file.** A NeoForge server's
property schema used to be underivable — twice over: the generated argument file
resolves its libraries relative to the *data* directory, so it cannot run from
the throwaway dir, and FML gates on the EULA *before* vanilla writes
`server.properties` (vanilla writes it first), so even running it there yields
no file. Every NeoForge create then reported `PropertiesSchemaMissing`, a
warning about nothing the user did and nothing they could fix — its own hint
pointed at `update`, which failed identically.

The fix is what the profile already carries: the vanilla server jar. A
properties schema *is* the vanilla key set for that game version — the loader
contributes none, and no mods are installed at create — so `server_schema_plan`
drops the args file and boots the jar (`-jar server.jar nogui`),
which stops at the EULA gate having written the file, exactly as every other
flavor does. Nothing else in the pipeline changed, and the warning now fires
only for a run that genuinely failed (a timeout, a crash), which is a thing
worth saying.
