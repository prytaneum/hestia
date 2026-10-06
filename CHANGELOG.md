# Changelog

Release notes, newest first. The release workflow reads the section matching
the version being cut into `latest.json`'s `notes`, so what the updater offers
is what is written here.

One `## <version>` heading per release. Everything until the next heading is
that release's notes, rendered as markdown.

## 1.0.0-beta.2

Shared settings rebuilt around what the game actually writes, a new icon
editor, a guided first run, and fixes for SteamOS and Windows.

- **A welcome flow, guided tours and a getting-started checklist.** First launch
  now opens a short welcome rather than a one-off overlay, and each surface has
  a spotlight tour anchored to the real controls — the shell, browsing content,
  an instance, a server and skins. A tour can be skipped at any point
  instead of having to be turned down before it starts. The library carries a
  checklist for the first four things worth doing: sign in, create an instance,
  play it, host a server. It ticks itself off from your actual state and leaves
  once you are through.
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
- **Resource packs and datapacks sync by identity.** A shared pack is installed
  wherever it fits and enabled or removed everywhere at once; `hestia sync
  packs` manages the library from the CLI. Screenshots are not copied at all —
  every instance's folder reads as one page.
- **Content profiles are gone.** Named profiles added a layer most entries never
  used; installed content is managed in the pool directly.
- **A new icon editor.** Build an entry's icon from a background and a symbol,
  pick from a set of Minecraft-themed images, upload your own, or fetch one from
  a URL. A modpack's icon carries over to the entry it creates, and the library
  cards are redesigned around it.
- **Recently played.** The play bar is now the last five worlds, servers and
  Realms you joined, across every instance: Play launches straight back into
  the most recent, and its menu switches to another. The bar appears once you
  have played something. It fills in from what the game itself reports, so a server picked in
  the in-game list counts too; it needs Minecraft 1.20 or newer. `hestia play
  --recent` and `hestia instance recent` do the same from the CLI, and
  `--realm <id>` joins a Realm on start.
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
  clients. A portable build also resolves one install
  scope however its path is spelled.
- **Dependency security updates**, including the patched `rustls` advisory.

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
