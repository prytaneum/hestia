# The host opens with the host's environment

*Applies to: [Front-ends](../architecture/frontends.md#desktop--hestia-desktop)*

"Open folder" did nothing from the AppImage. The image's launcher puts its
mounted AppDir first in `PATH` and `LD_LIBRARY_PATH`, and linuxdeploy's GTK hook
points GTK, GIO and GSettings into it and forces `GDK_BACKEND=x11` and an Adwaita
`GTK_THEME`. The shell needs that to run. But `tauri-plugin-opener` starts
`xdg-open` with the shell's own environment, so the image's bundled `xdg-open`
ran, and the host file manager it launched loaded the image's GStreamer beside
the host's and died in the dynamic linker. The plugin spawns detached, so the
click reported success.

**The shell opens paths and links through its own `open_path`/`open_url`.**
Outside an AppImage they are the plugin's call, `open::that_detached`. Inside
one, the same openers the `open` crate would try are started with the
environment the launcher changed put back: the AppImage markers and the two
forced GTK values removed, and every entry under `$APPDIR` dropped from every
variable — a filter on the path, not a list of names, so a variable a later hook
adds is covered too. The shell itself keeps its environment; WebKit's helper
processes need it.

**Every external link goes through `ExternalLink`.** The plugin's click handler
for `target="_blank"` links calls the plugin's `open_url` directly, so it is
turned off rather than left as a second route. The plugin stays for
`reveal_item_in_dir`, which asks the file manager over D-Bus and starts no child
of the shell. `open_url` keeps the plugin's default schemes (`http`, `https`,
`mailto`, `tel`), since a content description is untrusted.

**Rejected:** clearing the AppImage variables from the shell's own environment
at startup. The WebKit helpers the shell starts would lose the libraries they
are linked against, and setting the environment of a running multi-threaded
process is unsound. **Also rejected:** waiting on the opener to turn its exit
status into an error. Some `xdg-open` backends block until the handler exits, so
any timeout is a guess; the exit status is logged instead.
