# Pages

A plugin never ships markup, scripts or styles into the app. A page is a tree of `UiNode` values the plugin returns; the
desktop draws it with its own components and the CLI prints it, so a page is written once. A page needs no permission.

## Declaring a page

```toml
[[contributes.pages]]
id = "overview"          # also `hestia <plugin> overview` on the CLI
title = "Overview"
slot = "entry"           # nav (default) | entry | settings
kind = "server"          # required for an entry page: server or instance
icon = "server"          # a name from the app's icon set, or a path inside the package
```

| Slot | Desktop | CLI |
|---|---|---|
| `nav` | a sidebar item | `hestia <plugin> <page>` |
| `entry` | a tab on every server or instance of `kind`; the page gets the entry id | `hestia <plugin> <page> --entry <name>` |
| `settings` | a section on the plugin's own page under Plugins | `hestia <plugin> <page>` |

An `icon` containing `/` or `.` is a package path; anything else is a name: `instance`, `server`, `mod`,
`resource_pack`, `shader`, `data_pack`, `modpack` or `plugin` (an unknown name draws the plugin glyph). A page id that
is also a command id is a command on the CLI.

A page can ask to be drawn again while open, with `topics` (at most 8 topic globs) and `refreshEvery` (5 to 3600
seconds). The front-end calls `render` again when a topic is published (throttled) or the interval passes. The plugin
is never sent the event, so `topics` needs no permission.

## Rendering and actions

`Plugin::page(call: Pages)` routes by page id: `call.on(page, render, action)`, or `call.view(page, render)` for a page
that only displays, then `finish()`. A page no handler takes answers `unsupported`.

```rust
call.on(
    "overview",
    |context: PageContext| -> Result<UiNode, Error> { /* ... */ },
    |action: Act, values: Values, context: PageContext| -> Result<Vec<Effect>, Error> { /* ... */ },
)
```

