# Changelog

Release notes, newest first. The release workflow reads the section matching
the version being cut into `latest.json`'s `notes`, so what the updater offers
is what is written here.

One `## <version>` heading per release. Everything until the next heading is
that release's notes, rendered as markdown.

## 1.0.0-beta.3

Every running job in one place, launch progress where you press Play, and
quieter, clearer notifications.

- **Activity.** The status bar's job list is now an Activity panel: running jobs
  with progress and cancel, recent ones with retry, open-entry and dismiss, and
  a badge for failures you have not seen. Jobs started from the CLI, or before
  the window reloaded, show up too.
- **Launch progress.** The Play button turns into the progress readout while an
  instance launches, with stage, percent, bytes, speed and time left, and the
  play bar keeps showing it on every page. The first-launch dialog is gone.
- **Inline saves.** Settings confirm on the control you used instead of a
  "Saved" toast.
- **Notifications.** Toasts appear bottom-right above the status bar. A job's
  outcome is reported once, errors stay until dismissed, and cancelling or a job
  started elsewhere no longer pops a toast.
- **CLI.** `hestia job list` shows the daemon's running jobs and
  `hestia job cancel <id>` cancels one.

## 1.0.0-beta.2

Servers you can manage from end to end — files, players, scheduled tasks and a
console that stays connected — plus shared settings rebuilt around what the game
actually writes, smarter modpacks and local files, archives that move between
machines, a new icon editor, a guided first run, and fixes for SteamOS, Windows
and the AppImage.

### Servers

- **Server files.** Browse, read, write, create, rename, copy, move, remove,
  compress, extract and import files in a server's directory, from the CLI and
  the desktop. `server.properties` is listed with each setting's default and
  edited inline, and entry settings are reorganised into tabs and sections.
- **Players.** See who is online, the whitelist, ops and bans, and kick players
  or add and remove them from the lists — from the CLI and a new Players tab in
  the desktop, with searchable rows, player heads and a per-player menu.
- **Scheduled tasks.** Run a restart, a console command or a backup on a
  schedule: an interval like `30m`, `6h` or `1d`, down to five minutes, or a
  daily time like `04:30`. The daemon runs them while the server is up; view
  and edit them from the CLI (`hestia server <name> task`) or the server's
  settings. A backup task keeps the newest scheduled archives (7 by default,
  `--keep` to change it) and never prunes a manual one.
- **Restart.** Servers and instance sessions can be restarted in one step.
- **A console that stays connected.** The RCON link is kept open per server
  instead of reconnecting for every command, so the logs stop filling with
  connect and disconnect noise. Commands complete from the server's own `help`
  output — including mods and plugins, and aliases such as `tp` for `teleport` —
  and can be recalled from history, in the CLI and the desktop. The CLI attach
  console clears with Ctrl-L and the desktop console has a clear button. Typing
  works from anywhere on the tab, and your commands now sit in the log where
  they were sent.
- **A world seed at create.** The create wizard takes a seed alongside the other
  details, and the server's settings show it afterwards. It only applies to a
  world that has not been generated yet; left empty, the game picks one.
- **Memory is capped at what the machine has.** Memory sliders stop at the
  installed RAM, and the daemon rejects a larger value.

### Instances

- **Recently played.** The play bar is now the last five worlds, servers and
  Realms you joined, across every instance: Join launches straight back into
  the most recent, and its menu switches to another. The bar appears once you
  have played something. It fills in from what the game itself reports, so a
  server picked in the in-game list counts too; it needs Minecraft 1.20 or
  newer. `hestia play --recent` and `hestia instance recent` do the same from
  the CLI, and `--realm <id>` joins a Realm on start.
- **An entry menu.** Right-clicking a library entry opens its actions, and a
  pinned entry can be played straight from its row.

### Shared settings

- **Sync is a catalogue of units, each merged the way its file needs.**
  Settings, the server list, the command history and the creative hotbars are
  shared through one store and reconciled against what each instance last
  agreed on, so only a side that actually changed wins. The merge writes the
  game's own file back: comments, key order and unknown `servers.dat` tags
  survive, and an unreadable file is skipped rather than replaced. A unit starts
  from an instance you name, keeps a copy of each instance's own file before
  sharing lands, and runs when a session exits and over idle instances too — not
  only at launch.
- **Sync respects the game version.** Options follow a typed catalogue that knows
  which keys each version writes; a unit is skipped below the version that has
  its file (options 1.13, hotbars 1.12, command history 1.20.2), and hotbars are
  kept per item-format era so a pre- and post-1.20.5 instance never trade a file
  the other cannot read. Keys that describe the machine or the moment — display,
  audio, `lastServer` — are never shared, and any key can be pinned local to one
  instance.
