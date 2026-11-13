# Providers

A provider fills one of the launcher's open registries: a content source, a flavor (a game distribution), an archive
format or a Java distribution. A plugin declares each one in the manifest, and the host asks it questions as the user
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

| Kind | Permissions | Registers into | Operations |
|---|---|---|---|
| `content` | `content:read`, `network:connect` | the content source registry | `info`, `parse-url`, `recognise-file-url`, `search`, `project`, `projects`, `versions`, `versions-by-id`, `identify` |
| `flavor` | `flavor:provide`, `network:connect` | the flavor registry | `info`, `versions`, `loader-versions`, `resolve-server`, `resolve-client`, `build` |
| `format` | `format:provide` | the archive formats import detection ranks | `info`, `read`, `land`, `write` |
| `java` | `java:provide`, `network:connect` | the Java provider roster | `majors`, `resolve` |

`flavor:provide`, `format:provide` and `java:provide` are unrestricted: the install states them as full access to the
computer ([permissions.md](permissions.md)).

## Answering operations

All operations of all kinds reach the plugin through one export, `provider.invoke(kind, id, op, args)`. In Rust,
`Plugin::provide` receives a `Provide`; `on::<Op>` routes an operation to a handler typed by `proto::provider`, which
decodes the arguments and encodes the reply. The handler also gets the provider's id, for a plugin that contributes
several. An operation no handler takes answers `unsupported`.

- **Optional** operations fall back when `unsupported`: `parse-url` and `recognise-file-url` read as no match,
  `projects` asks `project` for each, `versions-by-id`, `identify` and `loader-versions` find nothing.
- **Required** operations fail the call as `PluginCallFailed`, naming the plugin.
- Each call has a 30 s timeout. A reply that does not decode fails the call.
- The host stamps the plugin's id on everything returned and vets it at the edge; a result that fails the checks below
  is refused whole.

