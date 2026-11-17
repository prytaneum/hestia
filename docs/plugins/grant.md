# The grant lockfile

*[← Writing a plugin](README.md)*

Hestia derives a plugin's grant from its component, so a dependency that starts calling a new function widens the grant
with no change to your manifest. `plugin.grant` makes that visible: it is the committed record of what the built
component asks for, as `Cargo.lock` is for versions. Hestia never reads it; it informs review and cannot widen anything.

```toml
# plugin.grant - written by `hestia plugin check --update-grant`, committed, never edited by hand
api = "0.1.0"

[permissions]
"network:connect" = { hosts = ["api.example.com"], unhashed = ["api.example.com"] }
"process:spawn" = { programs = [{ path = "/usr/bin/git", args = ["gc"], os = "linux" }] }
"secrets" = { optional = true }

[schedules]
"nightly" = "1d"

[contributes]
exports = ["hestia:kernel/commands@0.1.0", "hestia:kernel/lifecycle@0.1.0", "hestia:kernel/on-schedule@0.1.0"]
```

It also lists `requires` (the packages and plugins it needs), `imports` (interfaces from packages other than the
kernel's, because the CLI cannot see those permissions) and `[dependencies]` when the manifest has any. It holds the
programs (with the `os` each is written for) and the endpoints of every platform, so it is identical on every machine.

## `hestia plugin check`

```bash
hestia plugin check [dir] [--wasm <file>] [--update-grant]
```

Builds the component for `wasm32-wasip2` (or takes `--wasm`) and applies the rules the host applies at install: the
manifest parse, the component's shape, the version gates and the grant derivation. It prints every warning, then every
problem, and fails on any. Each stage stops at its first problem, so a run shows at most one per stage, plus the
lockfile's. Warnings include an import of a deprecated interface. An `@unstable` import is an error here.

If the build asks for something other than `plugin.grant`, `check` fails with a diff (`-` locked, `+` built).
`--update-grant` rewrites the file once you have reviewed the change; commit it with the code that caused it. `pack` and
`publish` hold the same line, and `publish` also compares the grant with the newest published version's, so a release
that asks for more needs `--allow-widen`. See [Packing and publishing](publishing.md).
