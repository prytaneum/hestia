# Testing and `dev`

*[← Writing a plugin](README.md)*

A mistake that the install would refuse should fail in a unit test, in `check` or in `dev`, never in front of a user.

## The mock host

On a native build, the SDK's `hestia::mock` answers every `hestia:kernel` import from memory, so `cargo test` runs the
plugin's own code with no daemon and no WebAssembly. It checks each call against a grant the test sets, so a missing
permission or scope fails in the test with the same `Error::Denied("<permission>")` the host answers:

```rust
use hestia::mock::{self, Scope};

#[test]
fn fetching_without_the_permission_is_denied() {
    mock::reset();
    assert!(matches!(
        hestia::download::fetch("https://example.com/a", "a.bin", None),
        Err(hestia::Error::Denied(permission)) if permission == "network:connect"
    ));

    mock::grant(
        &["network:connect"],
        Scope { hosts: vec!["example.com".into()], unhashed: vec!["example.com".into()], ..Scope::default() },
    );
    assert!(hestia::download::fetch("https://example.com/a", "a.bin", None).is_ok());
    assert!(mock::calls().contains(&"hestia:kernel/download#fetch".to_owned()));
}
```

| Function                        | Does                                                                   |
|---------------------------------|------------------------------------------------------------------------|
| `mock::grant(&[names], scope)`  | sets the permissions held and the `Scope` (hosts, programs, endpoints) |
| `mock::entries(scope)`          | sets what `permissions::entries()` answers                             |
| `mock::confirm(bool)`           | what `notify::confirm` answers; declined by default                    |
| `mock::cancel(bool)`            | what `progress::cancelled` answers                                     |
| `mock::calls()`, `mock::logs()` | every import called, as `interface#function`, and every log line       |
| `mock::reset()`                 | nothing granted and no history                                         |

A successful call does the least that is observable: `download::fetch` writes nothing, `programs::run` exits 0 with no
output, an `ipc` connection reads end of stream. What the mock does not model:

- State and the grant are per thread, which is per test; a plugin that runs another thread sees nothing granted.
- There is no `/data`: `std::fs` on `/data/...` touches the real filesystem.
- `app::info().online` is always true, locks are not released when a call ends (call `reset`), and notices, links and
  `open_url` have no budget.
- An `ipc` permission left out of the real grant fails the plugin's load; the mock answers `denied`.

Test the logic, not the host: the sandbox, the limits and the prompt are Hestia's own tests.

## `hestia plugin test`

```bash
hestia plugin test [dir] [-- <cargo test args>]
```

Runs the project's own `cargo test` with the project's toolchain, natively; the generated CI runs it first.

## `hestia plugin world`

```bash
hestia plugin world [dir] [--wasm <file>]
```

Builds the component (or takes `--wasm`) and prints its imports and exports, the permissions the grant derives from them
(required or optional, with what each allows) and the plugins it needs. Use it to see what a dependency pulled in.

## `hestia plugin dev`

```bash
hestia config set plugins.developer true
hestia plugin dev [dir]
```

Installs the project as a dev install, rebuilds it on every change (without LTO, with the symbols a backtrace needs),
reloads it and streams the plugin's lines from the daemon's debug log. It needs the daemon and developer mode. The first
load asks for the plugin's permissions; each reload is an update of the dev install, so its settings, secrets, schedule
runs and granted optional permissions stay, and it asks again only when a rebuild widens what it asks for. A directory
created inside the project after `dev` started is not watched until it is restarted. `@unstable` imports link only here.

Developer mode adds diagnostics the user never sees:

- A denial in the plugin log names the interface and function that was refused, besides the permission.
- A trap logs the guest backtrace with symbols when the component carries them.

Turning `plugins.developer` off lists every dev install as `failed` until it is turned on again.
