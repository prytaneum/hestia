# A plugin theme is raw CSS behind a permission, a CSP and a safe mode

*Applies to: [Front-ends](../architecture/frontends.md#desktop--hestia-desktop)*

A plugin can restyle the desktop ([0091](0091-plugins-are-wasm-components.md)).
The choice was between named colour tokens (VS Code's model: a theme is a map
of token to colour, and the host decides what it paints) and raw CSS (Obsidian's
model: a stylesheet loaded after the app's own).

**Raw CSS, from day one, desktop only** (owner decision). It reaches what
tokens cannot — spacing, fonts, shapes, any component — and it needs no
vocabulary the host must keep stable and extend. The cost is that a stylesheet
is untrusted code of a kind: it can hide a button, restyle a confirmation to
look like something else, or load resources. So the decision is the raw CSS
plus what contains it.

**The mitigations.**

- **CSP** ([0093](0093-the-desktop-runs-under-a-csp.md)). A stylesheet cannot
  load a remote image, font or `@import`; `img-src` and `font-src` name only the
  app and the `hestia-plugin` scheme.
- **`ui:css`.** A manifest that contributes a theme must declare the
  permission, or validation refuses it. The install prompt lists the theme ids
  in the grant, and a plugin that adds a theme later widens its grant and is
  asked again.
- **Native dialogs.** Every approval (install, widen, remove) is a native
  dialog drawn by the shell, never in the webview, so no stylesheet can restyle
  or cover the prompt that grants it.
- **Safe mode.** `hestia-desktop --safe-mode` or `HESTIA_SAFE_MODE=1` loads no
  plugin CSS for that run, answered by the `safe_mode` shell command before the
  first stylesheet is injected. A notice offers to clear the theme pref.
- **Opt-in and reversible.** Nothing is injected until the user picks the
  theme (the `theme` pref, [0049](0049-desktop-prefs-live-in-the-data-home.md)).
  A theme that is removed, whose plugin is disabled, or whose stylesheet fails
  to load falls back to the built-in look, and a failed load also clears the
  pref.

**The stable surface.** The shadcn colour variables in `frontend/src/styles.css`
(`--background`, `--primary`, …) are what a theme should override. Component
class names are not a contract. [`theme-ember-night`](https://github.com/prytaneum/plugin-examples/tree/main/plugins/theme-ember-night) models it:
variables only, under `.dark`, no `!important` (so a user's own snippet can
still win, as Obsidian recommends).

**`appearance`.** The app carries the `dark` class. `light` removes it while the
theme is applied, so the theme's `:root` rules apply; `dark` and `both` keep it.

**The wire.** `plugin.themes` returns the package-relative `file`, not a URL:
the `hestia-plugin` URL form differs per platform
([0098](0098-plugin-assets-are-served-by-the-daemon.md)) and only the webview
knows it. The channel is the shell's, not a plugin's (`PLUGIN = None`, listed in
`DENIED`).

Rejected: a token map only (cannot restyle what the host did not name); a theme
without a permission (the prompt is where the user learns a plugin restyles the
app); injecting a stored theme before safe mode is known (a broken theme could
never be escaped).
