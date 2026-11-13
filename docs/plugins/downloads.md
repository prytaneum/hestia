# Downloads

A plugin that needs a file on disk (a tool to run, an installer, a jar) asks the daemon to fetch it. The daemon does the
transfer, so the plugin never holds the body and gets the engine's checksum, cache, progress and cancel for free
([0119](../decisions/0119-downloads-are-host-side.md)). Use `hestia::http` for API calls and small documents; use
`hestia::download` for files.

```rust
use hestia::download::{download, Expect};

let jar = download(
    "https://files.example.com/tool-1.4.jar",
    Expect::Sha256("a948904f2f0f479b8f8197694b30184b0d2ed1c1cd2a1ec0fb85d299a192a447"),
    "/data/tool.jar",
)?;
hestia::log::info(&format!("{} bytes", jar.size));
```

`download(url, expect, dest) -> Result<Downloaded, Error>` where `Downloaded { size }`.

| `Expect` | Meaning |
|---|---|
| `Sha1(hex)`, `Sha256(hex)`, `Sha512(hex)` | the file must have this digest; a mismatch fails and leaves nothing at `dest` |
| `Unhashed` | nothing verifies the file; allowed only from a host the grant lists in `unhashed` |

## What the host checks

- **Permission.** The plugin needs `network:connect`. The URL must be https, on a host the grant names, and must not
  resolve to this computer or its network. Every redirect is held to the grant again, and with `Unhashed` every hop must
  also be on an `unhashed` host. A refusal is `denied` and names the rule.
- **Destination.** `dest` is a guest path in a directory that already exists under a writable mount of the current call:
  `/data`, or `/build` while a flavor's `build` runs. Anything else, including `..` and a symlink out of the mount, is
  `denied`. The file appears only once it is complete and verified.
- **Cache.** With a checksum, a file the engine already holds is copied from its cache, so repeating a download (every
  build of a flavor) costs no transfer. Cache the *result* in `/build` or `/data` and check for it first when copying
  is still too much.
- **Size.** A body over 4 GiB is refused while it streams (`invalid`).

## Progress, cancel and time

The transfer reports into the current call's progress, the same channel as `hestia::progress::report`: a command's job
or a flavor's provisioning. A download in a call that nothing watches reports nowhere. It stops when the call is
cancelled or out of time, leaving nothing at `dest`.

## Errors

| Case | Error |
|---|---|
| no route to the host, or the daemon is offline | `Error::Offline` |
| the server answered 404 | `Error::NotFound` |
| any other failing status | `Error::Upstream { status, .. }` |
| a host, hop, scope or destination the call may not use; no `network:connect` | `Error::Denied` |
| a malformed checksum; a body over the cap | `Error::Invalid` |
| a checksum mismatch, a cancel, an exhausted budget | `Error::Failed` |

Test fixture: the `probe` plugin's `download` command in `crates/engine/tests/fixture/plugins/probe/src/lib.rs`.
