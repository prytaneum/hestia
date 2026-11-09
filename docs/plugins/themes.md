# Themes

A theme is a stylesheet that restyles the desktop app. It is plain CSS loaded after the app's own, on the desktop only;
the CLI and the tray have no themes. The reasoning, and what it was chosen over, is in
[0100](../decisions/0100-themes-are-plugin-css.md).

## Declaring a theme

A theme-only plugin has no component: just `plugin.toml` and `themes/`. List its folder under the repository's
`exclude`, since `plugins/*` would otherwise expect a crate there ([repository.md](repository.md)). The manifest must
declare `ui:css`, or validation refuses it. From
[`theme-ember-night`](https://github.com/prytaneum/plugin-examples/tree/main/plugins/theme-ember-night), `plugin.toml`:

```toml
id = "theme-ember-night"
name = "Ember Night"
version = "0.1.0"
description = "A warm, low-light theme that only overrides Hestia's colour variables."
permissions = ["ui:css"]

[[contributes.themes]]
id = "ember-night"
name = "Ember Night"
file = "themes/ember-night.css"
appearance = "dark"
```

- `file` is a path inside the package, under `themes/`. A path that escapes the package is refused.
- `appearance` is `light`, `dark` or `both` (the default); see below.
- A plugin may contribute several themes. A plugin that adds one later widens its grant, and the user is asked again.

## The variable contract

The stable surface is the set of CSS variables in `frontend/src/styles.css`: a theme should override these and nothing
else. Component class names are not a contract and change without notice.

| Variables | What |
|---|---|
| `--background`, `--foreground` | the page and its text |
| `--card`, `--card-foreground` | cards and panels |
| `--popover`, `--popover-foreground` | menus, popovers and dialogs |
| `--primary`, `--primary-foreground` | the main action colour and the text on it |
| `--secondary`, `--secondary-foreground` | secondary surfaces and the text on them |
| `--muted`, `--muted-foreground` | quiet surfaces and de-emphasised text |
| `--accent`, `--accent-foreground` | hover and selected surfaces |
| `--destructive` | destructive actions |
| `--border`, `--input`, `--ring` | borders, input borders and the focus ring |
| `--chart-1` to `--chart-5` | chart series |
| `--radius` | the base corner radius; the other radii are computed from it |
| `--sidebar`, `--sidebar-foreground`, `--sidebar-primary`, `--sidebar-primary-foreground`, `--sidebar-accent`, `--sidebar-accent-foreground`, `--sidebar-border`, `--sidebar-ring` | the sidebar |

The app defines each twice, with light values on `:root` and dark values on `.dark`. Colours are `oklch(...)` values,
with alpha where the default has it (`--border` is `oklch(1 0 0 / 10%)` in dark). Any CSS colour works.

If `styles.css` and this table ever disagree, the stylesheet is right.

## Light and dark

The app toggles a `dark` class on the root. Put dark values under `.dark` and light values under `:root`.

`appearance` decides which of the two the app is in while the theme is applied:

| `appearance` | The `dark` class | Write |
|---|---|---|
| `dark` | kept | your values under `.dark` |
| `light` | removed | your values under `:root` |
| `both` | follows the user's setting | values under both `:root` and `.dark` |

The `theme-ember-night` stylesheet declares `appearance = "dark"` and overrides only colour variables, under `.dark`.
From `themes/ember-night.css`:

```css
/* Overrides the shadcn colour variables only: they are the stable surface
 * (frontend/src/styles.css). Component class names are not, and no rule here
 * uses !important, so a user's own snippet can still win. */
.dark {
    --background: oklch(0.16 0.012 50);
    --foreground: oklch(0.95 0.015 70);
    --primary: oklch(0.75 0.16 55);
    --primary-foreground: oklch(0.18 0.02 50);
    --border: oklch(0.95 0.03 70 / 12%);
    /* ... the card, popover, secondary, muted, accent, destructive, input, ring and sidebar variables ... */
}
```

The file sets 26 variables in all. It does not set `--chart-*` or `--radius`, which keep the app's values.

Avoid `!important`, so a user's own snippet can still win.

## What contains a theme

A stylesheet is untrusted code of a kind: it can hide a button or restyle a confirmation. So:

- **`ui:css`** is required, and the install prompt lists the theme ids.
- **The CSP** ([0093](../decisions/0093-the-desktop-runs-under-a-csp.md)) stops a stylesheet loading a remote image, font
  or `@import`. Images and fonts may come only from the app and from the plugin's own package.
- **Approval dialogs** are native windows drawn by the shell, never in the webview, so no stylesheet can restyle or cover
  the prompt that grants a permission.
- **Opt-in.** Nothing is applied until the user picks the theme in Settings, General, Appearance. A theme that is removed,
  whose plugin is disabled, or whose stylesheet fails to load falls back to the built-in look, and a failed load also
  clears the pick.

## Safe mode

If a theme makes the app unusable, start the desktop in safe mode:

```bash
hestia-desktop --safe-mode
HESTIA_SAFE_MODE=1 hestia-desktop
```

No plugin CSS loads for that run, and a notice offers a button that clears the theme pick.

Safe mode is separate on the daemon. `hestia daemon start --safe-mode` (or `HESTIA_SAFE_MODE=1` for `hestiad`) loads
no third-party plugin at all; see [cli.md](../cli.md#daemon-lifecycle).

## Testing a theme

`hestia plugin dev <folder>` installs the folder in place and reloads it when a stylesheet changes (the daemon polls the
manifest, component and `themes/` once a second). A theme-only plugin has no component, so `dev` stages just the
manifest and `themes/`. Pick the theme in Settings, then edit.
