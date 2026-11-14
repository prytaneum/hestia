# A build is a plan the guest returns and the host runs

*Applies to: [Plugins](../architecture/plugins.md#provider-surfaces), [Minecraft providers](../architecture/minecraft.md)*

Builds of [0110](0110-a-flavor-builds-in-its-own-area.md) were imperative guest code: the guest called `exec` for each
tool and `download` for each artifact, and answered with outputs. Everything core's own Spigot build gets from the
process supervisor was missing. Output reached progress only when the guest narrated it, racing creates could not join a
build already running, a cancel was seen only where the guest looked, and a step already done was redone unless the guest
checked by hand. NeoForge's port was 1,252 lines of this.

**`build` answers a plan.** `proto::provider::flavor::BuildPlan` is `{ fetch, steps, outputs }`, `outputs` being the
`BuildOutput` list of [0110](0110-a-flavor-builds-in-its-own-area.md) and [0121](0121-a-build-places-trees-and-the-profile-launches-from-an-args-file.md).
`fetch` is files the host downloads into `/build` (`{ url, checksum?, dest }`). A step is `{ program, args, env, cwd?,
javaMajor?, creates, narrate? }`. The plan is data and crosses the ABI as JSON like every provider reply, so the WIT
does not change. A guest that did the work itself with `exec` answers only `outputs`, which is the old shape.

**The host vets the whole plan before anything runs.** A step names a declared program and must fit one of its
invocations, with the environment names it passes and the Java major, through `Programs::vet`, which is the check
`exec` applies (`authorize`), so a plan can do nothing a guest `exec` could not. An argument may refer only to the
mounts the build has (`${build}`, `${vanilla}`), so `${data}` is refused. `cwd`, every `creates`, every download
destination and every output are plain paths under `/build`; a download's URL goes through the plugin's `Policy` with
its checksum rule; the count of steps and downloads is bounded. A refusal fails the build as `PluginCallFailed` before the first download.

**Steps run under the process supervisor, not as guest children.** Each step is a `Task` of `ProcessSupervisor::run`,
the call core's BuildTools and NeoForge installs use. The environment is cleared and holds exactly what the chosen
invocation allows (`Programs::prepare`, shared with `exec`), the process is owned by `plugin:<id>`, and a cancel stops it
and its tree. The task id is `build-<plugin>-<version>-<step>-<hash>`, the hash covering the step's program, arguments,
environment, cwd and filter: a build still running under that id, from another create or from before a daemon restart,
is joined; a different plan never is. The id says (plugin, version) as the plan asked, and the hash is what keeps
the join safe.

**Output is narrated by a declared filter.** A step's `narrate` is `{ skipPrefixes }`: a trimmed line that is not blank
and starts with none of them is shown as the provision's progress, the rest stay in the captured log. It is
core's `is_step` as data, and `Task`'s narrator became a closure so both use one mechanism. A step with no filter narrates
nothing. Progress also carries the step number over the step count.

**A step whose `creates` all exist is skipped.** The check runs when the step's turn comes, so an earlier step may
satisfy a later one, and downloads are made only when some step has work to do (or the plan has no steps), so a client
launch with everything built costs one guest call and no transfer. A step that exits cleanly but did not create what it
declared fails the build, which catches a wrong path at once.

**Cancel is checked before every download and step** and stops the running one.

**Racing builds serialize on the plugin's area lock, then skip.** The lock of 0121 stays one per plugin, so two
versions never write one tree at once. Two creates of one version therefore do not overlap: the second takes the lock
when the first is placed, finds every `creates` present and runs nothing, then places its own outputs into its own
entry. Joining by task id covers what the lock cannot, a build that outlived its daemon.

**`exec` keeps working and shows its output live.** Each line a program prints reaches the call's progress as it
arrives (the newest, since progress keeps only the latest report), where it used to appear only on return.

Not taken:

- **A flavor-area destination.** The plan text proposed a third destination for a durable per-plugin folder. None of
  the host-run builds needed one: the build area is already per plugin and durable across builds, so a step's cached work
  tree and the paths its `creates` names live in `/build`. It stays unbuilt until a case appears.
- **One task for the whole plan.** A shell the host spawns to chain steps would be a program the plugin never declared.
- **Letting the guest narrate by regular expression.** A prefix list is enough for what core does and cannot be made to
  backtrack.
- **Releasing the area lock to let same-version builds overlap.** The saving is a second wait on the same result; the
  risk is two builds in one tree.

**Addendum, 2026-11-14: a plan may list 512 downloads.** The cap was 64, which a loader whose installer needs a hundred
libraries could not fit once the host, not the guest, did the downloading. It is 512; the step cap, every other bound
and each download's vetting (host, checksum, destination under `/build`) are unchanged.
