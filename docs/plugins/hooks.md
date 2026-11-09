# Hooks

A hook point is a moment an operation offers every plugin that declares it a chance to adjust or refuse it. A hook is
customisation, not a control: it is bounded and fails open, so nothing may present one as a policy or a scanner
([0095](../decisions/0095-interceptors-are-bounded-and-fail-open.md)). Points are data in `proto::hook`; the ABI carries
every one through a single export, `hooks.intercept(point, context)`, so a new point changes neither the WIT nor the SDK
trait.

## The points

| Point | Runs | Context | Patch | Permissions to declare it |
|---|---|---|---|---|
| `before-launch` | once per instance launch, after the plan is assembled | `LaunchContext` | `LaunchPatch` | `launch:patch`, `account:read` |
| `before-server-start` | once per server start, before spawn | `ServerContext` | `LaunchPatch` (game arguments ignored) | `launch:patch`, `account:read` |
| `before-content-install` | once per file about to land in an entry | `InstallContext` | none (`Empty`) | `content:read` |

`LaunchContext` carries the instance id and name, game version, flavor, loader version, Java path, configured memory, the
quick-play target and the profile name and UUID. It never carries the access token or the raw command line.
`ServerContext` carries id, name, version, flavor, port and memory. `InstallContext` carries the entry, its kind, the
source and project and version ids (empty for a local import), the file name, its sha1 (empty when the source publishes
none) and the content kind; never the file's bytes. The fields are in `crates/proto/src/hook.rs`.

Declare the points in the manifest; declaring one without its permissions is refused at `pack` and at install
(`HookPermissionMissing`):

```toml
permissions = ["launch:patch", "account:read"]

[contributes]
hooks = ["before-launch"]
```

