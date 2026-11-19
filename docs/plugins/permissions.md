# Permissions

*[← Writing a plugin](README.md)*

The API is the `hestia:kernel` WIT package. Each permission is exactly one import interface, so what a component asks
for is what it imports, and an interface the user did not grant is linked as a stub: its functions answer
`denied("<permission>")`, naming the permission. Calling a function adds its permission to the grant; each function's
rustdoc in the SDK says which. Run `hestia plugin world` to see what your build asks for.

## What needs a permission

| Interface                                        | Permission        | Tier         | Scope (manifest)    |
|--------------------------------------------------|-------------------|--------------|---------------------|
| `log`, `app`, `messages`, `progress`, `settings` | none              |              |                     |
| `data` (named locks), `permissions`, `types`     | none              |              |                     |
| `notify` (`notify`, `confirm`, `open-url`)       | none, budgeted    |              |                     |
| `jobs` (`start`)                                 | none, budgeted    |              |                     |
| `secrets`                                        | `secrets`         | sandboxed    |                     |
| `download`, and `wasi:http`                      | `network:connect` | sandboxed    | `hosts`, `unhashed` |
| `programs`                                       | `process:spawn`   | unrestricted | `programs`          |
| `ipc`                                            | `ipc:connect`     | unrestricted | `endpoints`         |

An *unrestricted* permission reaches native code or local services, and the prompt says so for that permission alone.
Your own files are the WASI `/data` preopen, which needs nothing: it is the only directory the plugin sees (backed by
`<data_home>/sandbox/<id>/`). The environment is empty, and TCP, UDP and name lookup are off; reaching outside goes
through the interfaces above.

## Network: `network:connect`

`hosts` lists exact names or `*.domain` wildcards, which match exactly one label and never a public suffix (`*.com` and
`*.github.io` are refused). Every request, from `download` or `wasi:http`, is held to https, no IP literal, no
credentials in the URL, a granted host, a resolver that refuses addresses inside the machine, 30 s and 32 MiB, and it is
checked again on every redirect. A plugin that is offline reaches nothing (`download` answers `offline`; check
`app::info().online`).

`download::fetch(url, path, sha256)` streams into `/data`: `path` is relative with no `..`, every name valid on Windows too (no `\`, `:`, `con`, `nul.txt`, trailing dot or space),
directories are created, a fetch that would take `/data` past 8 GiB fails (so does one while your own writes have it over), the
file is staged as `.part` and renamed, and a link inside `/data` that leaves it is refused. Without a SHA-256 the host
must be listed in `unhashed`; prefer the checksum. `/data` is measured before each schedule tick; while it holds more than 8 GiB the plugin gets no ticks, and its other calls still run so it can delete files.

## Programs: `process:spawn`

```toml
[permissions."process:spawn"]
programs = [{ path = "/usr/bin/git", args = ["gc", "--auto"], os = "linux" }]
```

An entry is an absolute path, its exact arguments and optionally the one OS (`linux`, `windows`, `macos`) it is for;
only the current OS's entries are granted. Without `os` the path's form decides the platform. A unix path starts with
`/`; a Windows path is a drive path to an `.exe` (`C:\tools\tool.exe`), never a network (`\\server\...`) or device path. `programs::run(path, args)` must name a declared invocation. The program is
pinned by its SHA-256 when the user approves, so a program that changes fails its approval at the next start and is
refused at the call. It runs with no shell, an empty environment (Windows keeps the few system variables programs need to start), `/data` as its directory and at most 1 MiB of each
output stream, in its own process group that ends with the call. A program inside a plugin's sandbox directory or one of
Hestia's own binaries is refused.

## Local sockets: `ipc:connect`

`endpoints` lists exact unix socket paths or Windows pipe names; only the current platform's form is granted, and
Hestia's own endpoint is refused. `ipc::connect` returns a connection with `write` and `read`, closed when dropped or
when the call ends (at most 8 open). Because the interface holds a resource it cannot be answered `denied`: a plugin
that imports `ipc` without the permission fails to load, and `ipc:connect` cannot be optional.

## Optional permissions

Mark a permission `optional = true` in its `[permissions."<name>"]` table when the plugin can work without it. The
prompt shows it apart from the required ones, unticked. The user ticks it at approval or grants it later (`hestia plugin
grant <id> <permission>`), and revokes it the same way; a revoke halts the plugin's running commands. Until granted, its
interface answers `denied`, so adapt with `hestia::permissions::granted("secrets")` instead of failing. The plugin is
relinked on every change; a call already running keeps the link it began with.

An update keeps a granted optional permission only while it is optional in both versions with a scope no wider. One
added back after being dropped, or with a wider scope, arrives declined. An update that only adds an optional permission
does not ask again; one that makes an optional permission required does.

## Entries

Permissions of a built-in's package that act on servers or instances take an **entries scope** the user picks (all, or
chosen entries, changeable with `hestia plugin entries`), never the manifest. No `hestia:kernel` permission takes it,
and no built-in package ships yet, so there is nothing to scope today; `hestia::permissions::entries()` reports what the
user approved once there is.

## Approval and updates

The prompt shows the plugin's permissions with their scopes, its schedules, its requirements and its signer, and the
approval covers the exact package: one that changes between the prompt and the install is refused. An update asks again
when it adds a required permission, a scope entry (a host, an endpoint, an unchecked host, a program with another path,
arguments or digest), a required package, an export or a schedule, or one that ticks more often. A grant that widened
because a dependency changed is caught earlier by [the lockfile](grant.md).
