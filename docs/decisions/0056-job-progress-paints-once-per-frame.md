# Job progress paints once per frame, because the store re-renders synchronously

*Applies to: [Front-ends](../architecture/frontends.md)*

Pressing Play on an instance that had never been launched killed the desktop
UI: the first-launch modal opened, the materialize phases started reporting,
and React threw `Maximum update depth exceeded` — repeatedly, each throw
landing in `common::crash` as a `ui-error` report. The stack named the same
frame every time: `emit` in the job store, through `forceStoreRerender`.

Nothing in the tree was looping. The global job store is read through
`useSyncExternalStore`, so **every emit re-renders each subscriber
synchronously**, and `patch` emitted once per event. The daemon reports
provisioning progress *per file* — a libraries or assets download is hundreds
of events a second — so the render never caught up with the stream, and React
read a burst it could not settle between as a runaway update loop. Spacing the
same events 80 ms apart is clean; back-to-back hangs the page. It is a cadence
bug, not a cycle: the launch that emits the most events is the very first one,
which is why the failure looked like "first play is broken".

A gauge cannot show more than one frame's worth of progress, so a progress
patch now coalesces onto a `requestAnimationFrame` and every other change —
start, done, error, foreground/background, dismiss — emits at once, keeping
terminal states immediate. The launch provider's context value is memoized for
the same reason: it re-renders on each tick of the job it owns, and an
unmemoized `{launch, isLaunching}` pushed that tick through every consumer —
the entry cards, the play bar, the whole instance detail page with its charts.

The rejected alternative was throttling at the display site (`useRate`, the
progress view). That leaves every other subscriber re-rendering at the daemon's
cadence and has to be repeated at the next surface that renders a job; the
store is the one place that knows a burst is a burst.

## Amended: coalescing belongs at the publish seam first

Recorded as a front-end cadence bug, this read as done — and it was not. The
same burst reaches the CLI and the tray, and it crosses the socket either way.
The real fix is one layer earlier: the daemon coalesces each job's progress
before it publishes, forwarding only on a phase change, past a 0.5% ratio step,
or on the terminal tick.

That coalescer existed, but the spawn/publish skeleton around it was copied per
job family, so it had reached six of ten start sites — a backup, an export, a
download and the self-update still published per file or per chunk. Progress
coalescing is now the **runner's** obligation
(`daemon/runtime/managers/job.rs`), uniform across every job by
construction: a job declares its contract and its work, never its cadence. See [0061](0061-a-job-declares-what-differs.md).

The desktop's `requestAnimationFrame` store stays, as the second line it should
always have been: it protects the render from any burst, wherever it came from.