Source: `plugins/launch-flags/plugin.toml`. The [`launch-flags`](https://github.com/prytaneum/plugin-examples/tree/main/plugins/launch-flags) example adds garbage collector
flags chosen by a `select` setting.

## Answering

Route each point to a typed handler with `Intercept::on`. The handler returns the patch to add, or an `Err(reason)` that
vetoes the operation. This is the doc example of `crates/sdk/src/hooks.rs` (a compiled no-run doctest):

```rust
use hestia::hooks::{Intercept, Intercepted};
use hestia::proto::hook::{BeforeLaunch, LaunchPatch};

struct Tuner;

impl hestia::Plugin for Tuner {
    fn intercept(call: Intercept) -> Intercepted {
        call.on::<BeforeLaunch>(|context| {
            let mut patch = LaunchPatch::default();
            if context.memory.is_none() {
                patch.extra_jvm_args.push("-XX:+UseZGC".into());
            }
            Ok(patch)
        })
        .finish()
    }
}

hestia::plugin!(Tuner);
```

The same shape in `plugins/launch-flags/src/lib.rs`, reading the plugin's setting:

```rust
call.on::<BeforeLaunch>(|context| {
    let collector = hestia::settings::get::<String>("collector")
        .ok()
        .flatten()
        .unwrap_or_default();
    let flags: &[&str] = match collector.as_str() {
        "zgc" => &["-XX:+UseZGC"],
        _ => &["-XX:+UseG1GC", "-XX:MaxGCPauseMillis=130"],
    };
    hestia::log::info(&format!("{collector} flags for {}", context.instance_name));
    Ok(LaunchPatch {
        extra_jvm_args: flags.iter().map(|flag| flag.to_string()).collect(),
        ..LaunchPatch::default()
    })
})
.finish()
```

A point no handler takes adds nothing. A context that does not decode is logged and answered as no patch.

A veto, from the engine test fixture `crates/engine/tests/fixture/plugins/probe/src/lib.rs`:

```rust
call.on::<BeforeContentInstall>(|context| {
    let refused: Vec<String> = hestia::data::load("content")
        .ok()
        .flatten()
        .unwrap_or_default();
    match refused.contains(&context.file_name) {
        true => Err("not on this list".into()),
        false => Ok(Empty {}),
    }
})
```

## Failing open, vetoing, ordering

- Each plugin runs under a 2 second limit on a fresh instance. A timeout, a trap, a refused re-entry or an answer that is
  not the point's patch skips that plugin and adds a `PluginHookFailed` warning to the operation's result; the operation
  goes ahead.
- Only an `Err` from the handler stops it: the run ends at that plugin, later plugins are not called, and the operation
  fails with `PluginVetoed { plugin, hook, reason }`. For `before-content-install` the veto refuses that file only, the
  rest of the batch installs, and the reason is prefixed with the file name.
- Plugins run in plugin-id order, enabled and loaded ones whose grant lists the point, and their patches fold in that
  order. The id is stable across reinstalls, so the same plugins give the same launch.
- A server start and a content install hold the entry's claim while the hook runs, at most 2 seconds per plugin; an
  instance launch holds none.

## What a launch patch may add

`LaunchPatch` has three fields: `extra_jvm_args`, `extra_game_args` and `wrapper`. The host keeps what an allowlist
(`crates/engine/src/plugins/patch.rs`) names and drops the rest, one `LaunchArgDropped { plugin, arg }` warning each. A
new JVM option is refused until someone decides it is safe.

JVM arguments (`extra_jvm_args`):

- `-XX:+Flag` or `-XX:-Flag` for `UseG1GC`, `UseZGC`, `UseParallelGC`, `ZGenerational`, `UnlockExperimentalVMOptions`,
  `AlwaysPreTouch`, `ParallelRefProcEnabled`, `DisableExplicitGC`.
- `-XX:Name=value` for `MaxGCPauseMillis`, `G1NewSizePercent`, `G1ReservePercent`, `G1MixedGCCountTarget`,
  `InitiatingHeapOccupancyPercent`, `ParallelGCThreads`, `ConcGCThreads` (digits, at most 9), and `G1HeapRegionSize`,
  `MetaspaceSize`, `MaxMetaspaceSize`, `ReservedCodeCacheSize` (digits with an optional `k`, `m` or `g`).
- `-Dkey=value` only for a key the manifest listed in `[launch] properties` ([manifest.md](manifest.md#launch)) and that
  is not on the hard deny list; the value is at most 1024 characters with no control characters.
- No code-loading option (`-javaagent`, `-cp`, `--module-path`, …) passes, and the heap (`-Xmx`) is not on the list.

Game arguments (`extra_game_args`): `--fullscreen`, and `--width N` or `--height N` with `N` up to 9 digits. Ignored for a
server start: every one is dropped with a warning, since a server has no window.

### Wrapping the launch

`wrapper` names one of the plugin's own `[[programs]]`; the host runs the game or server command through it, such as
`gamemoderun` or `mangohud`. It needs `process:spawn` and a program whose `command` is a name on `PATH`, run by the
absolute path pinned at approval ([processes.md](processes.md#path-programs)). An unknown program, a program in `data/`,
a missing permission or a file that vanished drops the wrapper with the same `LaunchArgDropped` warning. Wrappers nest;
the first plugin in id order is the outermost.

```toml
permissions = ["launch:patch", "account:read", "process:spawn"]

[[programs]]
id = "mangohud"
command = "mangohud"

[contributes]
hooks = ["before-launch"]
```

```rust
LaunchPatch {
    wrapper: Some("mangohud".into()),
    ..LaunchPatch::default()
}
```

No example plugin wraps a launch; the manifest and the value above are written from `Program` and
`proto::hook::LaunchPatch` and have not been compiled into a plugin.

## Limits to design for

- A hook sees a launch or a start, not its secrets, and cannot change the account, the Java runtime or the game version.
- It cannot be relied on: a disabled, slow or crashing plugin lets the operation through. The content hook is a
  convenience check for a known-incompatible mod, not a malware scanner.
