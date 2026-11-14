# The plugin UI grows by general primitives, live pages and message catalogs

*Applies to: [Plugins](../architecture/plugins.md#declarative-ui-and-themes)*

A port of core's own players tab was not expressible: no picture of a person, no badge, no tabs, no way to ask before a
destructive click, no filter on a table, no way for a page to follow a server that starts or stops, and no way to read in
the user's language. The settings form could not say what range a number takes. The answer is the declarative tree
([0096](0096-plugin-ui-is-a-declarative-tree.md)) with more general nodes, not a second UI technology.

- **New nodes, all general.** `remote_image` (with an `avatar` shape), `badge`, `tabs`, a `confirm` text on a button,
  and a `filter` flag on a table. Each is something core's own pages use, none is specific to one plugin.
- **A remote image is the daemon's fetch.** The webview never contacts the host
  ([0098](0098-plugin-assets-are-served-by-the-daemon.md)'s rule, applied to images): the front-end asks
  `plugin.image { plugin, url }`, and the engine fetches through the image proxy under the plugin's `network:connect`
  grant. The first URL and every redirect hop go through the same `Policy` that guards downloads
  ([0119](0119-downloads-are-host-side.md)), on top of the proxy's own rules (https, no address literals, no credentials,
  10 MiB, must sniff as an image). A host the grant does not name gets no picture, and the page still draws. No new
  permission: reaching a host is what `network:connect` already means.
- **A live page is declared in the manifest.** A page contribution may name `topics` (topic globs, at most 8) and
  `refresh_every` (5 to 3600 seconds). While the page is open the front-end asks the plugin to draw it again when one of
  those topics is published, at most twice a second, or on the interval. The plugin is never sent the event, so a page
  gains no way to observe the daemon that its grant does not give it, and the call stays a render a person could have
  asked for (not budgeted, [0118](0118-host-pushed-events-and-a-call-budget-instead-of-a-resident-instance.md)).
- **Catalogs are package files, resolved by the host.** `messages/<locale>.json` is a flat object of texts. The host
  resolves a locale from the general to the specific (the `en` file, overlaid by the language's, overlaid by the exact
  locale's), so a key the locale lacks reads in the nearest language that has it. One resolver serves two readers: the app
  asks `plugin.messages` for the manifest's own texts (`page.<id>.title`, `setting.<key>.label`, `.help`,
  `.placeholder`, `group.<name>`, each falling back to the manifest's text), and the plugin asks `messages.catalog` for the
  texts of a page it draws, reading the locale from `PageContext.locale`. The client says its locale in the page context;
  the daemon drops one that is not a plain tag, which also keeps a locale from naming a path. A catalog that cannot be read
  is skipped with a warning, never an error: a broken translation must not break a page.
- **Settings gain constraints, checked by the host.** `min`, `max` (numbers), `pattern` (strings and secrets, matched
  against the whole value), `placeholder`, `group` and `help_url` (https). `Setting::check` is the one validation, used by
  the user's write and the plugin's own `settings.set`; manifest validation refuses a constraint on the wrong kind, an
  inverted range, a pattern that does not compile, and a help link that is not https. The form repeats min, max and
  pattern before the write so the message appears beside the field, but the daemon stays the authority.

Rejected:

- **A webview for rich pages.** It would make everything expressible, and make each plugin its own UI, with no shared
  look, no terminal rendering and no way to confirm or filter the way core does.
- **The webview fetching an image itself, with a CSP allow-list.** The CSP is app-wide and static; a plugin's hosts are
  per plugin and per grant. The daemon already fetches images for core.
- **A permission for remote images.** The grant already says which hosts the plugin may reach; a second list would drift
  from it.
- **Event payloads delivered to a live page.** A page that needs data draws it from its own calls; sending it the event
  would add a second way to observe the daemon.
- **Translating tree text by magic keys.** A guest-supplied string that is sometimes a key and sometimes text cannot be
  told apart. The plugin asks for a message by key and puts the answer in the tree.
- **`regex` in the SDK.** `pattern` is checked host-side only; the dependency sits behind a default feature that the SDK
  build leaves off, as the public-suffix list does.
