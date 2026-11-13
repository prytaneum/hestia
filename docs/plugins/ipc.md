# Local connections

A plugin can talk to a program running on the same computer, over a Unix socket or a Windows named pipe. This is how a
plugin reaches a running Discord, or any local program, speaking that program's protocol itself. The design is in
[architecture/plugins.md](../architecture/plugins.md#local-endpoints).

## Granting an endpoint

```toml
permissions = ["ipc:connect"]
ipc = ["discord-ipc-*"]
```

`ipc:connect` is an unrestricted permission, and `open` refuses a name the manifest's `ipc` list does not grant. An entry
is a relative name whose last segment may end in `*`; a bare `*` is refused. See [permissions.md](permissions.md).

## Using a connection

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

Source: the module documentation of `crates/sdk/src/ipc.rs` (a no-run doctest).

- Every call runs on a fresh instance, so the host keeps a connection open between calls. Keep its id (with
  [`hestia::data`](data.md)) or find it again with `ipc::connections()`, and take it up with `Connection::resume(id)`.
- At most 8 connections are open per plugin, a message is at most 1 MiB, and every connection closes when the plugin is
  disabled, removed, reloaded or taken out of service.
- `receive(max, wait)` returns what has arrived, at most `max` bytes, waiting up to `wait` for the first byte, and an
  empty list when nothing came in time. It answers `not-found` once the peer has closed the connection and everything
  it sent was read.
- The host never logs the bytes that pass.

## Hearing that something arrived

The host reads each connection itself, so a plugin need not poll `receive` from a [schedule](events.md#schedules).
Subscribe to `ipc.*` in `contributes.events` and handle two topics in `Plugin::event`:

| Topic | Payload | When |
|---|---|---|
| `ipc.readable` | `IpcReadableEvent { connection }` | bytes arrived, and the plugin has not been told since it last called `receive` |
| `ipc.closed` | `IpcClosedEvent { connection }` | the peer closed the connection or it failed; once |

They reach only the plugin that owns the connection and are delivered like every other event, under the
[call budget](events.md#delivery). The exact rules, and an example handler, are in
[events.md](events.md#local-connections).
