# The page vocabulary grows by need, and a page can run a command

*Applies to: [Plugins](../architecture/plugins.md#declarative-ui-and-themes)*

[0096](0096-plugin-ui-is-a-declarative-tree.md) made a page a tree the host draws. Reworking the playit, players and
tailscale pages showed what that tree could not say: grouping, a status on a list item, a value to copy, an empty state,
hints and bounds on a field, and long work (an action times out at 10 s).

**The vocabulary grows only by what more than one plugin needs.** It is public API for every plugin, so a node shaped for
one is dead weight for the rest. Each addition names two consumers: `section` and `copy` (addresses, fingerprints),
`ListItem.badges` and `ListItem.image` (players, tunnels, backups), `empty` on list and table, field `help`,
`placeholder` and number `min`/`max`/`step`. A node for one plugin's widget (a tunnel row, a claim box) is refused; the
webview stays the escape hatch ([0109](0109-plugin-webviews-are-opaque-iframes.md)).

**`FormField` is common parts with the input flattened in.** `{ name, label, help, input }` with
`#[serde(flatten)]` on `input`, so the JSON stays `{ type, name, label, … }`. `help` applies to every field, and a
field of five kinds does not repeat it five times. The bounds are hints at the edge: an empty number blocks the submit
instead of sending 0, and the plugin still validates, since its input is untrusted.

**`Effect::Run { command, args }` starts one of the plugin's own commands as a `plugin.run` job.** The job store gives
it progress, outlives the page and reports the outcome once ([0089](0089-a-job-outcome-is-reported-once.md)); the action
stays fast. The entry comes from the page being shown, never from the effect, so a plugin cannot aim a run at an entry
the user did not open. The daemon checks the effect when the action answers (`check_run_effects`): the command must be
in the manifest, and a `server` or `instance` command needs an `entry` page of that kind. It is not trusted from the
front-end alone.

**Optional parts are SDK builders** (`Section`, `Item`, `List`, `Table`, `Button`, `Field`) that convert into a node;
`button`, `form` and `table` stay as one-liners over them.

The plugin API is unreleased, so the old `FormField` shape is not kept and `API_VERSION` is not bumped.

**Rejected.** TypeScript components loaded into the app (breaks the sandbox) or webviews as the main path (no CLI, no
shared look). A generic card with free slots, over nesting headings in stacks. A copy flag on `detail` pairs: a second
way to say the same thing, and pairs cannot sit in a list item. A new row node, over a list item. `help` on each field
variant. Daemon-side validation of declared bounds. A longer action timeout, or a button kind that names a command
directly (two click paths). Trusting the effect's own fields, or checking only in the front-end. Methods on the proto
types, over builders.
