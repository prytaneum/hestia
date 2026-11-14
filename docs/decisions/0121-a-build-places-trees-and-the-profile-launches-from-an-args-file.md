# A build may place directory trees, and a server may launch from an args file

*Applies to: [Plugins](../architecture/plugins.md#provider-surfaces), [Minecraft providers](../architecture/minecraft.md)*

A loader that installs itself (NeoForge) produces a tree, not a file: a server's `libraries/` with the argument file that
names them, and library jars the game must find by path but must not load on `-cp`. The build outputs of
[0110](0110-a-flavor-builds-in-its-own-area.md) were single files named by the profile, and `vet_server` refused an
args-file launch, so such a flavor could not be written.

**An output may be a directory.** `BuildOutput` gains an optional `destination` (`entry` or `libraries`). Without one it
is the file of 0110. With one, `path` is a directory under `/build` and `target` the relative path under that
destination it is placed at (empty for the root). `entry` is the server's data directory; a client has none, so a tree
aimed at it is refused. The host copies a tree file by file, staged and renamed, never replaces a file that exists
(so the user's `server.properties` or world survives a build aimed at the data directory, and placing twice changes
nothing), and refuses the whole output for a symlink or special file inside it, a target that is not a plain relative
path, or a source outside the build area.

**A library says whether it is on the classpath.** `Library.classpath` defaults to true. One set false is still
materialised, and left out of `-cp`, so a patched jar does not shadow the module it patches.

**A server may launch from an args file, with a file per OS.** `ServerLaunch::ArgsFile` carries `{ path, alternatives:
[{ os, path }] }`, `os` in a rule's vocabulary. The host picks the alternative for the machine at launch, so no guest
probes the platform and core NeoForge drops its `cfg!(windows)`. Vetting requires every path to be a safe relative path;
the file's contents are the guest's, as the jar's already are under `flavor:provide`.

**The build area is keyed by the plugin**, `meta/flavors/<plugin-id>/`, not the flavor: one plugin's flavors share a
build tree (Paper and Folia download one tool) and one lock.

Not taken:

- **A `flavor` destination.** The build area is already where the guest works; placing a tree there is a copy onto
  itself. Added when a use appears.
- **Replacing existing files in a tree.** A data directory holds the user's files, and libraries are named by version.
- **Checking the args file exists after install.** The launch fails naming it; a check needs the launch in the install
  request.
