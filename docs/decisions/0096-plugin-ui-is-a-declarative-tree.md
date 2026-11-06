# Plugin UI is a declarative tree the host renders

*Applies to: [The socket boundary](../architecture/wire.md), [Front-ends](../architecture/frontends.md)*

A plugin page is a value, not code: `render` returns a `proto::ui::UiNode` tree
and `action` returns a list of `Effect`s (render a new tree, toast, navigate,
refresh). The host draws the tree with its own components, so a plugin cannot
ship markup, scripts or styles into the app (R-05, R-06).

**The vocabulary is closed.** Twelve nodes: `stack`, `heading`, `text`,
`markdown`, `image`, `table`, `list`, `detail`, `form`, `button`, `notice`,
`progress`; five form fields: `text`, `number`, `toggle`, `select`, `entry`. A
node is added only when a real plugin needs it, and the set is versioned with the
plugin API, so a front-end that renders every variant renders every plugin. An
unknown `type` is a decode error at the daemon, never a half-drawn page.

**One tree, both front-ends.** The CLI is a first-class front-end, and a webview
page would leave every plugin invisible to `hestia`. A tree has a terminal
rendering for each node (a table, a key/value block, a prompt for a form), so a
plugin is written once and parity is a property of the vocabulary rather than a
promise each plugin makes. Everything the desktop needs beyond that (icons, the
entry picker) degrades to text in the terminal.

**Safe by construction.** `markdown` is rendered without raw HTML; embedded markup
shows as text. `image.src` is a path inside the plugin's own assets, resolved by
the host, never a URL. Actions carry only an id and a JSON payload back to the
plugin, which owns its own state.

Rejected: a sandboxed iframe per page as the first mechanism. It gives full
freedom, but no CLI rendering, no shared look, and a second security surface
(CSP, message bridge, asset serving) to get right before any plugin needs it. It
remains the later escape hatch for pages the vocabulary cannot express (phase 9);
declarative stays the default. Also rejected: HTML strings from the plugin run
through a sanitiser, which keeps the sanitiser as the whole security story and
still cannot be drawn in a terminal.
