# Runtime API

What a plugin can do while it runs, through the SDK (`hestia-sdk`, imported as `hestia`). Every call runs on a fresh
instance of the plugin, so nothing kept in memory survives to the next call; keep state in [data](#data) or
[settings](#settings).

Failures are one typed `hestia::Error`: `NotFound`, `Invalid`, `Denied`, `Unsupported`, `Offline`, `Upstream`,
`RateLimited`, `SignInRequired`, `NotConfigured(key)`, `MissingRequirement`, `Busy` and `Failed`. How a provider's
errors reach the user is in [providers.md](providers.md#errors).

## The sandbox

A plugin sees no host path. Its filesystem is a few directories mounted at fixed guest paths for one call; TCP, UDP and
name lookup are off and the environment is empty.

| Guest path | Access | Present |
|---|---|---|
| `/data` | read-write, `plugins/<id>/data/` | every call; the only place state outlives a call |
| `/build` | read-write, the plugin's build area, shared by all its entries and kept between calls | a flavor's `build` |
| `/vanilla` | read-only, the side's vanilla jar alone | a flavor's `build`, when the side has a jar |
| `/files` | read-only, the files being identified, as `/files/0`, `/files/1`, ... | a content source's `identify` |

A path you hand a host call (a working directory, a download destination) must be a plain path at or under a *writable*
mount of the current call: `..`, an empty component and a symlink out of the mount are `denied`. Package files
(`assets/`, `themes/`) are served by the daemon, never mounted.

## Data

`hestia::data` keeps typed JSON files in `/data`, named by a key of 1 to 64 of `A-Z a-z 0-9 - _` (stored as
`<key>.json`).

```rust
let mut counter: Counter = hestia::data::load("counter")?.unwrap_or_default();
counter.runs += 1;
hestia::data::save("counter", &counter)?;
```

`save` writes aside and renames, so a reader never sees half a file, but it is last-writer-wins. Calls of one plugin can
overlap (an event handler and a command), so a key that is changed concurrently goes through `update`, which does the
read-modify-write under a host-held lock:

```rust
let runs = hestia::data::update("counter", |counter: &mut Counter| {
    counter.runs += 1;
    counter.runs
})?;
```

The lock is released when the call ends, however it ends. A wait is bounded and fails with `Error::Busy`, so keep the
closure quick. `update` on the same key from inside its closure is `Error::Invalid`; a call holds at most 8 locks and a
plugin uses at most 64 names.

## Settings

Settings are declared in the manifest ([manifest.md](manifest.md#settings)) and stored owner-only in
`plugins/<id>/settings.json`.

```rust
let key: Option<String> = hestia::settings::get("api-key")?;   // user value, else default, else None
hestia::settings::set("token", &"s3cret")?;
hestia::settings::clear("token")?;
```

A secret arrives in clear, to this plugin only. `Plugin::on_settings_changed(key)` runs when the user changes a key. A
plugin writes only its own settings, validated exactly like the user's writes (an undeclared key is `NotFound`; a wrong
kind, an unlisted `select` option or a string over 4096 bytes is `Invalid`). `on_settings_changed` does not run for the
plugin's own writes, so it cannot loop.

## HTTP

`network:connect` reaches only the granted hosts. Only `https://` URLs to named hosts are allowed, redirects are not
followed (a `3xx` comes back as it is; request the `Location` yourself, and it must be granted too), and the host limits
the body.

```rust
use hestia::http::{Request, Url};

let url = Url::new("https://api.example.com/v2")?
    .segment("search")
    .query("query", "fabric api")
    .query("limit", 20);

let hits: Vec<Hit> = Request::get(url)
    .header("authorization", format!("Bearer {key}"))
    .send_json()?;
```

- `Request::get/head/post/put/patch/delete(url)`, then `.header(name, value)`, `.body(content_type, bytes)`,
  `.json(&value)?` or `.form(pairs)`. `.send()?` returns the `Response` whatever the status; `.send_json::<T>()?` also
  applies `error_for_status` and decodes the body.
- `Response` has `status`, `headers` (`get`, `get_all`; case-insensitive), `body`, `text()`, `json::<T>()`, the status
  class checks (`is_success`, ...) and `retry_after()`.
- `Url` percent-encodes every segment and query value, so `"a b&c/é"` stays inside its component.
- A call that cannot be made is `Denied` (host not granted), `Offline` (unreachable or timed out) or `Failed`. An
  answer, even a `500`, is a `Response`.

`response.error_for_status()?` passes a `2xx` and maps the rest: `404` to `NotFound`; `429` to
`RateLimited { retry_after }`; everything else to `Upstream { status, retry_after, message }`. `401` and `403` stay
`Upstream` because only the plugin knows whether that means a wrong key, so map them where known:

```rust
let reply = match Request::get(url).send()?.error_for_status() {
    Err(Error::Upstream { status: Some(401 | 403), .. }) => {
        return Err(Error::NotConfigured("api-key".into()))
    }
    other => other?,
};
```

`hestia::hash` has `sha1`, `sha256` and `sha512` over bytes (lowercase hex) and `file(path, algorithm)`, which reads in
chunks so a large file is never held whole.

## Downloads

For a file on disk (a tool, an installer, a jar), ask the daemon to fetch it. It transfers, verifies and caches the
file, and reports progress and cancel; use `hestia::http` for API calls and small documents.

```rust
use hestia::download::{download, Expect};

let jar = download(
    "https://files.example.com/tool-1.4.jar",
    Expect::Sha256("a948904f2f0f479b8f8197694b30184b0d2ed1c1cd2a1ec0fb85d299a192a447"),
    "/data/tool.jar",
)?;
hestia::log::info(&format!("{} bytes", jar.size));
```

`Expect` is `Sha1`, `Sha256`, `Sha512` (a mismatch fails and leaves nothing at `dest`) or `Unhashed` (allowed only from
a host in the grant's `unhashed`; every redirect hop must be on one too). The URL must be https on a granted host and
must not resolve to this computer or its network. `dest` is a guest path in an existing directory under a writable mount
(`/data`, or `/build` during a flavor's `build`); the file appears only when complete and verified. A body over 4 GiB is
refused. Errors: `Offline`, `NotFound` (404), `Upstream` (other statuses), `Denied` (host, scope or destination),
`Invalid` (malformed checksum, oversized body), `Failed` (mismatch, cancel, out of time).

## Progress

`hestia::progress::report(Some(0.5), "halfway")` reports a fraction (`None` leaves it indeterminate) into the current
call: a command's job or a flavor's provisioning. `hestia::progress::cancelled()` turns `true` when the caller cancels.
A cancel only asks: the call is dropped 10 seconds later if it has not returned.

## Programs

`process:spawn` runs the programs declared in the manifest. Calls name a program id, never a path, and the arguments
must fit a declared invocation: the host runs the first invocation whose patterns match the arguments and which declares
every environment name the call passes. Anything else is `Denied`, naming the rule that failed but not the arguments.
There is no shell, the working directory is `/data` unless `exec_in` or `exec_with` names another writable mount, and
the environment starts empty. At most 256 arguments of 32 KiB each.

| | `exec` | `spawn` |
|---|---|---|
| For | a short command whose output you want | a long-lived agent |
| Returns | `Output { status, stdout, stderr }` when it exits | a process id |
| Limits | each stream cut at 1 MiB; killed with its whole tree on `timeout`, when the call runs out of time or is cancelled | at most 8 running per plugin |
| Supervision | none | the daemon's supervisor, owner `plugin:<id>`; output captured |

```rust
let status = hestia::process::exec("tailscale", &["status", "--json"], Duration::from_secs(10))?;
let agent = hestia::process::spawn("agent", &["run"], &[("TS_AUTHKEY", "secret")])?;
hestia::process::stop(&agent)?;
```

Source: the doc example of `crates/sdk/src/process.rs`.

- `exec_with(program, args, env, timeout, cwd, java_major)` adds declared environment variables, a working directory
  and, for a program declared with a Java range, the major to use (inside the range, or `Denied`; a range program called
  without one is `Invalid`). `exec_in` sets only the directory; `spawn_on_java` is `spawn` with a major.
- An argument may hold `${data}`, `${build}` or `${vanilla}`; the host substitutes that mount's host path after
  matching. A placeholder for a mount the call lacks is `Denied`.
- Also: `stop(id)`, `status(id)` (`Running`, `Exited` or `Killed`, with an exit code), `logs(id, tail)` (at most 1000
  lines) and `list()`. A process the plugin did not start reads as `NotFound`.
- The host logs program ids and exit statuses, never arguments or environment values.

A missing `PATH` program fails with `MissingRequirement` carrying the `name` and `url` of the program's `requires`. A
flavor lists the programs it cannot do without in its `info`, so the create wizard says "needs Git" before a long build.

### Agents that are downloaded and hashed

A package holds only the manifest, component and assets; nothing is copied into `data/` at install. An agent not on
`PATH` is fetched at run time:

1. Download it with `hestia::download` into `/data`, at the path the manifest's `command` names (on Windows the name
   with `.exe` is tried too).
2. On the first `exec` or `spawn` the host copies the file to a directory the plugin cannot write, checks the copy
   against the `sha256` for this host's target triple, sets the exec bit and runs only that copy. A symlink in `data/`
   is refused; a mismatch is `Denied`, a missing file `MissingRequirement`.

A new agent version means a new plugin version with new hashes, which asks the user to approve again.

A spawned agent outlives the daemon like any workload, but disabling or removing the plugin stops it. The plugin keeps
the process id in `data` (or finds it again with `list()`). The `tunnel-demo` example starts an agent on
`process.started` and stops it on `process.exit`:

```rust
fn up(server: &str) -> Result<(), Error> {
    let agent = hestia::process::spawn("agent", &["86400"], &[])?;
    hestia::log::info(&format!("tunnel up for {server}"));
    hestia::data::save(server, &agent)
}

fn down(server: &str) -> Result<(), Error> {
    let Some(agent) = hestia::data::load::<String>(server)? else {
        return Ok(());
    };
    hestia::process::stop(&agent)?;
    hestia::log::info(&format!("tunnel down for {server}"));
    Ok(())
}
```

Source: `plugins/tunnel-demo/src/lib.rs`.

## Local sockets

`ipc:connect` lets a plugin talk to a program on the same computer over a Unix socket or Windows named pipe, speaking
that program's protocol itself. `open` refuses a name the grant does not list.

```rust
use std::time::Duration;
use hestia::ipc::{self, Connection};

let chat = match ipc::connections()?.pop() {
    Some(open) => open,
    None => ipc::open("chat-ipc-0")?,
};
chat.send(b"hello")?;
let reply = chat.receive(4096, Duration::from_millis(500))?;
```

Source: the module documentation of `crates/sdk/src/ipc.rs`.

The host keeps a connection open between calls. Keep its id in `data` or find it with `ipc::connections()`, and take it
up with `Connection::resume(id)`. At most 8 connections per plugin, a message is at most 1 MiB, and every connection
closes when the plugin is disabled, removed or reloaded. `receive(max, wait)` returns up to `max` bytes, waiting up to
`wait` for the first, and is empty when nothing came; it answers `NotFound` once the peer has closed and everything was
read. To be told when bytes arrive instead of polling, see [local connection events](events.md#local-connections).

## Messages

`PageContext.locale` is the app's language. A package may carry `messages/<locale>.json`, a flat object of texts by key
with `{name}` placeholders (at most 256 KiB each). `hestia::messages::Messages::load(locale)` picks the best match (exact
locale, then language, then `en`), and `format("key", &[("name", "value")])` fills placeholders; a missing key reads as
itself. The app translates the manifest's own texts from the same file: `page.<id>.title`, `setting.<key>.label`,
`setting.<key>.help`, `setting.<key>.placeholder` and `group.<name>`.
