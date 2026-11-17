# `plugin.grant` is a committed lockfile, written for every platform

*Applies to: [Writing a plugin](../plugins/grant.md)*

**Decision.** A plugin project commits `plugin.grant`, the grant its built component asks for. `check`, `pack` and
`publish` fail with a diff when the derived grant differs; `check --update-grant` rewrites it; `publish` also refuses a
grant wider than the newest published version's unless `--allow-widen` says so. The file holds the programs (each with
its `os`) and the endpoints of every platform, so it is identical on every machine. Hestia never reads it.

**Why.** A dependency can widen the derived grant with no change to the manifest. The lockfile makes every widening a
reviewed line, as `Cargo.lock` does for versions. Writing it for every platform lets a Linux CI check a plugin that also
runs on Windows.

**Rejected.** Trusting the install prompt alone: the author never sees the widening. Deriving the lockfile for the
current platform: it would differ per machine and fail CI. Reading it in the host: it could then widen or narrow what
the host derives itself.
