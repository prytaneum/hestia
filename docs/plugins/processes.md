# Running programs

`process:spawn` lets a plugin run native programs, so it is an unrestricted permission and the install states it as full
access to the computer. What narrows it: the plugin can run only the programs its manifest declares, with argument
patterns and environment names it declared, and there is no shell
([0101](../decisions/0101-plugins-spawn-only-declared-programs.md)). The grant, the declaration rules and the
`ipc:connect` alternative are in [permissions.md](permissions.md#programs); the manifest fields are in
[manifest.md](manifest.md#programs). This page is about running them.

## Declaring a program

A `[[programs]]` entry says **what runs** (its identity) and **how it may be called** (its invocations). They are
separate so that one program serves every way a plugin needs it, instead of one near-identical entry per argument list
([0120](../decisions/0120-a-programs-identity-is-separate-from-its-invocations.md)).

The common case stays short: a program with one way of being called writes `args`, `env` and `path` at the top level.

```toml
[[programs]]
id = "git"
command = "git"
args = ["**"]
requires = { name = "Git", url = "https://git-scm.com/downloads" }
```

A program with several ways lists them as named `[[programs.invocations]]` and leaves the top level empty:

```toml
[[programs]]
id = "buildtools"
java = { min = 17, max = 21 }

[[programs.invocations]]
name = "build"
args = ["-jar", "${data}/BuildTools.jar", "--rev", "*", "--output-dir", "${build}"]

[[programs.invocations]]
name = "version"
args = ["-version"]
```

### Identity

Exactly one of three:

- **A `PATH` command**, `command = "git"`. A name without a `/`. See [pinning](#path-programs-are-pinned).
- **A managed Java**, `java = ...`: a runtime Hestia manages, installed on first use and never pinned (it belongs to the
  host). The grant shows it as "Hestia-managed Java 21", "Hestia-managed Java 17-21" or "Hestia-managed Java of the game
  version". Three forms:
  - `java = 21`, exactly that major.
  - `java = { min = 17, max = 21 }`, a range; **the call chooses the major** with `java_major`.
  - `java = "profile"`, the Java major the host uses for the Minecraft version the call is about. It resolves only in
    a call that carries a game version (a flavor's `build`); in any other call it is `invalid`. The call names no major.
- **A data file**, `command = "bin/playit"`: a path with a `/` inside the plugin's `data/`, which must carry a `sha256`
  per target triple (see [below](#programs-in-data)).

### Invocations

An invocation is a list of argument patterns plus the `env` names and `path` programs it may use. A literal matches
itself, `*` any one token, a trailing `**` any rest, including none; placeholders `${build}`, `${vanilla}` and `${data}`
are matched as written and replaced by the host afterwards. A call passes the program id, the arguments and the
environment; the host runs it through **the first invocation whose patterns match the arguments and which declares
every environment name the call passes**. None fits is `denied`, and the message names which rule failed (the
arguments, or an environment variable), never the arguments themselves. An invocation has no id the call passes: the
name exists so the grant and errors read clearly, and two invocations that overlap on `args` should differ in them.

```rust
hestia::process::exec_with("buildtools", &["-jar", "${data}/BuildTools.jar", "--rev", "1.21", "--output-dir", "${build}"],
    &[], Duration::from_secs(1200), Some("/build"), Some(21))?;
```

The last argument is the Java major, for a program declared with a range: it must lie inside the range (`denied`
otherwise) and a range program called without one is `invalid`. A program with a single major or `"profile"` takes
none, and a non-Java program rejects one.

### PATH programs are pinned

When the user approves a plugin, the host resolves each `PATH` program to an absolute path and records **the path and
the sha256 of the file there**. The approval shows both and the approval digest covers both. The program runs from that
file and no other, so a later `PATH` change or a binary planted earlier on `PATH` cannot substitute another. A relative
directory on `PATH` is skipped. At call time the host checks the file is still there and executable; it does not hash
it again (see [0120](../decisions/0120-a-programs-identity-is-separate-from-its-invocations.md) for why).

An update that would pin a different path, a file with different content, or no longer finds the program is held until
the user approves it again, and an unattended update never applies one.

A program **not found** when the plugin was approved is recorded as unpinned: the approval screen says so, and it
cannot run. If the program is installed later and the plugin calls it, the call fails (`denied`, "approve the plugin
again to pin it"), the program is not run, and the plugin goes to the `needs_approval` state, the same one an update that
asks for more lands in; installing the plugin again pins it under a new approval. A program not found at call time
either is `missing-requirement`.

### `missing-requirement` and `requires`

A declared program that is not on this computer fails with `missing-requirement` carrying a name and a link. They come
from the program's `requires = { name = "Git", url = "https://git-scm.com/downloads" }`, an `https` link; a program
without one is named by its command and has no link. The declaration grants nothing and is not part of the approval.

A flavor lists the programs it cannot do without in the `requires` of its description (ids of its own `[[programs]]`).
The host checks them each time it lists flavors, so the create wizard and `hestia server create` show "needs Git, get
it at ..." before a multi-minute build starts. See [providers.md](providers.md#flavors).

```toml
[permissions]
"process:read" = {}
"process:spawn" = { programs = ["agent"] }

[[programs]]
id = "agent"
command = "sleep"
args = ["*"]
```

Source: `plugins/tunnel-demo/plugin.toml`. The example uses `sleep` as a stand-in for a real agent such as `tailscale`.

### Programs in `data/`

A `command` with a `/` is a file inside the plugin's `data/` directory and must carry a `sha256` per target triple:

```toml
[[programs]]
id = "agent"
command = "bin/playit"           # a file inside data/, run from a verified host-owned copy
args = ["**"]
env = ["PLAYIT_SECRET"]          # the only names a call may pass
[programs.sha256]
x86_64-unknown-linux-gnu = "..."   # one per target triple the plugin supports
x86_64-pc-windows-msvc = "..."
```

The hash is 64 lowercase hex characters; the triple is the Rust target triple of the host (`x86_64-unknown-linux-gnu`,
`x86_64-unknown-linux-musl`, `x86_64-pc-windows-msvc`, `x86_64-pc-windows-gnu`, `<arch>-apple-darwin`). A host whose
triple has no entry refuses the program.

## Agents that are downloaded and hashed

A package holds the manifest, the component, `assets/` and `themes/`, and nothing is copied into `data/` at install, so
an agent that is not on `PATH` is fetched at run time into `data/`. The host copies the file before it trusts it:

1. The plugin fetches the agent with [`hestia::download`](downloads.md) (needs `network:connect` and the download host in
   its `hosts`; the daemon transfers, checks and caches the file, and nothing is followed past the granted hosts) into a
   directory that exists under `proto::plugin::DATA_MOUNT` (`/data`), at the path the manifest's `command` names. On Windows the same
   name with `.exe` is tried too.
2. On the first `exec` or `spawn`, the host copies the file into `plugins/<id>/bin/<program id>`, a directory the plugin
   cannot write, hashes the copy against the `sha256` for this host's triple, sets the exec bit and renames it into
   place. Only the copy runs, so the plugin cannot swap the file between the check and the run, and rewriting `data/`
   afterwards changes nothing. A copy that already matches is reused.
3. A symlink in `data/` is refused. A file that does not match is `Error::Denied`, a missing one
   `Error::MissingRequirement`.

The hash is in the manifest, so the plugin can run exactly the build it declared: publishing a new agent version means
shipping a new plugin version with the new hashes, which asks the user to approve again. No example plugin downloads an
agent; `hestia::download` fetches it and `hestia::hash::file` can check it again.

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

`exec_with(program, args, env, timeout, cwd, java_major)` is `exec` with declared environment variables, an optional
`cwd` and, for a program declared with a Java range, the major to run on. `spawn_on_java` is `spawn` with a major.

An `exec` argument may hold `${build}`, `${vanilla}` or `${data}`; the host replaces each with the host path of that mount
of the current call, after the argument is checked against the patterns (which match the placeholder as written). A
placeholder for a mount the call does not have, such as `${build}` outside a flavor's `build`, is `denied`.
Nothing else in an argument is rewritten.

An invocation that needs other tools on its `PATH` lists them: `path = ["git"]` names other `PATH` programs of the
manifest, and the child's `PATH` is exactly their pinned directories, joined with the platform separator. An entry must
name another `PATH` program, once (`BadProgramPath`). The grant shows it. `spawn` gets the same `PATH`.

The rest of `hestia::process`:

| Call | Does |
|---|---|
| `stop(id)` | asks one of the plugin's own processes to stop |
| `status(id)` | `Status { state: Running \| Exited \| Killed, exit_code }` |
| `logs(id, tail)` | the last `tail` captured lines, at most 1000 |
| `list()` | the ids of the plugin's running processes |

A process the plugin did not start reads as `Error::NotFound`, so another plugin's ids cannot be probed.

Rules for every call:

- A program not declared, arguments no invocation matches, or an environment name the matching invocation does not
  declare is `Error::Denied`. Without `process:spawn` every call is denied.
- At most 256 arguments of 32 KiB each, with no NUL.
- The working directory is `data/`; `exec_in(program, args, timeout, cwd)` (or `exec_with`) runs in another directory, a guest path under a writable mount of the call (`/data`, or `/build` during a flavor's `build`), and anything else is `denied`. The environment starts empty (on Windows only `SystemRoot`).
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
