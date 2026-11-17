# The Hestia WIT

`kernel/` is the package `hestia:kernel`: one file per interface. Built-in plugins
carry the WIT of their own packages (`hestia:servers`, ...) beside their adapters.

## Version policy

A package's version is the API version. A plugin's `plugin.toml` may record the one it
was built against as `api = "0.1.0"`, which its author sets (packing it from the
component's imports lands with the plugin tooling); the host refuses an `api` that is not
the version the component imports, or an import or export that is not on the host's own
`0.y` line, is a pre-release, or is later than the host's. `0.0.z` must match exactly.

- **Additive** (`0.y.z+1`): a new function or interface, in a new interface marked
  `@since(version = 0.y.z+1)`. wasmtime links an older guest against a newer host of the
  same `0.y` line.
- **Breaking** (`0.y+1`): removing or changing a function, adding a field to a record,
  adding a case to a variant that has no `other`. A new function takes the new record.
- **Experimental**: an interface marked `@unstable(feature = ...)`. Only a dev install in
  developer mode may import it, and it may change in any release. Publishing will refuse
  it once the tooling exists. A real unstable interface also needs `features: [...]` in
  the host's `bindgen!`, which drops it otherwise; the inspection gate is then its only
  protection, and a dev install never runs without developer mode.
- **Deprecated**: `@since(...) @deprecated(version = ...)`, at least one `0.y` line
  before removal. `plugin.inspect` reports a deprecated import as a warning.

Version attributes live on interfaces only: the host gates and the tables track an
interface, never a function. An additive or experimental function goes in a new interface
carrying the attribute, never as an attributed function inside an existing one;
`crates/kernel/tests/wit.rs` fails an `@unstable` function and any `@since` or
`@deprecated` one that its interface does not carry.

A published version never changes. `crates/kernel/tests/wit.rs` fails a change to the
`.wit` files of `kernel/` or of a built-in's package under an unchanged version, comparing
the token stream (comments and layout do not count; attributes do, so changing an
`@unstable` interface needs a version bump too). It verifies only the current version's
line of `snapshot.txt`; rewriting an older line is caught by review. After raising a
package version, record the new hash (the failing test prints it) as a line of
`snapshot.txt`. Keep `KERNEL_VERSION` in `crates/kernel/src/grant/table.rs` equal to the
package version, and give an interface with `@since`, `@unstable` or `@deprecated` the
same stability in its package's `Table`; the test checks both ways.
