# Providers

A provider fills one of the launcher's open registries: a content source, a flavor (a game distribution), a pack format
or a Java distribution. The manifest declares each one and the host asks the plugin questions as the user searches,
creates, imports or installs.

```toml
[permissions]
"content:read" = {}
"network:connect" = { hosts = ["index.example.com"] }

[[contributes.providers]]
kind = "content"
id = "index"
```

- The registry id is always `<plugin>.<id>` (`static-index.index`). Users may type the bare `<id>`: it means core's
  provider of that name, else the one official plugin's, else the only provider with that id, and is refused as
  ambiguous when two plugins share it. A plugin can never take a core provider's id.
- A kind needs the permissions below, or the manifest is refused at `pack` and at install. `network:connect` always
  needs at least one host. `flavor:provide`, `pack:provide` and `java:provide` are unrestricted.
- The guest only names files; the host downloads them, and only from the plugin's `hosts`.

| Kind | Permissions | Operations | Compiled example |
|---|---|---|---|
| `content` | `content:read`, `network:connect` | `info`, `parse-url`, `recognise-file-url`, `search`, `project`, `projects`, `versions`, `versions-by-id`, `latest`, `identify`, `resolve-manifest` | [`static-index`](#content-sources) |
| `flavor` | `flavor:provide`, `network:connect` | `info`, `versions`, `loader-versions`, `resolve-server`, `resolve-client`, `build` | [`replay-flavor`](#flavors) |
| `java` | `java:provide`, `network:connect` | `majors`, `resolve` | [`replay-java`](#java-distributions) |
| `pack` | `pack:provide` | `info`, `claims`, `read`, `write` | [`replay-pack`](#pack-formats) |

Every kind has an example here that the engine's tests build and run from `crates/engine/tests/fixture/plugins/`. Each
excerpt is tagged with its file, and `crates/engine/tests/docs.rs` fails when a line of one is not a line of that file,
and when a kind in `proto::provider::KINDS` has no tagged Rust excerpt on this page.

## Answering operations

Every operation reaches the plugin through one export. In Rust, `Plugin::provide` receives a `Provide`, and
`on::<Op>(handler)` routes an operation to a handler typed by `proto::provider`, which decodes the arguments and encodes
the reply. The handler also gets the provider's id, for a plugin that contributes several. An operation no handler takes
answers `unsupported`.

- **Optional** operations fall back when unsupported: `parse-url` and `recognise-file-url` read as no match, `projects`
  asks `project` for each, `latest` asks `versions` for each item, `versions-by-id`, `identify` and `loader-versions`
  find nothing, `resolve-manifest` fails the import naming the source, `claims` claims the archive, and a pack's `write`
  fails the export naming the plugin.
- **Required** operations fail the call as `PluginCallFailed`, naming the plugin.
- Each call has a 30 second timeout. A reply that does not decode, or fails the checks below, is refused whole.

Operation names and payloads are `proto::provider::{content,flavor,java,pack}` in `crates/proto/src/provider/`;
[other-languages.md](other-languages.md#calling-the-host-without-rust) says where the TypeScript mirrors are.

## Content sources

A source describes what exists; the host downloads the files.

| Operation | Args | Reply | Notes |
|---|---|---|---|
| `info` | none | `SourceInfo` | asked once, when the plugin is enabled; `editions` names the game editions served, Java alone when empty |
| `parse-url` | `{ url }` | `SourceUrl` or null | optional: a page URL on the source's site |
| `recognise-file-url` | `{ url }` | `SourceFileRef` or null | optional: the project and version behind a download URL |
| `search` | `SearchQuery` | `SearchResult` | `loaders` lists every loader a hit may be built for: match any, none when empty |
| `project` | `{ project, kind? }` | `ContentProject` | by id or slug |
| `projects` | `{ ids }` | `ContentProject[]` | optional |
| `versions` | `VersionQuery` | `ContentVersion[]` | a file whose URL is not on a granted host is dropped |
| `versions-by-id` | `{ ids }` | `ContentVersion[]` | optional |
| `latest` | `{ gameVersion, loaders, files: [{ projectId, sha1 }] }` | map of project id to version id | optional: the newest compatible version per project (a release first, else any channel); a project left out has none |
| `identify` | `{ files: [{ sha1, path }] }` | map of sha1 to `ContentVersion` | optional; the files are mounted read-only at `path` for the call |
| `resolve-manifest` | `{ manifest }` | `ResolvedModpack` | optional: a pack manifest the source owns; each file is vetted like any download |

A source that cannot work until the user sets something, such as an API key, names that setting in `info`:
`requires_setting: Some("api-key".into())`, a key of the plugin's own `[[settings]]`. The host keeps the source out of
use, and tells the UI which setting is missing, until it holds a value.

`static-index` serves a JSON index compiled into the component. File URLs in the index are relative to a `base-url` data
file, so Hestia does the downloading. Its whole manifest:

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

and the routing of its handlers:

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

It leaves `parse-url`, `recognise-file-url` and `projects` unhandled, so those fall back as above.

## Flavors

A flavor decides the code the game runs, so the host vets every profile: each artifact must carry a checksum (sha1,
sha256 or sha512) and sit on one of the plugin's `hosts`, unless the profile names it as built. A host that generates
files on request, with no checksum to publish, may be listed in the grant's `unhashed`; the profile then marks the
artifact `unhashed: true`. A missing checksum without the marker is refused.

| Operation | Args | Reply | Notes |
|---|---|---|---|
| `info` | none | `FlavorDescription` | asked once, when the plugin is enabled |
| `versions` | none | `string[]` or `GameVersion[]` | bare ids are typed and ordered against the vanilla catalogue, newest first (an unknown id is dropped); full entries are kept as given; at most 4096 |
| `loader-versions` | `{ game, target }` | `string[]` | optional: loader builds, newest first |
| `resolve-server` | `{ version, loaderVersion?, target }` | `{ full: ServerProfile }` or `{ onVanilla: ServerAddition }` | |
| `resolve-client` | `{ version, loaderVersion?, target }` | `{ full: InstanceProfile }` or `{ onVanilla: ClientAddition }` | a library may be marked `built: true` |
| `build` | `{ side, version, loaderVersion?, vanillaJar?, target }` | a build plan | optional: the host runs the plan and places what the profile names as built |

`info` can also declare how the flavor meets the outside world:

- `loader`: the name a content platform gives the flavor's loader (`fabric`, `paper`); the flavor's id when absent.
- `compatible`: further loader names whose content it also runs (at most 8).
- `interchange`: how each interchange format names the flavor's loader, as `{ format-id: key }` (at most 16): `mrpack`
  for a Modrinth pack's `dependencies` key, `prism` for a Prism or MultiMC component uid, or the id of a plugin's pack
  format. Names are lowercase `a-z`, `0-9`, `-`, `_`, `.`; a key over 128 characters or with a control character
  refuses the flavor, and a key a launcher flavor already claims stays with it.
- `requires`: ids of the plugin's own `[[programs]]` the flavor cannot do without (Spigot's BuildTools needs `git`). The
  host checks `PATH` ones whenever it lists flavors, so a front-end says "needs Git" before a build starts.

Every `resolve-*`, `loader-versions` and `build` query carries `target { os, arch, javaMajor?, pathSeparator }` of the
host, so use it instead of probing. `javaMajor` is set for `build` only.

### Building on vanilla

A flavor that layers over the game (a modloader) answers `onVanilla` with only what it adds, and the host merges it
into the vanilla profile for the version. The guest never reads Mojang's metadata and needs no Mojang host. A flavor
that is not a layer over vanilla (a server platform with its own jar) answers `full`.

| | `ClientAddition` | `ServerAddition` |
|---|---|---|
| `loaderVersion` | the loader build, shown on the entry | the same |
| `libraries` | merged over vanilla's: the same Maven coordinate replaces it, whatever its version | |
| `mainClass` | replaces vanilla's when not empty | |
| `jvmArgs`, `gameArgs` | appended after vanilla's | `jvmArgs` only |
| `jar` | | replaces vanilla's server jar; absent runs vanilla's |
| `launch` | | `ServerLaunch::ArgsFile { path, alternatives }` replaces how the server starts; every path must stay in the data directory |
| `javaMajor` | the Java the client needs, when not vanilla's | the same; either must be 8, 16, 17 or 21 |

A library can be declared rather than spelled out:

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

The host derives `name`, `path`, URL and file name from the coordinate (`group:artifact:version[:classifier][@ext]`,
extension `jar` by default) and the repository (an http(s) base URL with no query or fragment). With `sha1_sidecar` and
no `artifact.checksum`, the host fetches `<url>.sha1` and verifies against it; the repository host must be in `hosts`.
A `maven` library leaves `name`, `path`, `artifact.url`, `artifact.filename` and `built` unset. The explicit form
(`name`, `path`, `artifact`) also works, and a `full` answer takes plain libraries only.

`rules` on a library, and `Argument::Ruled(RuledArgs { value, rules })` in `jvmArgs` and `gameArgs`, are Mojang-style:
with no rules it is allowed; otherwise the last rule whose `os` (`name`: `windows`, `osx`, `linux`; `arch`: `x86`,
`x86_64`, `arm64`) matches decides by `action` (`allow` or `disallow`). A rule naming `features` never applies.

### Building what cannot be downloaded

A flavor that builds on the user's machine names the result in its profile: a server's `ServerJar::Built { filename }`,
or a client `Library` with `built: true` (no URL or checksum; `path` is where it lands under the shared libraries). The
host calls `build` each time the entry is provisioned or launched, so check your own outputs first and make a finished
build cheap. During that call the guest also has `/build` and `/vanilla` ([api.md](api.md#the-sandbox)); `vanillaJar` is
the jar's guest path, absent when the side has none. Answer nothing (`unsupported`) for a version with nothing to build.

`build` answers a plan the host runs: `{ fetch: [{ url, checksum?, dest }], steps: [{ program, args, env, cwd?,
javaMajor?, creates, narrate? }], outputs }`.

- The host vets all of it first: each step must fit an invocation of a declared program as for `exec`; `cwd`,
  `creates`, `dest` and every output must be under `/build`; `${data}` is not available. A plan has at most 64 steps
  and 512 downloads.
- It downloads `fetch` (a few at a time; the first failure stops the rest), then runs the steps in order. A step whose
  `creates` all exist is skipped; `narrate: { skipPrefixes }` shows its other output lines as progress; a cancel stops
  between steps. An identical build already running is joined.
- An output is a file under `/build` named by the profile's built jar or library, or a directory tree with a
  `destination` (`entry`, the server's data directory, which keeps files that exist, or `libraries`) and a relative
  `target`. A library with `classpath: false` is materialised but kept off `-cp`. The host refuses a symlink, a path
  outside `/build` and a target the profile did not name.

You may instead run tools yourself with `hestia::process::exec_in(program, args, timeout, "/build")` (output shows as
progress), declare them in `[[programs]]` (a Java tool as `java = 21`), and answer with only the outputs. `progress::report`
and `cancelled` reach the provision while `build` runs.

`replay-flavor` is the compiled reference. It re-serves a vanilla-shaped profile and has one `build` version for each
thing a build can do, plus versions the host must refuse, so it is longer than a flavor needs. It declares two more
programs and providers for those tests; the manifest below leaves them out.

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
the archive; the guest is handed member names and the text of the members it declared. A pack that names its own files
also needs `network:connect` for their hosts; one that delegates its files to a content source needs none.

| Operation | Args | Reply | Notes |
|---|---|---|---|
| `info` | none | `PackInfo` (`name`, `members`, `extension`) | `members` are the archive members it reads, as names or globs (`*` within one path segment); `extension` is empty for a format that cannot write |
| `claims` | `{ files, members }` | `{ claimed }` | optional; asked once an archive holds every declared member, so a format can look inside before claiming. A member that is not UTF-8, is over 8 MiB or would pass 16 MiB in total is left out, not refused. Unsupported claims it |
| `read` | `{ files, members, loaders }` | `PackRead` | the pack (`name`, `gameVersion`, `loader`, `loaderVersion`, `files`), its `overrides` trees per side, optionally `resolve { source, manifest }`, `memory`, `jvmArgs` and `icon`. `loaders` are the keys registered flavors declare for this format's id in `interchange` |
| `write` | `{ instance, files, content }` | `PackWritten` | manifests to store, `{ entryPath, archivePath }` copies, and English warnings. Each `content` item has its `path`, whether it is `referenceable` and, when it is, `size` and `sha512` beside its SHA-1 |

`read` is vetted whole. These refuse the answer: an absolute, empty or escaping file path; a file URL not on a granted
host over https, or with a malformed checksum (an empty URL names a file its author withholds); an override tree that is
not a plain relative directory; no game version; `resolve` together with `files`. A `loader` that is one of the offered
keys becomes the flavor it maps to. A file in an override tree directly in a content directory ending in `.disabled`
is indexed as disabled content; a reader does nothing for it.

A `memory` or `jvmArgs` the settings would reject is dropped with a `pack_setting_dropped` warning. An `icon` (an
archive path to a PNG, JPEG, WebP or GIF of at most 10 MiB) becomes the new instance's icon, else it is dropped with
`pack_icon_dropped`. These apply only to the instance an install creates, not to an existing one, an update or a
server. `resolve.source` names a content source whose `resolve-manifest` returns the files; the pack's own name, game
version and loader win. `write` may copy only paths of its query, writes no archive path twice, and may add content
items it refers to by download instead of copying.

`replay-pack` identifies an archive by `replay.json`, answers it verbatim, and exports every file with one warning:

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

A Java provider only names a build; the host downloads, extracts and registers it.

| Operation | Args | Reply |
|---|---|---|
| `majors` | none | `{ major, lts }[]`: the feature versions on offer |
| `resolve` | `{ major, os, arch }` | `JavaBuild`, or null when there is no build for that platform |

`os` is `linux`, `windows` or `mac`; `arch` is `x64` or `aarch64`. A `JavaBuild` is `{ vendor, version, url, sha256,
archive }`: the URL is https on a granted host, the sha256 is 64 hex characters, `archive` is `tar.gz` or `zip`, and
`vendor` is a plain name (letters, digits, `-`, `_`) because it names the install directory. The user picks a provider
with the `java.provider` setting or `hestia java install --provider <plugin>.<id>`.

`replay-java` offers Java 17 and 21. It serves 21 from its granted host and, to show the refusal, 17 from one outside the
grant. Its manifest:

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

its answers:

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

Asking for 21 gives the host a package to download; asking for 17 fails as `resolve returned a refused build`; asking
for a version with no build answers `null`, which fails naming the platform.

## Errors

Return `Error::NotFound` for a provider id the plugin did not declare, `Error::Invalid` for arguments it cannot use,
and the case that says what happened for anything else:

| The plugin returns | The user sees |
|---|---|
| `Error::Offline` | the provider's service `Offline`, named by the provider's display name |
| `Error::Upstream { status, retry_after, .. }`, `Error::RateLimited { retry_after }` | an upstream failure of the provider's service |
| `Error::SignInRequired` | the sign-in prompt |
| `Error::NotConfigured(key)` | the plugin's setting `key` is not set |
| anything else | `PluginCallFailed` naming the plugin, with the message |

`hestia::http` already returns `Offline` for a timeout or no route, so `?` reports the provider offline;
`response.error_for_status()?` maps a failing status ([api.md](api.md#http)). Keep messages free of tokens and secrets.
