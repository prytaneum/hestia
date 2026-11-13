# A program's identity is separate from its invocations; pins are lazy and approved

*Applies to: [Plugins](../architecture/plugins.md#spawning-programs), [running programs](../plugins/processes.md), [the manifest](../plugins/manifest.md#programs)*

Supersedes in part [0101](0101-plugins-spawn-only-declared-programs.md): a program fused **what runs** with **how it may
be called**, the pin recorded only a path, and a program missing at approval could never be used later.

Porting Spigot's BuildTools showed the cost. The plugin declared `buildtools17`, `buildtools21` and so on, one program
per Java major with the same arguments, because `java = <major>` sat on the program beside the argument patterns. A
missing `git` surfaced as an opaque failure at build time, minutes in, and a program not found at install could be used
only by reinstalling.

**A program is an identity and its invocations.** The identity is exactly one of a `PATH` command, a managed Java
(`java = 21`, a `{ min, max }` range, or `"profile"`) or a data file with per-target `sha256`. The invocations are the
named argument-pattern lists with the `env` names and `path` programs each may use. A single-invocation program writes
`args`, `env` and `path` at the top level; several use `[[programs.invocations]]`. A call names the program and passes
the arguments and environment; the host picks the **first invocation whose patterns match and which declares every
variable the call passes**. WIT keeps its call shape and gains `java-major: option<u32>` on `exec` and `spawn`:
matching needs no invocation id (an id would let a call name an invocation its arguments do not fit, and the patterns
are checked either way), and the one input that is a choice rather than a derivation, the Java major, gets its own
parameter. A range program must be given a major inside it (`denied` outside, `invalid` when absent); `"profile"` is the
major the host uses for the game version of the call (`CallContext::game_java`, set by a flavor's `build` from the
profile's `java_major`) and is `invalid` in any call without one; an exact major takes none or that one. The matcher and
the Java choice are `Program::matching` and `invocation::select`, pure functions of the program and the call, so what
runs programs on the host's behalf reuses them.

**The grant entry** (`Program::entry`) is the whole definition on one line, identity first, then each invocation. Any
change to identity or to any invocation is a different entry and therefore a widening, as before.

**A pin is a path and a content hash, and the user approves every pin.** At install or update the host resolves each
`PATH` program, hashes the file and shows both; the approval digest binds both (`Grant::approval_digest` takes
`ProgramPin`s). Pins are recorded on the plugin record. A program **not found** at approval is recorded as unpinned.
Pinning lazily at first use would let a plugin run a binary the user never saw, so a call that reaches an unpinned
program, finding it on `PATH` now, does **not** run it: it fails (`denied`, naming the program) and flags the plugin
`needs_approval` through `HostCalls::needs_approval`, the state an update asking for more already lands in. The user
approves by installing again, which pins what is there under the new digest. No new concept in either front-end, which
render the pin and the not-found program from the inspection.

**A missing program is `missing-requirement`**, carrying the name and link of the program's `requires`
(`{ name, url }`, `https`), or its command when it declares none. A flavor's description lists the program ids it
`requires`; when the host lists flavors it checks them now and reports each unmet one on `Flavor.requires`, which the
CLI refuses before a build and the desktop wizard already shows with its link. A managed Java that cannot be installed
is a plain failure, not a requirement: the host installs it itself, so the cause is the network or the disk.

**The hash is verified when approval is evaluated, not on every call.** A call checks that the pinned file still exists
and is executable, as before. Hashing a multi-megabyte binary on every `exec` is too slow, and refusing on a hash
mismatch would break every plugin on each OS package update of `git`. An update that finds the same path with different
content counts as a changed pin and needs approval, exactly like a moved path. The trade-off: a binary replaced in place
between approvals runs unchecked until the next install or update evaluates the pin. The window needs write access to a
system directory already on the user's `PATH`, which defeats the plugin sandbox's premise anyway; the pin guards against
a *different* binary being substituted by `PATH` order or a planted file, not against the system's own tools changing.

## Rejected

- **Pin at first use and record it silently.** Simplest, but it extends the approval without the user seeing it.
- **An explicit invocation id on `exec`/`spawn`.** Redundant with matching, and a guest could name one its arguments
  do not fit; the patterns would be re-checked anyway.
- **Hashing on every call.** Too slow, and refusing a mismatch breaks plugins on every package upgrade.
- **A `${java}` selector inside the arguments.** The Java major is not an argument, and a placeholder would need its own
  validation against the range.
- **One program per Java major**, which is what this removes.