Operation names and payloads are `proto::provider::{content,flavor,format,java}` in `crates/proto/src/provider/`; the
TypeScript mirrors are in `frontend/src/api/types/` ([other-languages.md](other-languages.md#calling-the-host-without-rust)).

## Content sources

A source describes what exists; the host downloads the files. The permissions are `content:read` and `network:connect`.

| Operation | Args | Reply | Notes |
|---|---|---|---|
| `info` | none | `SourceInfo` | asked once, when the plugin is enabled |
| `parse-url` | `{ url }` | `SourceUrl` or null | optional: a page URL on the source's site |
| `recognise-file-url` | `{ url }` | `SourceFileRef` or null | optional: the project and version behind a download URL |
| `search` | `SearchQuery` | `SearchResult` | |
| `project` | `{ project, kind? }` | `ContentProject` | by id or slug |
| `projects` | `{ ids }` | `ContentProject[]` | optional |
| `versions` | `VersionQuery` | `ContentVersion[]` | a file whose URL is not on a granted host is dropped |
| `versions-by-id` | `{ ids }` | `ContentVersion[]` | optional |
| `identify` | `{ sha1s }` | map of sha1 to `ContentVersion` | optional |

Example: [`static-index`](https://github.com/prytaneum/plugin-examples/tree/main/plugins/static-index), a source
that serves a JSON index compiled into the component. File URLs in the index are relative to a `base-url` data file, so
Hestia does the downloading. Excerpt of `plugins/static-index/src/lib.rs`:

```rust
impl Plugin for StaticIndex {
    fn provide(call: Provide) -> Provided {
        call.on::<Info>(|source, _| info(source))
            .on::<Search>(search)
            .on::<Project>(project)
            .on::<Versions>(project_versions)
            .on::<VersionsById>(versions_by_id)
            .finish()
    }
}

hestia::plugin!(StaticIndex);
```

It leaves the optional operations unhandled, so URL pasting, bulk lookup and file identification fall back as above.

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
| `versions` | none | `GameVersion[]` | newest first |
| `loader-versions` | `{ game, target }` | `string[]` | optional: loader builds, newest first |
| `resolve-server` | `{ version, loaderVersion?, target }` | `{ full: ServerProfile }` or `{ onVanilla: ServerAddition }` | |
| `resolve-client` | `{ version, loaderVersion?, target }` | `{ full: InstanceProfile }` or `{ onVanilla: ClientAddition }` | a library may be marked `built: true` |
| `build` | `{ side, version, loaderVersion?, vanillaJar?, target }` | `{ outputs: [{ path, target }] }` | optional: produces what the profile names as built |

`info` may also give `mrpackKey` (the key a Modrinth pack names the flavor's loader by in its `dependencies`) and
`prismUid` (the component uid of a Prism or MultiMC instance), so packs and instances of the flavor import and export.

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

A library in `libraries` may be declared rather than spelled out ([0118](../decisions/0118-downloads-are-host-side.md)):

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

During that call, and only that call, the guest has two more mounts beside `/data`:

| Mount | Access | What |
|---|---|---|
| `/build` | read-write | the flavor's own area, `meta/flavors/<flavor-id>/`, shared by every entry of the flavor |
| `/vanilla` | read-only | a copy of the side's vanilla jar, alone; `vanillaJar` is that jar's guest path, absent when the side has none |

Run tools with `hestia::process::exec_in(program, args, timeout, "/build")`, declare them in `[[programs]]` (a Java tool
as `java = 21`, run on a Hestia-managed runtime), and answer with the files to keep: `path` is a file under `/build`,
`target` is the filename of the profile's built server jar or the `path` of one of its built libraries. The host copies
each output into place, refuses a symlink, a path outside `/build` and a target the profile did not name, and fails the
provision when a built file is still missing. A server that launches from an args file is not supported yet.
`hestia::progress::report` and `cancelled` reach the provision's progress and cancel while `build` runs. Return
`unsupported` (answer nothing) for a version with nothing to build.

No published example exists. The test fixture
[`replay-flavor`](../../crates/engine/tests/fixture/plugins/replay-flavor/src/lib.rs) is the compiled reference: it
re-serves a vanilla-shaped profile, includes two deliberately bad versions the host must refuse, and has `build`
versions that run a declared managed-Java program in `/build`. Its
manifest, `crates/engine/tests/fixture/plugins/replay-flavor/plugin.toml`:

```toml
id = "replay-flavor"
version = "0.1.0"
api = "^1.0.0"
wasm = "plugin.wasm"
[permissions]
"flavor:provide" = {}
"network:connect" = { hosts = ["files.example.com"], unhashed = ["files.example.com"] }
"process:spawn" = { programs = ["builder"] }

[[programs]]
id = "builder"
java = 21
args = ["*"]

[[contributes.providers]]
kind = "flavor"
id = "replay"
```

and its handlers, from `src/lib.rs`:

```rust
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
```

## Archive formats

A format reads and writes an instance archive. The host owns the archive: the guest is handed the manifest text and the
member names, and answers with data. A manifest that is not UTF-8 text is refused before the guest is asked. The only
permission is `format:provide`, and it needs no network.

| Operation | Args | Reply | Notes |
|---|---|---|---|
| `info` | none | `FormatInfo` (`name`, `kind`, `marker`) | `marker` is the archive member whose presence identifies the format; `kind` is `instance` or `pack` |
| `read` | `{ manifest, files }` | `FormatBlueprint` | the instance the manifest describes |
| `land` | `{ blueprint, files }` | `FormatCopy[]` | which archive members go where, on import |
| `write` | `{ blueprint, files }` | `FormatWritten` | the manifest bytes and the files to put in the archive, on export |

A `FormatCopy` is `{ archivePath, entryPath }`. One path that escapes the instance, or names a member not in `files`,
refuses the answer.

No published example exists. The fixture
[`replay-format`](../../crates/engine/tests/fixture/plugins/replay-format/src/lib.rs) identifies an archive by
`replay.cfg`, reads `key=value` lines, and lands `files/<path>` members. Its handlers:

```rust
impl Plugin for Replay {
    fn provide(call: Provide) -> Provided {
        call.on::<Info>(|format, _| info(format))
            .on::<Read>(read)
            .on::<Land>(land)
            .on::<Write>(write)
            .finish()
    }
}
```

with `"format:provide" = {}` under `[permissions]` and `kind = "format"` in its `plugin.toml`.

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

No example exists yet, compiled or published. The SDK shape is the same as the others, typed by
`proto::provider::java::{Majors, Resolve}`:

```rust
call.on::<Majors>(|id, _| /* Result<Vec<JavaRelease>, Error> */)
    .on::<Resolve>(|id, target| /* Result<Option<JavaBuild>, Error> */)
    .finish()
```

(a signature sketch, not a compiled sample).

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
