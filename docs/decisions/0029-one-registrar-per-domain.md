# No Service-class-per-prefix — but one registrar function per domain

*Applies to: [The daemon](../architecture/daemon.md)*

Unlike the historical C++ tree (which had one `Service` *object* per
channel-prefix, with its own lifetime and state), a handler here is a closure
and the registry is a flat map from channel to closure. What a domain gets is
only a `register(&mut Channels)` function: a compile-time grouping, no runtime
entity. The grouping exists because the flat `make_router()` grew to ~75
channels in one 1100-line function, which is the aggregation-point smell, not a
design: wiring in a channel is still exactly one `handle::<C>` line, now in the
file that owns its domain.

## Amended: a channel declares its intent, it does not re-derive its guards

The rejection still holds — a `Service` object per prefix is a runtime entity
nobody needed. But "a handler is a closure" put every precondition *inside* that
closure, as a checklist each handler assembled by hand, and the checklists
drifted: an instance content update could race an export while an add could not,
and a server content remove skipped the update check its sibling made. Worse,
one guard never fired at all — `ensure_stopped` asked the supervisor for
`instance-<id>`, but an instance runs its sessions under `instance-<id>_<seq>`,
so content was free to write jars a running JVM held open.

A handler now names what it is about to do — `Intent::{Read, Idle}`, or a
`Hold` that takes the entry's claim ([0086](0086-an-entry-has-one-claim.md)) —
and `guards::{server_for, instance_for, server_held, instance_held}` resolves
the entry and refuses whatever holds it, per side. The decision's
own goal is untouched: wiring in a channel is still one `handle::<C>` line in
the file that owns its domain. What changed is that the line now carries what
the channel needs instead of trusting the author to remember it.

A verb that exists on both sides is one channel aimed by an `EntryTarget`, and
`guards::entry_for` is the third resolver: it dispatches on the target's kind to
`server_for` or `instance_for` (`entry_held` to the held variants) and returns the owned handle a job or an engine flow takes. The
two halves that used to be line-for-line copies of each other are one function;
the `entry` registrar branches on the kind only where the engine call differs. The account gate for the
instance side lives there too
([0030](0030-instance-surface-gated-on-an-account.md)).
