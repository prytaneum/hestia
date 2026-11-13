# The app and the official plugins update together

*Applies to: [The engine](../architecture/engine.md), [Plugins](../architecture/plugins.md), [Packaging](../packaging.md)*

Content sources and flavors are official plugins, so Hestia without them can create nothing. A guest runs only on a host
whose plugin API satisfies the caret requirement it was built against
([0091](0091-plugins-are-wasm-components.md)), so an app update that raises `API_VERSION` past a plugin's requirement
would restart into a launcher where every official plugin failed to load.

**The release manifest names its plugin API, and the daemon stages before it downloads.** `latest.json` carries
`plugin_api`, the `API_VERSION` of the daemon in that release; the release workflow reads it from `crates/proto` so the
two cannot drift, and a manifest without the field asks for no coordination. `update.download` then runs, in this order:

1. Read the manifest and pick the artifact for this install ([0062](0062-the-daemon-owns-self-update.md)).
2. Find the installed official plugins (signed by an official key, never a dev install) whose declared `api` the new
   host would not satisfy. None means every plugin still runs: any earlier staging is discarded and nothing more
   happens, which is the common case, since guests within one major version keep running.
3. Resolve for each the newest build, newer than what is installed, that the new API supports, from the repositories
   pinning its key. The choice is `pick_for`, the catalogue's own rule asked against the new host's API instead of the
   running one.
4. If any has no such build, **refuse with `update_plugins_incompatible`** (the API and the plugin ids) before the
   installer is fetched. A launcher that stays on its current version is better than one that cannot create anything.
5. Otherwise download each package and its signature, check the checksum the index lists and the minisign signature
   against the official keys, and keep both in `<data home>/updates/plugins/<api>/`.
6. Download the app artifact.

**The new daemon installs the staged builds before it loads any plugin.** At start, `install_staged` seeds from
`updates/plugins/<its own API_VERSION>/` through the same path as the bundled plugins
([packaging](../packaging.md#the-bundled-official-plugins)): each package is verified against the official keys again,
installs over the plugin it replaces with the same grant check, and the directory is then removed. A directory staged for
another API belongs to an update not yet applied and is left alone; the next `update.download` clears it. The staging
directory lives in the data home, which every installer leaves alone, so the new daemon finds it however it was installed
and no path has to be handed across the restart. The check is made again by the new daemon rather than trusted from the
old one: nothing about the staged files is believed because it was staged.

**Third-party plugins are not carried.** A plugin the user installed from another repository or a link has no obligation
to the release, and its author may not have a build for the new API. It comes up `needs_update`: not loaded, with the
host's reason (`api ^… is not satisfied by host API …`) as its last error, not `failed`, because the remedy is an update
rather than a fix. It is still listed by the update check, which reads its declared version without validating the
manifest, so the scheduled check a minute after start offers the compatible build when its repository has one.

**A fresh installer needs none of this.** It carries the official plugins built for its own API
([12.09](../packaging.md#the-bundled-official-plugins)), so there is nothing to stage.

**Trust.** The manifest is not signed; only the artifact is. A feed that lied about `plugin_api` could make the update
refuse, or stage builds for an API no daemon will ever start with, which is the same power it already has to withhold an
update. It cannot make anything unsigned run: every staged package is verified twice against keys compiled into a
binary.

**Rejected.** Hosting two plugin API versions at once, so old plugins keep running beside new ones (a second runtime and
linker for a window that closes when the plugins update, and the owner decided against it). Updating the plugins after
the restart (the launcher is unable to create anything for as long as the download takes, or forever offline).
Refusing nothing and letting the plugins fail (the install the user had works; the one they get does not). Passing the
staging path to the installer (a Windows installer cannot carry it into the new process; the data home already crosses
the boundary).
