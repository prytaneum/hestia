# Events wait on their own budget; wake-ups skip

*Applies to: [The plugin kernel](../architecture/plugins.md#topics-and-hooks)*

**Decision.** A plugin's events queue in a per-plugin inbox of 256 and are delivered one at a time, each when the
plugin's event budget (30 in a burst, one more each second) has a token; only a full inbox drops, oldest first. The
budget is separate from the wake-up budget that `jobs.start` and schedule ticks spend, and those are skipped, not queued,
when it is empty. A hook is not queued: it runs inline, in id order, for 2 s each.

**Why.** An event is a fact the plugin should hear, so a burst is paced rather than lost. A flood of events must not
starve a plugin's schedules or `jobs.start`, and a plugin must not be able to loop itself awake.

**Rejected.** Dropping an event when the budget is empty: a plugin would miss events in a burst. One shared budget:
events could exhaust the schedules'. An unbounded inbox: a slow plugin would hold memory without limit.
