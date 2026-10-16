# Enable/disable, update-check, and pin extend the same model

*Applies to: [Content & modpacks](../architecture/content.md)*

Beyond add/list/remove/update, an installed item carries an `enabled` flag
(`content.json`, defaulting true so old records decode enabled) toggled by
`content.enable`. Disabling is enforced at the *single* point
the flag needs: the launch-time mirror `sync` keeps a disabled item out of
`data/`, so a disabled mod is never loaded and a backup restore can't resurrect
it; the toggle also applies the filesystem
change immediately (the entry is stopped) so the state is visible before the
next start. A datapack disables the same way, per world: its mirror in the
chosen worlds is dropped ([0016](0016-datapacks-are-world-of-record.md)). `content.check_updates` is a *separate* on-demand call (not baked into
`list`): it resolves each platform item's newest compatible version upstream and
reports which differ, keeping `list` fast and offline. `content.set_version`
re-pins one item to a chosen published version — the update path with an
explicit pin instead of "newest" — as a `ContentManager` job like `update`. All
three refuse a running or busy entry, matching the existing content ops. The
desktop reaches every content operation through the generic bridge; a local-file
import uses `tauri-plugin-dialog`'s native picker to hand the daemon a real
daemon-readable path (a webview `File` has none).
