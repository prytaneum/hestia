# A flavor builds in an area of its own, and the host places what it builds

*Applies to: [Plugins](../architecture/plugins.md#provider-surfaces), [Minecraft providers](../architecture/minecraft.md)*

Some flavors cannot name a download for everything they run. One patches the client jar by running installer
processors on Java; another compiles a server nobody may redistribute with a build tool that needs Java and `git`. A
plugin flavor could only name downloads: the host refused a built server jar, and a library with no URL had nowhere to
come from. The first design for closing that gap (a `java.run` import and a permission that preopened `meta/libraries/`
writable) was shaped like those two flavors, and a writable libraries root hands one plugin the files every other
launch reads ([0106](0106-the-plugin-api-is-mechanisms-not-features.md)). Four general mechanisms replace it.

**`build` is an optional flavor operation.** `proto::provider::flavor::Build` takes `{ side, version, loaderVersion?,
vanillaJar? }` and answers `{ outputs: [{ path, target }] }`; `unsupported` means the flavor has nothing to build, and
the host stops asking once it has said so. The host calls it from the flavor's `install` step, for the side being
provisioned, on every client launch and every server provision. It is idempotent: the guest checks its own outputs and
answers cheaply when they exist.

**The build area is the flavor's own, mounted for that call only.** `/build` is read-write and is
`meta/flavors/<flavor-id>/`, created by the host, shared by every entry of the flavor and regenerable like the rest of
`meta/` ([0054](0054-meta-root-for-materialised-files.md)). `/vanilla` is read-only and holds a host-owned copy of the side's
vanilla jar and nothing else (a server's jar sits beside `server.properties` and its RCON password, so its directory is never mounted), which `vanillaJar` names; a server whose own jar is built has none. No other call is given either mount, and
no entry directory, `meta/libraries`, `meta/versions` or `meta/assets` is ever writable to a guest. The mounts ride on a
per-call context beside the progress sink, so the store builder has one place that knows about extras.

**The host places outputs; the guest never writes outside `/build`.** Each output names a file under `/build` and a
target the profile already declared built: a server's `ServerJar::Built` filename, which is copied into the server's data
directory, or a client `Library` marked `built`, copied to its path under `meta/libraries/`. A target the profile did not
declare, a path outside `/build`, a symlink and a path that resolves out of the area are refused; the copy is staged and
renamed ([0024](0024-temp-artifacts-are-reclaimed-at-startup.md)); and afterwards every built file the profile names must
exist, or the provision fails naming the flavor. An existing destination of the same length is left alone, since the
libraries are named by version and a launch must cost nothing once built.

**A built artifact is marked in the profile, not signalled by what is missing.** `Library.built` says the file is produced
locally, so it needs no URL or sha1 and the download plan skips it; every other artifact keeps the host and sha1 rules.
Core NeoForge never named its patched jar in the profile, so there was no marker to reuse. `ServerLaunch::ArgsFile` stays
refused.

**A program may name a managed Java instead of a command.** `[[programs]]` takes `java = <major>` in place of `command`,
exactly one of the two. The host resolves it when the program is called, to the Hestia-managed runtime of that major,
installing it through the engine's own path when missing. Nothing is pinned at approval because the runtime is host-owned
and replaceable; the pin exists to stop a `PATH` change substituting a binary, and a managed runtime is not found on
`PATH`. It is still a declared program: `process:spawn`, argument patterns, an empty environment plus declared names
([0101](0101-plugins-spawn-only-declared-programs.md)). The grant and the approval prompt show it as "Hestia-managed
Java 21". The plugin runtime holds a `ManagedJava` resolver lent at load, so it never holds the engine.

**`process.exec` takes a working directory.** `cwd` is a guest path at or under a writable mount of the current call:
`/data`, or `/build` during `build`. `none` keeps `data/`; anything else, a `..` component, a missing directory or a
symlink that leaves the mount is `permission-denied`. `spawn` is unchanged.

**Progress and cancel are not specific to commands.** The `progress` import reads whatever sink the call was given. A
`build` maps `report` onto `ProvisionProgress` (phase `Libraries` for a client, `Server` for a server) and `cancelled`
onto the provision's own cancel, under the command timeout (an hour) and the same grace before the call is dropped.

**Interchange formats read the loader names from the flavors.** `FlavorDescription` and the engine's `FlavorInfo` gain
`mrpackKey` and `prismUid`; the Modrinth pack and Prism readers and the pack writer look a flavor up through the
registered ones instead of keeping tables a new flavor would have to edit. A pack naming a loader no flavor claims still
reads as that loader and is refused where the entry is created; a Prism component no flavor claims is scaffolding, as
before.

No permission is added: the build area is the flavor's own and running programs is already `process:spawn`.

Rejected:

- **`java.run` and a writable `meta/libraries`.** Shaped like two flavors, and a writable shared root lets one plugin
  corrupt what every launch reads.
- **A host `git` or BuildTools step.** The host names no tool: a flavor that needs `git` declares it as a `PATH` program
  and passes `PATH` in `env`.
- **Letting the guest write the server jar into the entry.** The entry directory is the user's; the host copies one named
  file there after checking it.
- **Pinning a managed Java at approval.** A runtime update would force a re-approval for no safety gain.
