# A job is its start contract, and every job reports on one topic

*Applies to: [The socket boundary](../architecture/wire.md#jobs-events-and-cancellation)*

A long-running operation answered its start channel with `{ id }` and then
published `<family>.progress|done|error|cancelled` — ten families, about forty
`Topic` structs. Every cancelled event was `{ id }`, every error `{ id, error }`,
every progress `{ id, …ProvisionProgress }`; only the done payload varied. Each
daemon manager wrapped the id into four per-family structs, and the client's
`run_job` took topic strings, derived the cancelled one with a string replace
and handed back untyped JSON that each facade decoded by hand. The comment that
claimed a test pinned the naming was wrong.

**The start contract is the job.** `proto::job::Job` extends `Contract` with
`Result = JobStarted` and declares two types, `Progress` and `Done`. Nothing
else about a job varies on the wire, so nothing else is declared.

**One topic, `job`.** Every job publishes `JobEvent<P, D>`: the job `id`, the
start contract's `channel`, and a `state` — `progress`, `done`, `failed` or
`cancelled` — with the state's payload flattened beside them. The daemon's
`Runner::start::<C>` builds every event; the client's `Session::run_job::<C>`
decodes them into `C::Progress` and `C::Done`. There is no topic string
anywhere to mistype, and a cancel has its own state rather than a derived name
(this supersedes the topic-derivation noted in
[0032](0032-jobs-are-cancelled-by-asking.md)).

`channel` is what keeps the single topic useful to a client that did not start
the job: another window refreshing its lists matches on the channel
(`server.create`, `content.add`, …) exactly as it used to match on the topic.
When one manager serves several contracts it is generic over the contract, so
the event names the channel that actually started the job.

What a cancel leaves behind is domain knowledge, so it is documented on the
start contract it belongs to.

Rejected:

- **A `Family` grouping** (a marker type per topic prefix, with the contracts of
  a prefix pointing at it). It existed only because topics were named per group;
  once the topic is one, the grouping is an extra type with nothing to carry.
  Contracts that share payloads just declare the same two types.
- **The start call's response being the result, with progress as notifications.**
  A job outlives the connection that asked for it
  ([0034](0034-workloads-outlive-the-daemon.md)), and other clients still need a
  broadcast to know a list is stale, so it would be a second mechanism beside
  the events rather than a replacement for them.
