# An entry's icon lives in the entry's directory, kept by the daemon

*Applies to: [Servers & instances](../architecture/entries.md), [Import & export](../architecture/transfer.md), [Front-ends](../architecture/frontends.md)*

Icons started as a desktop feature: the shell copied a picked image to `<data_home>/icons/<entry-id>.<ext>`, baked
generated ones there with a sidecar, and had to delete both when an entry was removed. That kept them out of every
operation the daemon performs on an entry: an export left the icon behind, an import from Prism dropped the one the
instance had, and the CLI and the tray could not see it.

**The icon is part of the entry.** It is `icon.<ext>` (PNG, JPEG, WebP or GIF, judged by its bytes, at most 10 MB)
in the entry's root, beside its record, with `icon.json` recording the background and symbol of a generated one. The
entry root is Hestia's namespace ([0018](0018-entry-root-versus-data-dir.md)), so the game never sees it. Because it
lives there, it moves with the entry, travels in a hestia archive and is removed with the entry, with nothing keyed by
id to keep in step. A Prism/MultiMC import takes the instance's `iconKey` image, or `icon.png`, the way Prism looks
it up.

**The daemon owns writes.** `entry.icon.set` takes a file the user picked or a PNG the webview baked, and an entry
created from a modpack takes the pack's icon (https only, size-capped while streaming). The server and instance views
carry the icon's path, a revision for cache busting and the generator. The shell keeps one command, `icons_allow`,
which widens the asset scope to exactly `<home>/<servers|instances>/<entry>/icon.<ext>` after resolving links, never
the directory around it, which holds logs an old client writes its session id into.

**`icon.json` is untrusted on read.** It may have arrived in somebody's archive and its colors reach a stylesheet, so
a generator that is not `#rrggbb` colors and a plain symbol id is dropped, leaving the image.

Rejected:

- **Keeping the desktop store and teaching the exporter about it.** The daemon would read a front-end's directory,
  and every other front-end would still see no icon.
- **Carrying the icon only inside the archive manifest.** The entry directory is already what the archive copies; a
  second representation is a second thing to keep in step.
- **A plugin channel.** `entry.icon.set` reads any daemon-local path, so it is closed to plugins like the other
  any-path channels ([0092](0092-a-contract-names-the-permission-a-plugin-needs.md)).
