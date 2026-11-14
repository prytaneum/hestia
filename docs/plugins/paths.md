# Guest paths and mounts

A plugin sees no host path. Its filesystem is a few directories the host mounts at fixed guest paths for one call, and
nothing else exists: no `/`, no home, no other plugin's files. TCP, UDP and name lookup are off and the environment is
empty. The mounts are WASI preopens, so a path outside them does not resolve at all.

## The mounts

| Guest path | Host directory | Access | Present |
|---|---|---|---|
| `/data` | `plugins/<id>/data/` | read-write | every call |
| `/build` | `meta/flavors/<plugin-id>/`, the plugin's build area | read-write | a flavor's `build` |
| `/vanilla` | a host-owned copy of the side's vanilla jar, alone | read-only | a flavor's `build`, when the side has a jar |
| `/files` | the files being identified, as `/files/0`, `/files/1`, ... | read-only | a content source's `identify` |

Every call gets a fresh instance and its mounts are made for it, so a mount of one call is gone in the next. `/data` is
the only place state outlives a call ([data.md](data.md)). `/build` is shared by every entry and every flavor of the
plugin and survives calls, which is what lets a build skip work it already did
([0121](../decisions/0121-a-build-places-trees-and-the-profile-launches-from-an-args-file.md)). `/vanilla` holds the jar
and nothing beside it, because a server's own directory holds `server.properties` and its RCON password. `/files` holds
links (or copies, across filesystems) of just the files asked about, named by position, and is removed after the call
([0122](../decisions/0122-a-source-declares-its-setting-and-one-vet-checks-every-download.md)).

The constants are `DATA_MOUNT`, `BUILD_MOUNT` and `VANILLA_MOUNT` in `proto::plugin`.

## How a path from the guest is checked

The guest reads and writes the mounts with `std::fs`, and the sandbox confines that. A path the guest hands the host,
as an argument of a host call, is checked by the host in addition.

- **A working directory** (`exec_in`, `exec_with`'s `cwd`) and **a download destination** (`download`) must be at or
  under a *writable* mount of the current call. The path is split into components and every one must be a plain name, so
  `..`, `.` and an empty component are `denied`. The directory is then resolved, and the result must still lie inside the
  resolved mount root and be a directory, so a symlink the guest planted cannot lead out. A download's file name must be
  one plain component (no `\` or `:`), the directory must exist, and a directory of that name is refused. A read-only
  mount is never a destination.
- **A placeholder** in an `exec` argument (`${data}`, `${build}`, `${vanilla}`) is matched against the program's argument
  patterns as written, then replaced by the host path of that mount of the call. A placeholder for a mount the call does
  not have is `denied`.
- **A build plan** is vetted whole before anything runs: `cwd`, every `creates`, every download `dest` and every output
  must be a plain path under `/build`, and an argument may name only `${build}` and `${vanilla}`
  ([0124](../decisions/0124-a-build-is-a-plan-the-host-runs.md)).
- **A build output** is read from the build area by the host: it must exist, must not be a symlink, and must resolve
  inside the area. The destination it is copied to comes from the profile (a built jar's name, a library path) or, for a
  tree, a plain relative `target` under the `entry` or `libraries` destination, never from a guest path
  ([0110](../decisions/0110-a-flavor-builds-in-its-own-area.md)).
- **A file of a package** (`assets/`, `themes/`) is served by the daemon, never mounted, after a check that it is
  relative, canonical and beneath those directories ([0098](../decisions/0098-plugin-assets-are-served-by-the-daemon.md)).

A guest can never write outside `/data` and `/build`, and a program that `exec` starts runs with `data/` as its working
directory unless it names another of those.

## What a `files` scope narrows

The `files` scope of `entry:read` is not a mount. A plugin has no view of a server's directory at all; it asks the daemon
for a file with the `server.files.read` channel, which names the server and a path relative to the server's directory.
The channel's contract says `NARROWED_BY` is `files` and reads the path it touches, and the plugin gate checks that path
against the entries of the caller's scope before the handler runs
([0117](../decisions/0117-a-grant-is-permission-to-scope.md)).

- An entry is a relative path matched against the call's path **by exact string**, so it must already be normalised: no
  leading `/`, no `\` or `:`, no `.`, `..` or empty segment, no glob character. `./ops.json` matches nothing and is
  refused at install.
- `server.properties` can never be listed, whatever its case, because it holds the RCON password.
- The scope says which files of *any* server, not which servers ([0130](../decisions/0130-a-plugin-reaches-all-entries.md)).
- A call whose payload does not decode is refused rather than let through.

The other scope kinds have no path semantics: `hosts` and `unhashed` match domain names, `endpoints` match local socket
names, and `programs` and `properties` name ids and keys ([permissions.md](permissions.md)).
