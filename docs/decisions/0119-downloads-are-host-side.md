# Downloads are host-side: a host download call and declarative Maven libraries

*Applies to: [Plugins](../architecture/plugins.md#network), [Minecraft providers](../architecture/minecraft.md)*

A plugin flavor did host-grade work in the guest. The first NeoForge guest was 1,252 lines: it fetched its installer
and tools over `wasi:http` and kept them in its own `data/`, re-downloaded them on every build, derived every Maven path
and `.sha1` sidecar URL itself, and found the host's `${pathsep}` by running `java -XshowSettings`. All of it is work the
engine already does once, well, for core flavors: a guarded client, a checksum, a cache, progress and a cancel.

**A guest asks the host to download.** `download(url, expect, dest)` is a host import (`download.wit`, `hestia::download`)
that fetches through the engine's `Downloader` under the plugin's `vouch::Policy`, the same path a flavor's artifacts
take. The guest never holds the body.

- *Permission and hops.* `network:connect`, and the URL, every redirect hop and, with no checksum, the `unhashed` scope
  are checked against the grant on every hop, over https, behind the guarded resolver.
- *Checksum or declared unhashed.* `expect` is a sha1, sha256 or sha512 the file must have, or none, which is allowed
  only from a host the grant lists as serving unhashed files
  ([0114](0114-an-unhashed-download-is-declared.md)). A mismatch leaves nothing at `dest`: the file is written to a `.part`
  beside it and renamed only when complete. The `.part` is opened exclusively, so a symlink a guest planted at that name
  is refused rather than written through.
- *Where it writes.* `dest` is a guest path in an existing directory of a writable mount of the current call (`/data`,
  or `/build` during a flavor's `build`), resolved by the rule `process.exec`'s `cwd` uses; anything else is `denied`.
- *Cache.* A checksummed download is served from the engine's content-addressed cache and feeds it, so a build that runs
  again, or another plugin fetching the same file, does not transfer it twice. With no checksum there is nothing to key on.
- *Progress and limits.* Progress goes into the call's own progress channel (the one `progress.report` feeds), the
  call's cancel aborts the transfer and the call's remaining time bounds it. A body is capped at 4 GiB while it streams.
- *Typed failures.* No route is `offline`, a 404 `not-found`, any other status `upstream` carrying it, a refusal
  `denied`, a malformed checksum or an oversize body `invalid`
  ([0115](0115-a-plugin-error-is-its-own-typed-enum.md)). The downloader's failures carry a typed reason beside their
  message so the mapping reads a status, never a string.

**Rejected: a download helper in the SDK.** A guest-side helper over `wasi:http` would be one more client that has to be
guarded, hashed, cached and bounded by every guest, with the body in guest memory and a 32 MiB cap, and each guest would
still choose its own cache. The host already owns that work and the grant check, so the helper would only move the
copy.
