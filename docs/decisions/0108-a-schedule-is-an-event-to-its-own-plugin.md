# A schedule is an event delivered to its own plugin

*Applies to: [Plugins](../architecture/plugins.md)*

A plugin ran only when called: an event, a hook, a command, a page. Nothing called it on a clock, so it could not poll
an upstream, retry a connection that dropped (Discord starting after Hestia) or run an automation at a time.

**The manifest declares timers; the host delivers a tick as an event.** `[[contributes.schedules]]` entries carry an
`id` and `every`, in whole seconds. Each tick is a `plugin.schedule` event (`{ id }`) put in that plugin's own inbox and
nowhere else: it is never published to the hub, so no other plugin and no IPC client hears it, and `plugin.*` globs
cannot reach it. In the SDK it arrives through `Plugin::on_event`, with `schedule::tick(topic, &payload)` to read the id.
This is [0106](0106-the-plugin-api-is-mechanisms-not-features.md)'s shape: no new WIT, no new `Plugin` method, the
varying part (the schedules) is data in `proto`.

- **Floor and cap.** `every` is 30 s to 30 days, at most 8 schedules per plugin, ids unique slugs; the manifest refuses
  anything else. The schedules are part of the grant, like `events`, so changing one asks for approval again.
- **No permission.** A tick grants nothing the plugin does not already have: it only runs the plugin's own exports,
  under the same limits as an event, and every host call it makes is gated as usual. The floor and the cap bound the
  cost, and the approval prompt lists the schedules.
- **No catch-up.** The timer skips what it missed (`MissedTickBehavior::Skip`), so waking from sleep yields one tick,
  not a burst. The first tick is one full period after the plugin loads.
- **No pile-up.** A tick is dropped while a tick of the same schedule still waits in the inbox, so a plugin slower than
  its schedule holds at most one pending tick per schedule; the inbox's own bound still applies on top.
- **Lifecycle.** The timers belong to the plugin's event `Delivery`: they start when the plugin loads enabled and stop
  when it is disabled, removed, reloaded, or stops receiving events after repeated timeouts or traps.

**Rejected: a `timer` import** that lets a plugin set and clear timers at run time. Every call runs on a fresh
instance, so a plugin would have to re-arm its timers each call, and the host would need per-plugin timer state,
limits and a new ABI surface for what is a static fact about the plugin. A declared schedule is reviewable at install,
needs no ABI change and cannot be left running by a bug.

**Rejected: broadcasting the tick** as an ordinary topic gated by a permission. It would reach other plugins and
clients for no reason and need a permission that protects nothing.
