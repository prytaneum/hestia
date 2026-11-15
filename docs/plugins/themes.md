# Themes

A theme is a stylesheet that restyles the desktop app: plain CSS loaded after the app's own. The CLI and the tray have no
themes.

## Declaring a theme

A theme-only plugin has no component, just `plugin.toml` and `themes/`. List its folder under the repository's
`exclude`, since `plugins/*` would otherwise expect a crate there ([repository.md](repository.md)). The manifest must
declare `ui:css`. From
[`theme-ember-night`](https://github.com/prytaneum/plugin-examples/tree/main/plugins/theme-ember-night):

```toml
id = "theme-ember-night"
name = "Ember Night"
version = "0.1.0"
description = "A warm, low-light theme that only overrides Hestia's colour variables."

[permissions]
"ui:css" = {}

[[contributes.themes]]
id = "ember-night"
name = "Ember Night"
file = "themes/ember-night.css"
appearance = "dark"
```

`file` is a path inside the package under `themes/`. A plugin may contribute several themes; adding one later widens the
grant and asks the user again. The user picks a theme in Settings > General > Appearance; nothing is applied until then.
A theme that is removed, disabled or fails to load falls back to the built-in look.

## The variable contract

Override the CSS variables in `frontend/src/styles.css` and nothing else; component class names are not a contract. If
the stylesheet and this table disagree, the stylesheet is right.

| Variables | What |
|---|---|
| `--background`, `--foreground` | the page and its text |
| `--card`, `--card-foreground` | cards and panels |
| `--popover`, `--popover-foreground` | menus, popovers and dialogs |
| `--primary`, `--primary-foreground` | the main action colour and the text on it |
| `--secondary`, `--secondary-foreground` | secondary surfaces and their text |
| `--muted`, `--muted-foreground` | quiet surfaces and de-emphasised text |
| `--accent`, `--accent-foreground` | hover and selected surfaces |
| `--destructive` | destructive actions |
| `--border`, `--input`, `--ring` | borders, input borders and the focus ring |
| `--chart-1` to `--chart-5` | chart series |
| `--radius` | the base corner radius; other radii derive from it |
| `--sidebar`, `--sidebar-foreground`, `--sidebar-primary`, `--sidebar-primary-foreground`, `--sidebar-accent`, `--sidebar-accent-foreground`, `--sidebar-border`, `--sidebar-ring` | the sidebar |

Colours are `oklch(...)` in the app, with alpha where the default has it, but any CSS colour works.

## Light and dark

The app toggles a `dark` class on the root. `appearance` decides which mode it is in while the theme applies:

| `appearance` | The `dark` class | Write |
|---|---|---|
| `dark` | kept | values under `.dark` |
| `light` | removed | values under `:root` |
| `both` (default) | follows the user's setting | values under `:root` and `.dark` |

```css
.dark {
    --background: oklch(0.16 0.012 50);
    --foreground: oklch(0.95 0.015 70);
    --primary: oklch(0.75 0.16 55);
    --primary-foreground: oklch(0.18 0.02 50);
    --border: oklch(0.95 0.03 70 / 12%);
    /* ... the card, popover, secondary, muted, accent, destructive, input, ring and sidebar variables ... */
}
```

From `themes/ember-night.css`. Avoid `!important` so a user's own snippet can still win.

## Limits

The install prompt lists the theme ids. The desktop's CSP stops a stylesheet loading a remote image, font or `@import`;
images and fonts may come only from the app and the plugin's package. Approval dialogs are native windows, so no
stylesheet can restyle the prompt that grants a permission.

If a theme makes the app unusable, start `hestia-desktop --safe-mode` (or `HESTIA_SAFE_MODE=1`): no plugin CSS loads and
a notice offers to clear the theme pick. `hestia daemon start --safe-mode` loads no third-party plugin at all
([cli.md](../cli.md)).

`hestia plugin dev <folder>` installs the folder in place and reloads it when a stylesheet changes.
