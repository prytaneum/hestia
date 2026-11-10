# Running programs

`process:spawn` lets a plugin run native programs, so it is an unrestricted permission and the install states it as full
access to the computer. What narrows it: the plugin can run only the programs its manifest declares, with argument
patterns and environment names it declared, and there is no shell
([0101](../decisions/0101-plugins-spawn-only-declared-programs.md)). The grant, the declaration rules and the
`ipc:connect` alternative are in [permissions.md](permissions.md#programs); the manifest fields are in
[manifest.md](manifest.md#programs). This page is about running them.

## Declaring a program

Two kinds of `command`:

### PATH programs

A `command` without a `/` is a name on `PATH`. The host resolves it to an absolute path when the user approves the
install, shows that path, stores it on the plugin's record, and runs that file and no other. A later `PATH` change cannot
substitute another binary; if the file disappears the call fails until the plugin is installed again. A relative
directory on `PATH` is skipped.

```toml
permissions = ["process:read", "process:spawn"]

[[programs]]
id = "agent"
command = "sleep"
args = ["*"]
```

Source: `plugins/tunnel-demo/plugin.toml`. The example uses `sleep` as a stand-in for a real agent such as `tailscale`.

### Managed Java

A program with `java = <major>` instead of a `command` runs on the Java runtime Hestia manages for that major, installed
on first use. Nothing is pinned at approval, because the runtime belongs to the host, and the grant shows it as
"Hestia-managed Java 21". Arguments (`-jar`, a jar under `data/`, `-version`) are declared as patterns like any other
program.

```toml
[[programs]]
id = "tool"
java = 21
args = ["-jar", "*"]
```

Source: `crates/engine/tests/fixture/plugins/replay-flavor/plugin.toml`.

### Programs in `data/`

A `command` with a `/` is a file inside the plugin's `data/` directory and must carry a `sha256` per target triple:

```toml
[[programs]]
id = "agent"
command = "bin/playit"           # a file inside data/, run from a verified host-owned copy
args = ["**"]
env = ["PLAYIT_SECRET"]          # the only names a call may pass
[programs.sha256]
x86_64-unknown-linux-gnu = "…"   # one per target triple the plugin supports
x86_64-pc-windows-msvc = "…"
```

Source: [permissions.md](permissions.md#programs). No example plugin ships a `data/` program. The hash is 64 lowercase hex
characters; the triple is the Rust target triple of the host (`x86_64-unknown-linux-gnu`, `x86_64-unknown-linux-musl`,
`x86_64-pc-windows-msvc`, `x86_64-pc-windows-gnu`, `<arch>-apple-darwin`). A host whose triple has no entry refuses the
program.

## Agents that are downloaded and hashed

A package holds the manifest, the component, `assets/` and `themes/`, and nothing is copied into `data/` at install, so
an agent that is not on `PATH` is fetched at run time into `data/`. The host copies the file before it trusts it:

1. The plugin downloads the agent with `hestia::http::get` (needs `network:connect` and the download host in `network`;
   the body is at most 32 MiB; redirects are not followed, so request the final URL) and writes it under
   `proto::plugin::DATA_MOUNT` (`/data`) with `std::fs`, at the path the manifest's `command` names. On Windows the same
   name with `.exe` is tried too.
2. On the first `exec` or `spawn`, the host copies the file into `plugins/<id>/bin/<program id>`, a directory the plugin
   cannot write, hashes the copy against the `sha256` for this host's triple, sets the exec bit and renames it into
   place. Only the copy runs, so the plugin cannot swap the file between the check and the run, and rewriting `data/`
   afterwards changes nothing. A copy that already matches is reused.
3. A symlink in `data/` is refused. A file that does not match is `Error::PermissionDenied`, a missing one
   `Error::NotFound`.

The hash is in the manifest, so the plugin can run exactly the build it declared: publishing a new agent version means
shipping a new plugin version with the new hashes, which asks the user to approve again. No example plugin downloads an
agent; the SDK has no download helper, only the calls above.

## `exec` and `spawn`

Both take a program `id`, never a path. Arguments must fit the declared patterns on every call.

| | `exec` | `spawn` |
|---|---|---|
| For | a short command whose output you want | a long-lived agent |
| Returns | `Output { status, stdout, stderr }` once the program exits | a process id |
| Limits | each stream is cut at 1 MiB; a program that outlasts `timeout` or the time the call has left is killed, with everything it started; so is one still running when the call is cancelled | at most 8 running per plugin |
| Environment | empty, plus the declared names you pass (`exec_with`) | empty, plus the declared names you pass |
| Supervision | none; the child's whole tree dies with the call | the daemon's process supervisor, owner `plugin:<id>`; output captured |

```rust
let status = hestia::process::exec("tailscale", &["status", "--json"], Duration::from_secs(10))?;
let agent = hestia::process::spawn("agent", &["run"], &[("TS_AUTHKEY", "secret")])?;
hestia::process::stop(&agent)?;
```

Source: the doc example of `crates/sdk/src/process.rs` (a compiled no-run doctest); it declares `tailscale` and `agent`.

`exec_with(program, args, env, timeout, cwd)` is `exec` with declared environment variables and an optional `cwd`.

An `exec` argument may hold `${build}`, `${vanilla}` or `${data}`; the host replaces each with the host path of that mount
of the current call, after the argument is checked against the patterns (which match the placeholder as written). A
placeholder for a mount the call does not have, such as `${build}` outside a flavor's `build`, is `permission-denied`.
Nothing else in an argument is rewritten.

A program that needs other tools on its `PATH` lists them: `path = ["git"]` names other `PATH` programs of the manifest,
and the child's `PATH` is exactly their pinned directories, joined with the platform separator. An entry must name
another `PATH` program, once (`BadProgramPath`). The grant shows it. `spawn` gets the same `PATH`.

The rest of `hestia::process`:

| Call | Does |
|---|---|
| `stop(id)` | asks one of the plugin's own processes to stop |
| `status(id)` | `Status { state: Running \| Exited \| Killed, exit_code }` |
| `logs(id, tail)` | the last `tail` captured lines, at most 1000 |
| `list()` | the ids of the plugin's running processes |

A process the plugin did not start reads as `Error::NotFound`, so another plugin's ids cannot be probed.

Rules for every call:

- A program not declared, arguments outside the patterns, or an environment name not in `env` is
  `Error::PermissionDenied`. Without `process:spawn` every call is denied.
- At most 256 arguments of 32 KiB each, with no NUL.
- The working directory is `data/`; `exec_in(program, args, timeout, cwd)` (or `exec_with`) runs in another directory, a guest path under a writable mount of the call (`/data`, or `/build` during a flavor's `build`), and anything else is `permission-denied`. The environment starts empty (on Windows only `SystemRoot`).
- The host logs program ids and exit statuses, never arguments or values: they carry auth keys.

## Owning a long-lived agent

The plugin keeps nothing in memory between calls, so it records the process id in `hestia::data`; `list()` also finds a
spawned agent again after the daemon restarts. A spawned agent outlives the daemon like any workload, but disabling or
removing the plugin stops it.

The `tunnel-demo` example starts a stand-in agent when a server starts, saves its id under the server's id, and stops
it when the server exits. This is `plugins/tunnel-demo/src/lib.rs`, `up` and `down`, with the permissions
`process:read` and `process:spawn` and the events `process.started` and `process.exit` from its manifest:

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

The event handler that calls them is in [events.md](events.md#subscribing).

## Wrapping a launch

A [launch hook](hooks.md#wrapping-the-launch) may name a PATH program as its `wrapper`; the host runs the game through it.
That needs `process:spawn` and does not call `exec` or `spawn`.