- **Resource packs sync by identity.** A shared pack is installed wherever it
  fits and enabled or removed everywhere at once; `hestia sync packs` manages
  the library from the CLI. A pack that could not install somewhere is offered
  again on the next pass rather than read as removed. Datapacks are not synced:
  they belong to a world, and adding one to a world nobody asked about changes
  how it runs. Screenshots are not copied at all — every instance's folder
  reads as one page.
- **Content profiles are gone.** Named profiles added a layer most entries never
  used; installed content is managed in the pool directly.

### Content and modpacks

- **Local files are recognised.** A mod or pack imported from disk is looked up
  on Modrinth and CurseForge by its hash; one they published is installed as
  that release, with its name, icon and updates, instead of as a bare file. The
  Add content dialog also takes files dropped onto it, several at a time, and
  its review step shows what each one resolved to and lets you pick which
  source tracks it — or keep it local. Change version can switch an installed
  item to another source that carries the same file, which is also how a local
  file starts being tracked later. Dependencies are not pulled in, and a file
  nobody recognises installs as before.
- **Better icons for local files.** A local file reads its embedded icon when it
  has one, and an untracked jar can borrow the icon of a source that recognises
  its hash.
- **Packs are identified however they arrived.** A modpack imported from a file
  — a `.mrpack` or a CurseForge archive — resolves its files through the same
  sources as one installed from a source: a mod a source serves is tracked as
  that release, and a jar shipped in an override tree joins the pool too rather
  than landing as an anonymous file. `hestia instance import` recognises a
  CurseForge pack by its manifest, and a pack nested inside another archive is
  read.
- **Update a pack from a file.** Update a modpack from a local `.mrpack` or
  CurseForge archive, with a preview of whether it is the same pack and whether
  it would be a downgrade.
- **Update a local file in place.** A mod you installed from disk takes a new
  build with `update --file` or from its row in the desktop. Its enabled state
  and per-world settings carry over, instead of a second copy landing beside
  the first.
- **Keep a pack's content when you remove it.** Its mods stay in the pool and
  can be managed on their own.
- **Pack versions follow the game version.** A pack's versions are resolved per
  game version, and an update never moves back to an older compatible version.
  A version change is guarded against pack drift in the CLI and the desktop: by
  default the pack moves to the version built for the target, and `--keep-pack`
  leaves it where it is. Picking a pack version shows whether it fits, with a
  warning when it would drift. The modpack card and dialog are decluttered.
- **Faster installs and update checks.** Content batches download
  concurrently, which shortens installing larger packs, and checking an entry
  for updates resolves its items in parallel.

### Import and export

- **A Hestia archive moves between machines.** An instance exported on Windows
  now launches on Linux and the other way round: the archive carries what the
  instance is, and the importing host resolves its own libraries and natives.
  Mods and packs a source can serve again are referenced instead of packed, so
  archives are much smaller; import downloads them and verifies each against
  its hash. Unchecking content in the export picker now leaves the item out
  entirely, so an import never lists something the archive does not have.

### Desktop

- **A welcome flow, guided tours and a getting-started checklist.** First launch
  opens a short welcome rather than a one-off overlay, and each surface has a
  spotlight tour anchored to the real controls — the shell, browsing content, an
  instance, a server and skins. A tour can be skipped at any point instead of
  having to be turned down before it starts, and replayed from settings. The
  library carries a checklist for the first four things worth doing: sign in,
  create an instance, play it, host a server. It ticks itself off from your
  actual state and leaves once you are through.
- **A new icon editor.** Build an entry's icon from a background and a symbol,
  pick from a set of Minecraft-themed images, upload your own, or fetch one from
  a URL. A modpack's icon carries over to the entry it creates, and the library
  cards are redesigned around it.
- **Files drop where they belong.** Every surface that takes one — an instance
  import, the content installer, a skin, an entry's icon — accepts a dragged
  file as well as a picked one, through one shared control.
- **Changes show up everywhere.** What you change from the CLI, the tray or a
  schedule refreshes the desktop straight away.
- **Polish.** An entry's actions and its pin sit in one overflow menu on its
  header, sessions are named by when they started, detail tab toolbars are
  aligned, the session menu stays on the card, and the modpack card is hidden
  when an entry has no pack.

### CLI

- **The create wizard fills in what is missing.** `server create` and
  `instance create` open the wizard whenever a flavor, version, name or (for a
  server) the EULA is missing, starting from whatever you passed — `hestia
  server create fabric` opens at the version step. A fully specified command
  runs without it, as before.
