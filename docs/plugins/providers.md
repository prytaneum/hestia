# Providers

A provider fills one of the launcher's open registries: a content source, a flavor (a game distribution), a pack format
or a Java distribution. A plugin declares each one in the manifest, and the host asks it questions as the user
searches, creates, imports or installs. The design is in
[architecture/plugins.md](../architecture/plugins.md#provider-surfaces); the reasoning for the shape is [0106](../decisions/0106-the-plugin-api-is-mechanisms-not-features.md).

```toml
[permissions]
"content:read" = {}
"network:connect" = { hosts = ["index.example.com"] }

[[contributes.providers]]
kind = "content"
id = "index"
```

- The registry id is `<plugin>.<id>` (`static-index.index`). A plugin signed by the official key registers under the
  bare `<id>` instead; an id already taken, by a built-in or another plugin, is refused.
- A kind needs the permissions in the table below. A manifest that declares a kind without them is refused at `pack` and
  at install. `network:connect` always needs at least one entry in its `hosts`.
- The guest only names files; the host downloads them, and only from the plugin's `hosts`.

| Kind | Permissions | Registers into | Operations | Compiled example |
|---|---|---|---|---|
| `content` | `content:read`, `network:connect` | the content source registry | `info`, `parse-url`, `recognise-file-url`, `search`, `project`, `projects`, `versions`, `versions-by-id`, `identify`, `resolve-manifest` | [`static-index`](#content-sources) |
| `flavor` | `flavor:provide`, `network:connect` | the flavor registry | `info`, `versions`, `loader-versions`, `resolve-server`, `resolve-client`, `build` | [`replay-flavor`](#flavors) |
| `java` | `java:provide`, `network:connect` | the Java provider roster | `majors`, `resolve` | [`replay-java`](#java-distributions) |
| `pack` | `pack:provide` | the pack formats import detection ranks and export offers | `info`, `claims`, `read`, `write` | [`replay-pack`](#pack-formats) |

Every kind has an example in this page that is compiled and run by the engine's tests, from
`crates/engine/tests/fixture/plugins/`. Each excerpt is tagged with the file it comes from, and a test fails when a
line of one is not a line of that file, so the examples cannot drift from code that builds
([the fixture build](#compiled-examples)).

`flavor:provide`, `pack:provide` and `java:provide` are unrestricted: the install states them as full access to the
computer ([permissions.md](permissions.md)).

## Answering operations

All operations of all kinds reach the plugin through one export, `provider.invoke(kind, id, op, args)`. In Rust,
`Plugin::provide` receives a `Provide`; `on::<Op>` routes an operation to a handler typed by `proto::provider`, which
decodes the arguments and encodes the reply. The handler also gets the provider's id, for a plugin that contributes
several. An operation no handler takes answers `unsupported`.

- **Optional** operations fall back when `unsupported`: `parse-url` and `recognise-file-url` read as no match,
  `projects` asks `project` for each, `versions-by-id`, `identify` and `loader-versions` find nothing,
  `resolve-manifest` fails the import naming the source, `claims` claims the archive, and a pack's `write` fails the
  export naming the plugin.
- **Required** operations fail the call as `PluginCallFailed`, naming the plugin.
- Each call has a 30 s timeout. A reply that does not decode fails the call.
- The host stamps the plugin's id on everything returned and vets it at the edge; a result that fails the checks below
  is refused whole.

Operation names and payloads are `proto::provider::{content,flavor,java,pack}` in `crates/proto/src/provider/`; the
TypeScript mirrors are in `frontend/src/api/types/` ([other-languages.md](other-languages.md#calling-the-host-without-rust)).

## Content sources

A source describes what exists; the host downloads the files. The permissions are `content:read` and `network:connect`.

| Operation | Args | Reply | Notes |
|---|---|---|---|
| `info` | none | `SourceInfo` | asked once, when the plugin is enabled |
| `parse-url` | `{ url }` | `SourceUrl` or null | optional: a page URL on the source's site |
| `recognise-file-url` | `{ url }` | `SourceFileRef` or null | optional: the project and version behind a download URL |
| `search` | `SearchQuery` | `SearchResult` | `loaders` names every loader a hit may be built for: match any of them, none when empty |
| `project` | `{ project, kind? }` | `ContentProject` | by id or slug |
| `projects` | `{ ids }` | `ContentProject[]` | optional |
| `versions` | `VersionQuery` | `ContentVersion[]` | `loaders` as for `search`; a file whose URL is not on a granted host is dropped |
| `versions-by-id` | `{ ids }` | `ContentVersion[]` | optional |
| `identify` | `{ files: [{ sha1, path }] }` | map of sha1 to `ContentVersion` | optional; the files are mounted read-only at `path` for the call |
| `resolve-manifest` | `{ manifest }` | `ResolvedModpack` | optional: a pack manifest the source owns; each file is vetted like any download |

### Serving only when configured

A source that cannot work until the user has set something, such as an API key, names that setting in `info`:
`requires_setting: Some("api-key".into())`, a key of the plugin's own `[[settings]]` (a key the plugin does not declare
refuses the source). The host keeps the source out of use, and tells the UI which setting is missing, until the setting
holds a value; there is nothing to refresh. Send the key to the API with `hestia::http::Request::header`.

The fixture [`static-index`](../../crates/engine/tests/fixture/plugins/static-index/src/lib.rs) is a source that serves a
JSON index compiled into the component. File URLs in the index are relative to a `base-url` data file, so Hestia does
the downloading. Its whole manifest:

```toml fixture=crates/engine/tests/fixture/plugins/static-index/plugin.toml
id = "static-index"
version = "0.1.0"
api = "^1.0.0"
wasm = "plugin.wasm"

[permissions]
"content:read" = {}
"network:connect" = { hosts = ["index.example.com"] }

[[contributes.providers]]
kind = "content"
id = "index"
```

and the routing of its handlers, where each `.on` is typed by `proto::provider::content`:

```rust fixture=crates/engine/tests/fixture/plugins/static-index/src/lib.rs
impl Plugin for StaticIndex {
    fn provide(call: Provide) -> Provided {
        call.on::<Info>(|source, _| info(source))
            .on::<Search>(search)
            .on::<Project>(project)
            .on::<Versions>(project_versions)
            .on::<VersionsById>(versions_by_id)
            .on::<Identify>(identify)
            .on::<ResolveManifest>(resolve_manifest)
            .finish()
    }
}

hestia::plugin!(StaticIndex);
```

It leaves `parse-url`, `recognise-file-url` and `projects` unhandled, so URL pasting and bulk lookup fall back as above.

## Flavors

A flavor decides the code the game runs, so the host vets every profile before acting on it: each artifact must carry a
checksum (sha1, sha256 or sha512, which the download is verified against) and sit on one of the plugin's `hosts`, unless the profile names it as built (see `build` below).

A host that generates a file on request has no checksum to publish. A profile may mark such an artifact `unhashed: true`
instead of leaving the checksum out, and the host accepts it only from a host `network:connect` lists in `unhashed` (each also
in its `hosts`). The list is part of the grant, so the user sees it when approving the install or an update that adds one
([0114](../decisions/0114-an-unhashed-download-is-declared.md)). A missing checksum without the marker is refused.

| Operation | Args | Reply | Notes |
|---|---|---|---|
| `info` | none | `FlavorDescription` | asked once, when the plugin is enabled |
| `versions` | none | `string[]` or `GameVersion[]` | bare ids are typed and ordered by the host against the vanilla catalogue (newest first; an id it does not list is dropped); full entries are kept as given, newest first; at most 4096 |
| `loader-versions` | `{ game, target }` | `string[]` | optional: loader builds, newest first |
| `resolve-server` | `{ version, loaderVersion?, target }` | `{ full: ServerProfile }` or `{ onVanilla: ServerAddition }` | |
| `resolve-client` | `{ version, loaderVersion?, target }` | `{ full: InstanceProfile }` or `{ onVanilla: ClientAddition }` | a library may be marked `built: true` |
| `build` | `{ side, version, loaderVersion?, vanillaJar?, target }` | `{ fetch, steps, outputs }`, a build plan | optional: the host runs the plan and places what the profile names as built |

`info` may also declare how the flavor meets the outside ([0127](../decisions/0127-a-flavor-description-declares-its-loader.md)):
`loader` is the name a content platform gives the flavor's loader (`fabric`, `paper`; absent, the flavor's id) and
`compatible` further loader names whose content it also runs, so a version published for any of them fits (at most 8).
`interchange` is an open map `{ format-id: key }` of how each interchange format names the flavor's loader: `mrpack`
for a Modrinth pack's `dependencies` key, `prism` for a Prism or MultiMC component uid, or the id of a pack format a
plugin contributes (at most 16). A pack read in a format maps its `loader` through that format's key to a flavor, and an
export writes the key. Names are lowercase `a-z`, `0-9`, `-`, `_`, `.`; a key longer than 128 characters or holding a
control character refuses the flavor. A key a launcher flavor already claims stays with it.
It may list `requires`, ids of the plugin's own `[[programs]]` the flavor cannot do without (Spigot's BuildTools needs
`git`). Each must be a program the plugin is granted to run, or the flavor fails to register. The host checks the `PATH`
ones whenever it lists flavors, so a front-end says "needs Git, get it at ..." before a build starts; the program's
`requires = { name, url }` supplies the words
([0120](../decisions/0120-a-programs-identity-is-separate-from-its-invocations.md)).

### Building on vanilla

A flavor that layers over the game (a modloader) answers `onVanilla` with only what it adds, and the host resolves the
vanilla profile for the requested version and merges them ([0113](../decisions/0113-a-flavor-builds-on-the-vanilla-profile.md)).
The guest never reads Mojang's metadata, and needs no Mojang host in `hosts`.

| | `ClientAddition` | `ServerAddition` |
|---|---|---|
| `loaderVersion` | the loader build, shown on the entry | the same |
| `libraries` | merged over vanilla's: one of the same Maven coordinate replaces it, whatever its version | |
| `mainClass` | replaces vanilla's when not empty | |
| `jvmArgs`, `gameArgs` | appended after vanilla's | `jvmArgs` only: the flavor's recommended flags |
| `jar` | | replaces vanilla's server jar; absent runs vanilla's |
| `launch` | | `ServerLaunch::ArgsFile { path, alternatives }` replaces how the server starts, as in a `full` profile; absent keeps vanilla's. Every path it names must stay inside the data directory or the answer is refused |
| `javaMajor` | the Java major the client needs, when not vanilla's | the same; either must be a major the launcher can provide (8, 16, 17, 21) or the answer is refused |

A library in `libraries` may be declared rather than spelled out ([0119](../decisions/0119-downloads-are-host-side.md)):

```rust
LibrarySpec {
    maven: Some(Maven {
        coordinate: "net.example:loader:1.4:natives-linux@jar".into(),
        repository: "https://maven.example.com/releases".into(),
        sha1_sidecar: true,
    }),
    rules: vec![/* optional */],
    ..LibrarySpec::default()
}
```

The host derives `name`, `path`, the URL and the file name from the coordinate (`group:artifact:version[:classifier][@ext]`,
extension `jar` by default) and the repository, which must be an http(s) base URL with no query or fragment. With
`sha1_sidecar` and no `artifact.checksum`, the host fetches `<url>.sha1` (a sha1, optionally followed by a file name),
verifies the download against it and persists it; the repository host must be in `hosts`, and a missing or malformed
sidecar refuses the whole answer. A `maven` library must leave `name`, `path`, `artifact.url`, `artifact.filename` and
`built` unset; a coordinate that does not parse refuses the answer. The explicit form (`name`, `path`, `artifact`) still
works, and `LibrarySpec::from(library)` makes one. A `full` answer takes plain libraries only.

`rules` on a library, and arguments given as `Argument::Ruled(RuledArgs { value, rules })` in `jvmArgs` and `gameArgs`,
are Mojang-style: no rules allow it, otherwise the last rule whose `os` (`name`: `windows`, `osx` or `linux`; `arch`:
`x86`, `x86_64` or `arm64`; `version` is accepted but not evaluated) matches decides, `action` being `allow` or
`disallow`. A rule naming `features` never applies. The host evaluates them when it resolves, as it does for vanilla's
own, and drops what they disallow before vetting. Plain strings convert with `.into()`.

Every `resolve-*`, `loader-versions` and `build` query carries `target { os, arch, javaMajor?, pathSeparator }`, the
host's own, in the same vocabulary as the rules. `javaMajor` is set for `build`, which runs on the profile's Java, and
absent for a resolve, which has not yet resolved the vanilla profile that decides it. Use it instead of probing; fetch
tools and installers with [`hestia::download`](downloads.md).

The client, natives, asset index and Java major always come from vanilla. Only the additions are vetted against the
plugin's hosts and checksums; vanilla's own files are the host's. A flavor whose profile is not a layer over vanilla (a
server platform with its own jar and Java requirement) answers `full`.

### Building what cannot be downloaded

A flavor that runs a build on the user's machine names the result in its profile: a server's `ServerJar::Built {
filename }`, or a client `Library` with `built: true` (it needs no URL or checksum, and `path` is where it lands under the
shared libraries). The host then calls `build` ([0110](../decisions/0110-a-flavor-builds-in-its-own-area.md)) every time
the entry is provisioned or launched, so it must be cheap once done: check your own outputs first.

During that call, and only that call, the guest has two more mounts beside `/data` ([paths.md](paths.md)):

| Mount | Access | What |
|---|---|---|
| `/build` | read-write | the plugin's own area, `meta/flavors/<plugin-id>/`, shared by every entry and flavor of the plugin |
| `/vanilla` | read-only | a copy of the side's vanilla jar, alone; `vanillaJar` is that jar's guest path, absent when the side has none |

`build` answers a plan the host runs ([0124](../decisions/0124-a-build-is-a-plan-the-host-runs.md)):
`{ fetch: [{ url, checksum?, dest }], steps: [{ program, args, env, cwd?, javaMajor?, creates, narrate? }], outputs }`.
The host checks all of it first (each step must fit an invocation of a declared program exactly as for `exec`, `cwd`,
`creates`, `dest` and every output must lie under `/build`, and `${data}` is not available), downloads `fetch`, then
runs the steps in order under the supervisor. A step whose `creates` all exist is skipped; `narrate: { skipPrefixes }`
shows every other line of its output as progress; a cancel stops the build between steps. A build still running under
the same plugin, version and step is joined rather than started twice. A plan lists at most 64 steps and 512 downloads.

A plan answers one more thing than files: where they go. An output is a file under `/build`, named by the profile's
built server jar or built library, or a directory tree with a `destination` (`entry`, the server's data directory, which
keeps a file that already exists, or `libraries`, the shared libraries root) and the relative `target` it is placed at
([0121](../decisions/0121-a-build-places-trees-and-the-profile-launches-from-an-args-file.md)). A `Library` with
`classpath: false` is materialised and kept off `-cp`, and a server profile may launch from an args file,
`ServerLaunch::ArgsFile { path, alternatives }`, where each alternative names the file for one `os` of a rule's
vocabulary and the host picks the one for the machine.

Or run tools yourself with `hestia::process::exec_in(program, args, timeout, "/build")` (its output shows as progress as
it prints), declare them in `[[programs]]` (a Java tool as `java = 21`, run on a Hestia-managed runtime), and answer
with only the outputs: `path` is a file under `/build` and `target` is the filename of the profile's built server jar or
the `path` of one of its built libraries. The host copies each output into place, refuses a symlink, a path outside
`/build` and a target the profile did not name, and fails the provision when a built file is still missing.
`hestia::progress::report` and `cancelled` reach the provision's progress and cancel while `build` runs. Return
`unsupported` (answer nothing) for a version with nothing to build.

The fixture [`replay-flavor`](../../crates/engine/tests/fixture/plugins/replay-flavor/src/lib.rs) is the compiled
reference. It re-serves a vanilla-shaped profile and has one `build` version for each thing a build can do, and others
the host must refuse, so its source is longer than a flavor needs. It also declares two more programs and two more
providers for those tests; the manifest below leaves them out.

```toml fixture=crates/engine/tests/fixture/plugins/replay-flavor/plugin.toml
id = "replay-flavor"
version = "0.1.0"
api = "^1.0.0"
wasm = "plugin.wasm"

[permissions]
"flavor:provide" = {}
"network:connect" = { hosts = ["files.example.com"], unhashed = ["files.example.com"] }
"process:spawn" = { programs = ["builder", "strict"] }

[[programs]]
id = "builder"
java = 21
args = ["*", "**"]

...

[[contributes.providers]]
kind = "flavor"
id = "replay"
```

and its routing, where `build` answers a plan and the other four operations a profile:

```rust fixture=crates/engine/tests/fixture/plugins/replay-flavor/src/lib.rs
impl Plugin for Replay {
    fn provide(call: Provide) -> Provided {
        call.on::<Info>(|flavor, _| info(flavor))
            .on::<Versions>(|flavor, _| versions(flavor))
            .on::<ResolveServer>(resolve_server)
            .on::<ResolveClient>(resolve_client)
            .on::<Build>(build)
            .finish()
    }
}

hestia::plugin!(Replay);
```

## Pack formats

A pack format reads, and optionally writes, an archive that carries a game version, a loader and files. The host owns
the archive: the guest is handed member names and the text of the members it declared, and answers with data. The only
permission is `pack:provide`. A pack that names its own files needs `network:connect` with the hosts they live on; one
that delegates its files to a content source needs none.

| Operation | Args | Reply | Notes |
|---|---|---|---|
| `info` | none | `PackInfo` (`name`, `members`, `extension`) | `members` are the archive members it reads, as names or globs (`*` matches within one path segment); `extension` is empty for a format that cannot write |
| `claims` | `{ files }` | `{ claimed }` | optional; asked once an archive holds every declared member, with the names relative to the pack's directory. Unsupported claims it |
| `read` | `{ files, members }` | `PackRead` | the pack (`name`, `gameVersion`, `loader`, `loaderVersion`, `files`), its `overrides` trees per side, optionally `resolve { source, manifest }`, and optionally `memory` and `jvmArgs` |
| `write` | `{ instance, files, content }` | `PackWritten` | manifests to store, `{ entryPath, archivePath }` copies, and warnings in English |

`read` is vetted whole: a file path that is absolute, empty or leaves the entry, a file whose URL is not on a granted host
over https with a well-formed checksum (an empty URL names a file its author withholds), an override tree that is not a
plain relative directory, a pack with no game version, `resolve` together with `files`, or a `memory` or `jvmArgs` the
`memory` and `jvm-args` settings would reject, refuses the answer. `memory` and `jvmArgs` are applied to the instance an
install creates and to nothing else: an existing instance, an update and a server keep their own.
`resolve.source` names a content source whose `resolve-manifest` returns the files; the pack's own name, game version and
loader win over the source's. A member a guest declares must be UTF-8 text of at most 8 MiB.
`write` may copy only paths of its query (`files` and each `content` item's `path`), writes no archive path twice, and
may add the content items it can refer to by download instead of copying.

The fixture [`replay-pack`](../../crates/engine/tests/fixture/plugins/replay-pack/src/lib.rs) identifies an archive by
`replay.json`, answers it verbatim, and exports every file with one warning. Its whole manifest:

```toml fixture=crates/engine/tests/fixture/plugins/replay-pack/plugin.toml
id = "replay-pack"
version = "0.1.0"
api = "^1.0.0"
wasm = "plugin.wasm"

[permissions]
"pack:provide" = {}

[[contributes.providers]]
kind = "pack"
id = "replay"
```

and its routing:

```rust fixture=crates/engine/tests/fixture/plugins/replay-pack/src/lib.rs
impl Plugin for Replay {
    fn provide(call: Provide) -> Provided {
        call.on::<Info>(|pack, _| info(pack))
            .on::<Claims>(claims)
            .on::<Read>(read)
            .on::<Write>(write)
            .finish()
    }
}

hestia::plugin!(Replay);
```

## Java distributions

A runtime is native code the launcher executes, so a Java provider only names a build; the host downloads it, extracts
it and registers it ([0105](../decisions/0105-a-java-provider-only-resolves.md)).

| Operation | Args | Reply |
|---|---|---|
| `majors` | none | `{ major, lts }[]`: the feature versions on offer, `lts` marking a long-term-support release |
| `resolve` | `{ major, os, arch }` | `JavaBuild` or null when there is no build for that platform |

- `os` is `linux`, `windows` or `mac`; `arch` is `x64` or `aarch64`.
- A `JavaBuild` is `{ vendor, version, url, sha256, archive }`. The URL must be https on a granted host, the sha256 is
  64 hex characters, `archive` is `tar.gz` or `zip`, and `vendor` is a plain name (letters, digits, `-`, `_`) because it
  names the install directory.
- The user picks it with the `java.provider` setting or `hestia java install --provider <plugin>.<id>`.

The fixture [`replay-java`](../../crates/engine/tests/fixture/plugins/replay-java/src/lib.rs) offers Java 17 and 21 and
names a build for each. It serves 21 from its granted host and, to show the refusal, 17 from one outside the grant. Its
whole manifest:

```toml fixture=crates/engine/tests/fixture/plugins/replay-java/plugin.toml
id = "replay-java"
version = "0.1.0"
api = "^1.0.0"
wasm = "plugin.wasm"

[permissions]
"java:provide" = {}
"network:connect" = { hosts = ["downloads.example.com"] }

[[contributes.providers]]
kind = "java"
id = "replay"
```

its answers, typed by `proto::provider::java`:

```rust fixture=crates/engine/tests/fixture/plugins/replay-java/src/lib.rs
fn majors(distribution: &str) -> Result<Vec<JavaRelease>, Error> {
    check(distribution)?;
    Ok(vec![
        JavaRelease {
            major: 17,
            lts: true,
        },
        JavaRelease {
            major: 21,
            lts: true,
        },
    ])
}

fn resolve(distribution: &str, target: TargetQuery) -> Result<Option<JavaBuild>, Error> {
    check(distribution)?;
    let host = match target.major {
        21 => "downloads.example.com",
        17 => "mirror.example.org",
        _ => return Ok(None),
    };
    let archive = if target.os == "windows" {
        "zip"
    } else {
        "tar.gz"
    };
    Ok(Some(JavaBuild {
        vendor: "replay".into(),
        version: format!("{}.0.1", target.major),
        url: format!("https://{host}/replay-{}.{archive}", target.major),
        sha256: SHA256.into(),
        archive: archive.into(),
    }))
}
```

and its routing:

```rust fixture=crates/engine/tests/fixture/plugins/replay-java/src/lib.rs
impl Plugin for Replay {
    fn provide(call: Provide) -> Provided {
        call.on::<Majors>(|distribution, _| majors(distribution))
            .on::<Resolve>(resolve)
            .finish()
    }
}

hestia::plugin!(Replay);
```

Asking for 21 gives the host a package it downloads; asking for 17 fails the call as `resolve returned a refused build`,
and asking for a version with no build answers `null`, which fails it naming the platform.

## Compiled examples

The fixtures are plugins of their own under `crates/engine/tests/fixture/plugins/`, each a Rust crate that depends on
the SDK by path. The engine's tests build every one once into a kept directory under the target directory
(`crates/engine/tests/fixture/`), so a warm `cargo test` does not compile them again, and load it through the same
runtime and registries a daemon uses. `crates/engine/tests/docs.rs` reads this guide: each fenced block tagged
`fixture=<path>` must be lines of that file in order (a line of `...` stands for lines left out), and every kind in
`proto::provider::KINDS` must have a tagged Rust excerpt from a plugin that declares it. Adding a provider kind
therefore fails that test until this page has an example of it.

## Errors

Return `Error::NotFound` for a provider id the plugin did not declare (the examples' `check` does), `Error::Invalid`
for arguments it cannot use, and the case that says what happened for anything else. The host keeps the case
([0115](../decisions/0115-a-plugin-error-is-its-own-typed-enum.md)):

| The plugin returns | The user sees |
|---|---|
| `Error::Offline` | the provider's service is `Offline`, named by the provider's display name, as for a built-in provider |
| `Error::Upstream { status, retry_after, .. }`, `Error::RateLimited { retry_after }` | an upstream failure of the provider's service |
| `Error::SignInRequired` | the sign-in prompt |
| `Error::NotConfigured(key)` | the plugin's setting `key` is not set |
| anything else | `PluginCallFailed` naming the plugin, with the message |

`hestia::http` already returns `Error::Offline` for a request that timed out or found no route, so passing it up with `?`
reports the provider offline; a refused request is `Error::Denied` and an answered one is the plugin's to judge, such
as `Error::Upstream` for a 503; `response.error_for_status()?` does that mapping ([http.md](http.md#from-a-status-to-an-error)). Keep every message free of tokens and secrets.