- `context.entry` is the server or instance id on an `entry` page, else `None`; `context.locale` is the app's language
  ([api.md](api.md#messages)).
- `render` runs each time the page is shown or refreshed, with a 10 second limit. The tree is checked against the
  vocabulary below; anything outside it fails the render.
- `action` runs when the user clicks a button or submits a form. `values` holds the submitted fields, empty for a
  button: `values.get::<T>("name")?` reads one, `values.parse::<Form>()?` decodes all into your struct. A payload that
  does not decode is `Error::Invalid`.

### Actions are the plugin's own enum

Tag the enum so it has the wire form of an `Action` (`id`, and `payload` for its data). The same enum goes out on the
node and comes back typed; `ui::button`, `ui::form` and `ui::action` take any `Serialize` value:

```rust
#[derive(Serialize, Deserialize)]
#[serde(tag = "id", content = "payload", rename_all = "kebab-case")]
enum Act {
    Backup,
    Remove(String),
}

ui::button("Back up now", Act::Backup)
```

A table's `rowAction` is the variant's `id`, and the row's `payload` is its data. An enum not tagged this way produces a
node whose click answers `Error::Invalid`.

### Effects

`action` answers a list of effects, applied in order:

| Effect | JSON | Does |
|---|---|---|
| `render` | `{ "type": "render", "tree": { ... } }` | replaces the page |
| `toast` | `{ "type": "toast", "tone": "success", "text": "Saved" }` | shows a short message |
| `navigate` | `{ "type": "navigate", "page": "overview" }` | opens another page of the plugin |
| `refresh` | `{ "type": "refresh" }` | renders the current page again |
| `run` | `{ "type": "run", "command": "setup", "args": { "x": 1 } }` | starts one of the plugin's commands as a job |

`run` is for work longer than an action's 10 seconds: the action answers at once and the command runs as a job with
progress, outliving the page and reporting its outcome once, so the plugin adds no toast of its own. `args` are checked
against the command's declaration. A `server` or `instance` command runs on the entry of the page being shown, and is
refused from a page that is not an entry page of that kind; a `top` command runs from any page.

## Nodes

A node's `type` is `snake_case` and its other fields `camelCase`. `direction`, `tone` and `variant` default to
`vertical`, `neutral` and `primary`. Tones: `neutral`, `muted`, `info`, `success`, `warning`, `error`. The samples are
the pinned encodings in `crates/proto/tests/golden/ui_node.json`.

| Node | Fields | Notes |
|---|---|---|
| `stack` | `direction` (`vertical`, `horizontal`), `children` | nest for layout |
| `section` | `title`, `description?`, `actions?` (buttons), `children` | |
| `heading` | `text`, `level` (1 to 3) | |
| `text` | `text`, `tone?` | |
| `markdown` | `source` | no raw HTML: markup shows as text |
| `image` | `src`, `alt` | `src` is a path in the package, e.g. `assets/logo.png`; the daemon serves it |
| `remote_image` | `url`, `alt`, `shape` (`natural`, `avatar`) | https, at most 2048 bytes; the host and every redirect hop must be granted by `network:connect`. The daemon fetches it |
| `copy` | `text`, `label?` | a value with a copy button; composes anywhere, including a list item's `actions` |
| `badge` | `text`, `tone` | |
| `tabs` | `tabs`: 1 to 12 of `{ id, label, children }` | |
| `table` | `columns`, `rows: [{ cells, payload? }]`, `filter?`, `rowAction?`, `empty?` | cells are strings; `filter: true` adds a search box |
| `list` | `items`, `empty?` | an item has `title`, `subtitle?`, `icon?`, `image?` (`{ url, shape }`, https as `remote_image`), `badges` (`{ text, tone }`) and `actions` |
| `detail` | `pairs: [{ label, value }]` | |
| `form` | `id`, `fields`, `submit: { label, action }` | see below |
| `button` | `label`, `action`, `variant` (`primary`, `secondary`, `danger`), `confirm?` | `confirm` asks the user first; use it for what cannot be undone |
| `notice` | `tone`, `text` | |
| `progress` | `fraction?` (0 to 1), `label?` | no fraction is an indeterminate bar |
| `webview` | `src`, `height` | [below](#webview) |

```json
{
  "type": "section",
  "title": "Tunnels",
  "actions": [ { "type": "button", "label": "Add", "action": { "id": "add", "payload": null }, "variant": "secondary" } ],
  "children": [ { "type": "copy", "text": "abc.example.com:1234", "label": "Address" } ]
}
```

A form's fields are keyed by `name` and arrive in `action`'s `values`. Every field has `name`, `label` and an optional
`help`:

| Field `type` | Other fields | Submits |
|---|---|---|
| `text` | `value`, `placeholder` | string |
| `number` | `value`, `min`, `max`, `step` | number |
| `toggle` | `value` | boolean |
| `select` | `options: { value, label }[]`, `value` | string |
| `entry` | `kind`: `server` or `instance` | the id of an existing entry |

`min`, `max` and `step` are front-end hints; the daemon does not enforce them, so validate what you receive.

```json
{
  "type": "form",
  "id": "settings",
  "fields": [
    { "type": "number", "name": "keep", "label": "Keep", "value": 7.0, "min": 1.0, "max": 30.0, "step": 1.0 },
    { "type": "select", "name": "mode", "label": "Mode", "options": [ { "value": "fast", "label": "Fast" } ], "value": "fast" }
  ],
  "submit": { "label": "Save", "action": { "id": "save", "payload": { "x": 1 } } }
}
```

## Building trees in Rust

`hestia::ui` has a builder for every node and re-exports the types, so anything a builder lacks is the enum written
out. Free functions: `stack`, `row_of`, `heading`, `text`, `markdown`, `notice`, `image`, `remote_image`, `avatar`,
`badge`, `copy`, `copy_labelled`, `tabs` with `tab`, `detail`, `progress`, `webview`, `table` with `row`,
`filtered_table`, `button`, `button_confirming` and `form`. Nodes with options have builders: `Section::new`
(`description`, `action`, `children`), `Item::new` and `List::new`, `Table::new` (`filter`, `row_action`, `empty`),
`Button::new` (`secondary`, `danger`, `confirm`) and `Field::text`, `number`, `toggle`, `select`, `entry` (`help`, and
`placeholder`, `min`, `max`, `step` where they apply). A progress `label` and a text `tone` need `UiNode::Progress { .. }`
or `UiNode::Text { .. }` written out, and `Effect::Run { command, args }` is built directly.

From the doctest in `crates/sdk/src/pages.rs`, a page whose button and form answer with effects:

```rust
impl Plugin for Backups {
    fn page(call: Pages) -> Paged {
        call.on(
            "overview",
            |context| {
                Ok(ui::stack([
                    ui::text(context.entry.unwrap_or_default()),
                    ui::button("Back up now", Act::Backup),
                ]))
            },
            |act: Act, values: Values, _context| match act {
                Act::Backup => Ok(vec![Effect::Refresh]),
                Act::Remove(world) => {
                    let rename: Rename = values.parse()?;
                    Ok(vec![Effect::Toast {
                        tone: Tone::Success,
                        text: format!("{world} -> {}", rename.name),
                    }])
                }
            },
        )
        .finish()
    }
}
```

## In the CLI

On a terminal, `hestia <plugin> <page>` draws the page and offers its buttons, list-item buttons, table rows (with a
`rowAction`) and form submits as one menu, applying each action's effects; a `confirm` asks first, a form prompts field
by field and a `run` effect runs the command and redraws. Without a terminal it draws once. `--action <id>` runs one
action, `--set name=value` fills a form field and `--entry <name>` names the entry of an `entry` page
([cli.md](../cli.md)). The CLI prints text for what it cannot draw: `alt` for images, `[text]` for badges,
a note for a webview. The desktop shows a node type it does not know as a notice.

## Webview

When the nodes cannot draw what you need, such as a chart or a map, return a `webview` node: an HTML file from your
`assets/` in a sandboxed frame on the desktop (the CLI prints a note).

```json
{ "type": "webview", "src": "assets/chart.html", "height": 320 }
```

In Rust, `ui::webview("assets/chart.html", 320)`. `src` is an `.html` file under `assets/`, `height` 1 to 2000 pixels.

The page can run its own scripts and styles from your `assets/` (`.js`, `.css`, images, fonts, `.json`, by relative
path). It has no network, no other frames, no forms, no shared cookies or storage and no access to the app's DOM, so
bundle everything it needs. It talks to the host with `postMessage`, only for your plugin:

| Request | Does | Reply `result` |
|---|---|---|
| `{ id, type: "run", command, args }` | runs one of your `top` commands | the command's output |
| `{ id, type: "action", action, payload }` | sends a UI action to the page being shown, as a button does | `null` |

Replies are `{ id, ok: true, result }` or `{ id, ok: false, error }`. A message that is not exactly one of these, or is
over 64 KiB, gets no reply. A helper to copy:

```js
const pending = new Map();

window.addEventListener("message", (event) => {
  if (event.source !== window.parent) return;
  const reply = event.data;
  const waiting = pending.get(reply?.id);
  if (!waiting) return;
  pending.delete(reply.id);
  clearTimeout(waiting.timer);
  reply.ok ? waiting.resolve(reply.result) : waiting.reject(new Error(reply.error));
});

function hestia(type, body, timeoutMs = 30000) {
  const id = crypto.randomUUID();
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      pending.delete(id);
      reject(new Error("the host did not answer"));
    }, timeoutMs);
    pending.set(id, { resolve, reject, timer });
    window.parent.postMessage({ id, type, ...body }, "*");
  });
}

const runCommand = (command, args = {}) => hestia("run", { command, args });
const sendAction = (action, payload = null) => hestia("action", { action, payload });
```

`"*"` is required because the host's origin is not addressable from an opaque-origin frame; the host only accepts
messages from the frame it created, and the helper only accepts replies from `window.parent`.