- **Logs follow inline.** `logs -f` streams in the terminal until Ctrl-C
  instead of opening a fullscreen view; the server or game keeps running.

### Changed

- **Scheduled backups are a task.** The `backup-interval` and
  `backup-retention` settings are gone. A schedule set in beta.1 is not carried
  over: add it again with `hestia server <name> task backup <interval>` or
  from the server's Tasks settings.
- **`hestia account login --sisu` is removed.** Sign in with the device-code
  flow.
- **`instance create --modpack` refuses `--memory`.** It was silently ignored.

### Fixes

- **The tray now appears on every Linux desktop that has one**, SteamOS
  included. It reached the status area through libappindicator — a library most
  desktops do not ship — so where it was absent no icon appeared, and the daemon
  spawning the tray on every start wrote a crash report each time. The tray now
  speaks the StatusNotifierItem protocol over D-Bus itself, the way Chromium
  does, so it asks nothing of the machine beyond a desktop that shows a tray at
  all. A session with no tray host yet keeps the icon waiting and registers it
  the moment one appears.
- **The AppImage renders again.** It shipped its own `libwayland-client` and put
  it ahead of the host's, so a system whose graphics stack expects a newer one
  failed to start the webview and the window came up blank. That library now
  comes from the host, where it belongs, and the image is rebuilt around the
  corrected AppDir rather than repacked.
- **Windows: the daemon no longer reads as down under load.** A busy named
  pipe is retried instead of failing the connection, and the daemon re-arms the
  pipe even after a connection fails, so its name never disappears between
  clients. A portable build also resolves one install scope however its path is
  spelled.
- **AppImage: Open folder works, and login autostart survives a reboot.** The
  image's own libraries leaked into the file manager it launched, which then
  crashed; host programs and the daemon now start with the host's environment.
  A login entry registered from an AppImage runs the image itself rather than a
  path on its temporary mount.
- **Nothing is left half-written.** Settings, `server.properties`, synced game
  files, skins, desktop preferences and the BuildTools jar are written through
  a temporary file and renamed into place, and a failed download, archive or
  import cleans up after itself. A backup restore interrupted midway puts the
  server's data back at the next start instead of losing it.
- **Restoring a backup says what it keeps.** The dialog claimed the current
  data was backed up first; it is not, and the dialog now says so.
- **A removed account stays removed** when a token refresh was already in
  flight.
- **Restarting the daemon is reliable.** A restart from the CLI or the desktop
  waits for the old daemon to exit before starting the new one, a stop request
  is no longer lost, and a failed restart after an update is logged.
- **Smaller fixes.** A cancelled job shows as cancelled rather than failed, a
  CurseForge pack is refused as a single content file, removing an entry also
  removes its icon, editing a server list keeps tags Hestia does not know, and
  the update-channel setting shows up in settings search.
- **Dependency security updates**, including the patched `rustls` and `zip`
  advisories.

## 1.0.0-beta.1

The first beta of the 1.0 release. A resident daemon owns everything, and the
desktop app and the `hestia` CLI are two views of the same state — so a server
keeps running when you close the window, and anything one front-end can do, the
other can too.

- **Instances** for vanilla, Fabric and NeoForge. Launch several sessions of
  one instance at once, start straight into a world or onto a server, and move
  an instance between game versions in either direction.
- **Servers** for vanilla, Fabric, NeoForge, Paper, Folia, Spigot and
  CraftBukkit — fully provisioned at create, with their own jar, Java runtime,
  EULA and port, live resource charts, and a console over RCON.
- **Content** from Modrinth or CurseForge, a pasted project link, or a local
  file: mods, plugins, resource packs, shaders and datapacks, with dependencies
  resolved. Update, pin, enable and remove in batches, and slice what is
  installed into named profiles — per instance or global.
- **Modpacks** installed into a new or an existing entry, with an update check
  against the pack's published versions.
- **Backups** for servers, on demand or scheduled, taken live under the RCON
  save-off dance and restored from a stamped manifest.
- **Import and export** — Hestia archives, `.mrpack`, and Prism / MultiMC
  instances.
- **Accounts and skins** — Microsoft sign-in with token rotation, a skin
  library with a real-time 3D preview, the vanilla characters, and your capes.
- **Shared settings** across instances: `options.txt` merged, worlds and
  configs linked into one store, and existing instances adopted into it.
- **Old versions** launch as they should: a pre-1.19 client's LWJGL natives are
  unpacked from their classifier jars, and an asset index predating the hashed
  store is mirrored to the named tree those clients read.
- **Self-update** over a signed release feed, an in-app announcement feed, and
  a system tray beside the running daemon.

This build follows the beta feed, and lands on `1.0.0` by itself once that
ships.
