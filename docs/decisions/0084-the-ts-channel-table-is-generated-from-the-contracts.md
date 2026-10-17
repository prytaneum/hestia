# The TypeScript channel table is generated from the contracts, and a contract owns its timeout

*Applies to: [The socket boundary](../architecture/wire.md#the-typescript-projection), [Front-ends](../architecture/frontends.md#the-frontend)*

Rust has one definition per channel: an `impl Contract` naming the channel, its
params and its result. ts-rs exported the payload *structs*, but not the
contract, so the desktop's `api/` layer restated it by hand at every call —
the channel as a string literal, the params as an unchecked object, the result
as an inline assertion (`call<{ flavors: Flavor[] }>('server.flavors')`).
`call<T>(channel: string, params: unknown)` checked none of it. About 160 sites
repeated the channel; a job's `TDone` was asserted by hand beside a
`start` closure; `queries/invalidation.ts` keyed its table by typed-in channel
strings; the browser mock was a third copy keyed by the same strings. Long-call
timeouts were mirrored by hand too — `Duration::from_secs` in a Rust facade and
`timeoutMs` in the TS function — and `contributing.md` told contributors to keep
them in step. They had not stayed in step.

**The TS table is a projection of the contract, generated from `proto`'s source.**
`scripts/lib/gen-channels.py` reads every `impl Contract`, `impl Job` and
`impl Topic` in `crates/proto/src` and writes
`frontend/src/api/types/channels.ts`: `Channels` (channel to params and result),
`Jobs` (job channel to progress and done), `Topics` (topic to event payload, the
`job` topic being the union of `JobEvent`s narrowed per channel) and `TIMEOUTS`.
`call`, `tryCall`, `runJob`, `onDaemonEvent`, the mock's handler map and the
invalidation tables are typed against it, so the API function names a channel and
the rest is inferred, and a renamed channel, a wrong param or a fixture that no
longer matches the wire fails `tsc`. The generator exits non-zero on a block it
cannot parse or a type ts-rs did not export, and on a count that disagrees with
the source, so a contract cannot silently drop out of the table.

**A request struct with a struct-level `#[serde(default)]` is emitted
`Partial<…>`, and so is any such struct nested in it.** The daemon fills what a
request omits, which ts-rs cannot express; without this every call would have to
spell out every default. Params that are `Empty`, or all-default, may be left
out of a call.

**`Contract::TIMEOUT` is the one place a timeout is written.** It defaults to
`DEFAULT_TIMEOUT` (10 s); a contract whose handler waits on the network, a disk
tree or a person overrides it. `Session::call` applies it (`call_with_timeout`
is gone), the generator copies the overrides into `TIMEOUTS`, and the TS `call`
passes the contract's value to the bridge, so no facade method or API function
carries one. Where Rust and TS had disagreed, the larger value was taken. A
job's start channel answers at once and keeps the default.

Rejected:

- **Mirroring each channel as a Tauri command.** Already rejected in
  [0046](0046-desktop-bridge-is-one-generic-command.md): a third naming seam
  with `invoke()` results untyped regardless. The generic bridge stays; only the
  TS side of it gains types.
- **Hand-maintained TS types over the generic call** (the status quo). It is the
  drift the single definition exists to prevent, one assertion per call site,
  and it cannot check a job's done type, an event payload or a mock fixture.
- **Deriving the table with a proc-macro or a `Contract` registry in Rust.**
  The contracts are marker types with associated items; collecting them needs
  either a registry every contract must remember to join or a macro on each
  `impl`. Reading the source with a script that counts what it found keeps the
  contracts as plain Rust and fails loudly instead.
- **Deleting the API functions that only forward to `call`, and calling the
  channel from the query factory.** Once the table types every call, such a
  function is one line, and removing it only moves that line into `queries/`.
  The `api/` module stays the desktop's mirror of the Rust client facades
  (which keep theirs for the same reason), and the place a non-query caller
  reaches the daemon.
- **A deep `Partial` over every nested object.** It would also loosen required
  nested values with no default; only structs that carry `#[serde(default)]`
  themselves are loosened.
