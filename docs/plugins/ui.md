# Pages

A plugin never ships markup, scripts or styles into the app. A page is a tree of `UiNode` values that the plugin
returns; the desktop draws it with its own components and the CLI prints it, so a page is written once
([0096](../decisions/0096-plugin-ui-is-a-declarative-tree.md)). A page needs no permission. The design is in
[architecture/plugins.md](../architecture/plugins.md#declarative-ui-and-themes).

## Declaring a page

```toml
[[contributes.pages]]
id = "overview"          # the page id; `hestia <plugin> overview` on the CLI
title = "Overview"
slot = "entry"           # nav (default) | entry | settings
kind = "server"          # required for an entry page: server or instance
icon = "server"          # optional: a name from the app's icon set, or a path inside the package
```

| Slot | Desktop | CLI |
|---|---|---|
| `nav` | a sidebar item that opens the page | `hestia <plugin> <page>` |
| `entry` | a tab on the detail of every server or instance of `kind`; the page gets the entry id in its context | `hestia <plugin> <page> --entry <name>` |
| `settings` | a section at the end of Settings, Plugins | `hestia <plugin> <page>` |

An `icon` containing `/` or `.` is a path inside the package and must stay inside it; anything else is an icon name:
`instance`, `server`, `mod`, `resource_pack`, `shader`, `data_pack`, `modpack` or `plugin` (`NAMED_ICONS` in
`frontend/src/components/icons.ts`). An unknown name draws the plugin glyph.
A manifest with an `entry` page and no `kind` is refused. A page id that is also one of the plugin's command ids is a
command on the CLI.

## Rendering and actions

Two methods of `Plugin` carry a page (`crates/sdk/src/lib.rs`). Both default to a `not-found` error, so a plugin with
no pages implements neither.

```rust
fn render(page: &str, context: PageContext) -> Result<UiNode, Error>;
fn action(page: &str, action: Action, values: Value, context: PageContext) -> Result<Vec<Effect>, Error>;
```

- `context.entry` holds the server or instance id on an `entry` page, and is `None` elsewhere.
- `render` is called each time the page is shown or refreshed. The returned tree is checked against the vocabulary
  below; anything outside it fails the render. The channel's timeout is 10 s. Every call runs on
  a fresh instance, so keep state in the plugin's data directory, not in memory.
- `action` runs when the user clicks a button or submits a form. `action.id` and `action.payload` are what the node
  declared; `values` is the submitted form's fields by name, and an empty object for a button.

### Effects

`action` answers a list of effects, applied in order:

| Effect | JSON | Does |
|---|---|---|
| `render` | `{ "type": "render", "tree": { ... } }` | replaces the page with a new tree |
| `toast` | `{ "type": "toast", "tone": "success", "text": "Saved" }` | shows a short message |
| `navigate` | `{ "type": "navigate", "page": "overview" }` | opens another page of the same plugin |
| `refresh` | `{ "type": "refresh" }` | renders the current page again |

Source: `crates/proto/tests/golden/ui_effects.json`, a pinned encoding.

## Nodes

Twelve node types make the vocabulary. Each JSON sample below is the wire form, copied from the pinned fixture
`crates/proto/tests/golden/ui_node.json`. A node's `type` is `snake_case`, its other fields `camelCase`. A node that
sets a field to its default may omit it: `direction`, `tone` and `variant` default to `vertical`, `neutral` and
`primary`. Tones are `neutral`, `muted`, `info`, `success`, `warning`, `error`.

### stack

Lays children out in a column (`vertical`, the default) or a row (`horizontal`). Nest stacks for layout.

```json
{ "type": "stack", "direction": "vertical", "children": [ { "type": "heading", "text": "Backups", "level": 1 } ] }
```

### heading

`level` is 1 to 3.

```json
{ "type": "heading", "text": "Backups", "level": 1 }
```

### text

```json
{ "type": "text", "text": "Last run 2h ago", "tone": "muted" }
```

### markdown

Rendered without raw HTML: embedded markup shows as text. The CLI prints it as formatted terminal text.

```json
{ "type": "markdown", "source": "**Hello** [docs](https://example.com)" }
```

### image

`src` is a path inside the plugin's package, such as `assets/logo.png`; the daemon serves it
([0098](../decisions/0098-plugin-assets-are-served-by-the-daemon.md)). The CLI prints `alt`.

```json
{ "type": "image", "src": "assets/logo.png", "alt": "Logo" }
```

### table

Cells are strings. When `rowAction` is set, choosing a row sends that action id with the row's `payload`.

```json
{
  "type": "table",
  "columns": ["Name", "Size"],
  "rows": [ { "cells": ["world", "12 MB"], "payload": { "id": "w1" } } ],
  "rowAction": "open"
}
```

### list

Items with a title, an optional subtitle and icon (a name or package path, as for a [page](#declaring-a-page)), and
buttons. The CLI prints a two-column table of title and subtitle.

```json
{
  "type": "list",
  "items": [
    {
      "title": "Nightly", "subtitle": "03:00", "icon": "clock",
      "actions": [ { "type": "button", "label": "Run", "action": { "id": "run", "payload": null }, "variant": "secondary" } ]
    }
  ]
}
```

### detail

Label and value pairs.

```json
{ "type": "detail", "pairs": [ { "label": "Retention", "value": "7 days" } ] }
```

### form

Fields are keyed by `name`; their values arrive in `action`'s `values`, and the submit button's `action` says what was
asked. Field types:

| Field `type` | Fields besides `name` and `label` | Submits |
|---|---|---|
| `text` | `value`: string | string |
| `number` | `value`: number | number |
| `toggle` | `value`: boolean | boolean |
| `select` | `options`: `{ value, label }[]`, `value`: the selected `value` | string |
| `entry` | `kind`: `server` or `instance` | the id of an existing server or instance |

```json
{
  "type": "form",
  "id": "settings",
  "fields": [
    { "type": "text", "name": "title", "label": "Title", "value": "a" },
    { "type": "number", "name": "keep", "label": "Keep", "value": 7.0 },
    { "type": "toggle", "name": "on", "label": "Enabled", "value": true },
    { "type": "select", "name": "mode", "label": "Mode", "options": [ { "value": "fast", "label": "Fast" } ], "value": "fast" },
    { "type": "entry", "name": "target", "label": "Server", "kind": "server" }
  ],
  "submit": { "label": "Save", "action": { "id": "save", "payload": { "x": 1 } } }
}
```

### button

`variant` is `primary`, `secondary` or `danger`.

```json
{ "type": "button", "label": "Run", "action": { "id": "run", "payload": null }, "variant": "secondary" }
```

### notice

An inline message with a tone.

```json
{ "type": "notice", "tone": "warning", "text": "Careful" }
```

### progress

`fraction` is 0 to 1; leave it out when the length of the work is unknown (an indeterminate bar). `label` is optional.

```json
{ "type": "progress", "fraction": 0.5, "label": "Halfway" }
```

### webview

An HTML page from your `assets/` in a sandboxed frame, for what the nodes above cannot draw. See [webview.md](webview.md).

```json
{ "type": "webview", "src": "assets/chart.html", "height": 320 }
```

## Building trees in Rust

`hestia::ui` has builders for the common nodes and re-exports the types, so anything a builder does not cover is the
enum written out. Builders exist for `stack`, `row_of` (a horizontal stack), `heading`, `text`, `markdown`, `notice`,
`image`, `table` with `row`, `detail`, `button`, `form`, `progress` and `webview`. `list`, a table's `row_action`, a progress
`label`, a button `variant` and a text `tone` have none; build `UiNode::List { .. }` and the others directly.

From the SDK's doctest, `crates/sdk/src/ui.rs`:

```rust
use hestia::ui;

let page = ui::stack([
    ui::heading("Backups", 1),
    ui::table(["World", "Size"], [ui::row(["smp", "1.2 GB"])]),
    ui::button("Back up now", "backup"),
]);
```

From the test fixture `crates/engine/tests/fixture/plugins/probe/src/lib.rs`, an `entry` page whose button answers with
a toast and a refresh (its manifest is generated by the test; it is not shown):

```rust
fn render(page: &str, context: PageContext) -> Result<UiNode, Error> {
    match page {
        "overview" => Ok(ui::stack([
            ui::heading("Overview", 1),
            ui::text(context.entry.unwrap_or_else(|| "no entry".into())),
            ui::button("Go", "go"),
        ])),
        _ => Err(Error::NotFound(format!("unknown page `{page}`"))),
    }
}

fn action(
    page: &str,
    action: Action,
    values: Value,
    _context: PageContext,
) -> Result<Vec<Effect>, Error> {
    match page {
        "overview" => Ok(vec![
            Effect::Toast {
                tone: Tone::Success,
                text: format!("{} {}", action.id, values),
            },
            Effect::Refresh,
        ]),
        _ => Err(Error::NotFound(format!("unknown page `{page}`"))),
    }
}
```

The [`dashboard`](https://github.com/prytaneum/plugin-examples/tree/main/plugins/dashboard) example has a `nav` page that lists every entry and an `entry` tab on each
server with a form. Its form and save action, from `plugins/dashboard/src/lib.rs`:

```rust
fn notes(entry: &str) -> Result<UiNode, Error> {
    let note = hestia::data::load::<String>(entry)?.unwrap_or_default();
    Ok(ui::stack([
        ui::heading("Notes", 1),
        ui::form(
            "note",
            [FormField::Text {
                name: "note".into(),
                label: "Note".into(),
                value: note,
            }],
            "Save",
            "save",
        ),
    ]))
}
```

```rust
("notes", "save") => {
    let entry = context
        .entry
        .ok_or_else(|| Error::Invalid("no entry".into()))?;
    hestia::data::save(&entry, &values["note"])?;
    Ok(vec![
        Effect::Toast {
            tone: Tone::Success,
            text: "Note saved".into(),
        },
        Effect::Refresh,
    ])
}
```

## How each front-end draws it

| Node | Desktop | CLI |
|---|---|---|
| `stack`, `heading`, `text` | layout and text components | lines; `muted` prints as a note, `warning` and `error` as warnings |
| `markdown`, `image` | the shared markdown component without raw HTML; package images by URL | formatted text; the `alt` text |
| `table`, `list`, `detail` | data components | tables and a key/value block |
| `form`, `button` | inputs and buttons | an action menu entry; a form prompts field by field |
| `notice` | an inline notice | a note; `warning` and `error` as warnings |
| `progress` | a bar | a text bar |
| `webview` | a sandboxed frame ([0109](../decisions/0109-plugin-webviews-are-opaque-iframes.md)) | a note |

The desktop renders a node type it does not know as a notice instead of failing.

On a CLI terminal, `hestia <plugin> <page>` draws the page and offers its buttons, list-item buttons, table rows (when
the table has a `rowAction`) and form submits as one menu, applying the effects of each action. Without a terminal it
draws once. `--action <id>` runs one action by id, `--set name=value` fills a form field and `--entry <name>` names the
server or instance of an `entry` page. The full flags are in [cli.md](../cli.md#plugin-pages).
