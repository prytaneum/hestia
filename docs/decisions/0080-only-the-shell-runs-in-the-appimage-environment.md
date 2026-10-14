# Only the shell runs in the AppImage's environment

*Applies to: [Cross-cutting foundations](../architecture/common.md#the-appimage-environment),
[The daemon](../architecture/daemon.md#autostart),
[Front-ends](../architecture/frontends.md#desktop--hestia-desktop)*

"Open folder" did nothing from the AppImage. The image's launcher puts its
mounted AppDir first in `PATH` and `LD_LIBRARY_PATH`, and linuxdeploy's GTK hook
points GTK, GIO and GSettings into it and forces `GDK_BACKEND=x11` and an Adwaita
`GTK_THEME`. The shell needs that to run. But `tauri-plugin-opener` starts
`xdg-open` with the shell's own environment, so the image's bundled `xdg-open`
ran, and the host file manager it launched loaded the image's GStreamer beside
the host's and died in the dynamic linker. The plugin spawns detached, so the
click reported success.

The opener was one child among several. The shell also spawns `hestiad`, which
handed the same environment to every game, Java runtime and `git` it started —
pointing at a mount that only lives as long as something holds it open.

**`common::appimage` puts the host's environment back.** It removes the
AppImage markers and the two forced GTK values, and drops every entry under
`$APPDIR` from every variable — a filter on the path, not a list of names, so a
variable a later hook adds is covered too. Outside an AppImage it returns
nothing and callers leave the environment alone.

**Every process the shell starts gets it, except the shell's own internals.**
The shell keeps its environment: WebKit's helper processes need it, and
setting the environment of a running multi-threaded process is unsound.

- *Host programs* get `host_env`. The shell opens paths and links through its own
  `open_path`/`open_url`, which start the openers the `open` crate would try with
  that environment. The plugin stays for `reveal_item_in_dir`, which asks the file
  manager over D-Bus and starts nothing.
- *The daemon* gets `sidecar_env`, the same plus `$APPIMAGE`. It links nothing
  from the image (only libc), and it still needs to know the image it came from:
  self-update replaces that file.
- *The tray*, inheriting the daemon's, relaunches the shell through `$APPIMAGE`
  rather than the binary beside it, since the shell runs only under the image's
  launcher — WebKit finds its helpers relative to the launcher's working
  directory.

**The image is the way back in.** Anything that has to start Hestia again later
names `$APPIMAGE`, never a path on the mount, which only lives as long as
something holds it open. The tray relaunches the shell that way, and a login
entry registered from an AppImage runs `"<image>" --daemon`: the shell is the
image's only entry point, so the flag has it start the daemon and exit without
a window.

**Every external link goes through `ExternalLink`.** The plugin's click handler
for `target="_blank"` links calls the plugin's `open_url` directly, so it is
turned off rather than left as a second route. `open_url` keeps the plugin's
default schemes (`http`, `https`, `mailto`, `tel`), since a content description
is untrusted.

**Rejected:** clearing the environment per child inside the daemon. The daemon
has nothing from the image to keep, so every launch site would repeat the same
call for no reason. **Also rejected:** waiting on the opener to turn its exit
status into an error. Some `xdg-open` backends block until the handler exits, so
any timeout is a guess; the exit status is logged instead.
