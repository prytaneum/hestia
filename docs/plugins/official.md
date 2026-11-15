# The official plugins

Core ships mechanisms and one game flavor, vanilla. Everything else a launcher user expects is a plugin in the official
repository (`prytaneum/plugins`), written against the same API and permissions as any third party's. Fourteen exist:

| Id | Provides | Permissions it asks | Settings |
|---|---|---|---|
| `modrinth` | content source: mods, modpacks, resource packs, shaders, data packs and plugins | `content:read`; `network:connect` to `api.modrinth.com`, `cdn.modrinth.com` | none |
| `curseforge` | content source: mods, modpacks, resource packs, shaders and data packs | `content:read`; `network:connect` to `api.curseforge.com`, `*.forgecdn.net` | `api-key` (secret, required) |
| `fabric` | flavor `fabric`, for instances and servers | `flavor:provide`; `network:connect` to `meta.fabricmc.net` (no checksums published) and `maven.fabricmc.net` | none |
| `neoforge` | flavor `neoforge`, for instances and servers; builds the game jar from NeoForge's installer | `flavor:provide`; `network:connect` to `maven.neoforged.net`, `libraries.minecraft.net`; `process:spawn` for one `java` program | none |
| `paper` | flavors `paper` and `folia`, for servers | `flavor:provide`; `network:connect` to `fill.papermc.io`, `fill-data.papermc.io` | none |
| `spigot` | flavors `spigot` and `bukkit`, for servers; compiled locally with SpigotMC's BuildTools (needs `git`) | `flavor:provide`; `network:connect` to `hub.spigotmc.org`; `process:spawn` for `git` and `buildtools` | none |
| `adoptium` | Java distribution: Eclipse Temurin | `java:provide`; `network:connect` to `api.adoptium.net`, `github.com`, `release-assets.githubusercontent.com` | none |
| `mrpack` | pack format: import and export of Modrinth `.mrpack` | `pack:provide`; `network:connect` to Modrinth's CDN and the code hosts a pack may link | none |
| `curseforge-pack` | pack format: import of CurseForge modpacks, resolved through the `curseforge` source | `pack:provide` | none |
| `prism` | pack format: import of Prism Launcher, MultiMC and PolyMC instances | `pack:provide` | none |
| `discord` | Discord Rich Presence for what you are playing | `entry:read`, `process:read`; `ipc:connect` to `discord-ipc-*` | `enabled` (bool, default on) |
| `players` | a *Players* tab and `hestia server <name> players ...`: online players, whitelist, operators, bans | `entry:read` limited to `whitelist.json`, `ops.json`, `banned-players.json`; `process:control`; `network:connect` to `api.mineatar.io` | none |
| `tailscale` | a *Tailscale* tab and `hestia server <name> tailscale`: the tailnet address players join on (needs Tailscale installed and signed in) | `entry:read`; `process:spawn` for `tailscale status --json` | none |
| `playit` | a *playit.gg* tab and `hestia server <name> playit`: a public tunnel, up while the server runs; `hestia playit setup` and `reset` | `entry:read`, `process:read`; `process:spawn` for `agent` (playitd, fetched into `data/` and pinned by hash) and `playitd` (on `PATH`); `network:connect` to `api.playit.gg`, `github.com` and its release asset hosts | `agent` (`download` or `installed`), `secret-key` (secret) |

A source that is not ready says why: `hestia sources` lists CurseForge as waiting on its `api-key`, and it is not offered
for browsing or installing until the setting has a value (the plugin's page under Plugins, or
`hestia plugin config curseforge api-key`). The official build ships a default key in its package, so it works as
installed, and the user's own value overrides it.

## How they reach you

Hestia's own key signs them, which is what makes a plugin official. Their providers are `modrinth.modrinth` like any
plugin's, and the bare `modrinth` resolves to them before another plugin's of that name. Every installer carries them,
pinned in `plugins.lock`, and the daemon installs the missing ones on first start with the grant already approved, since
you approved the installer. One you remove stays removed. They update from the official repository, unattended when the
update asks for no more than is already granted (`plugins.auto-update`), and an app update that changes the plugin API
brings compatible builds with it ([packaging.md](../packaging.md)).

## Without them

With none installed Hestia is the launcher core: vanilla servers and instances, import and export of Hestia's own
archive, no content source (browse pages are empty; content installs from a local file) and no Java distribution to
download. A Java runtime already on disk still launches; `java install` and a create that needs a missing runtime say a
Java distribution plugin is needed. Installing, disabling or removing one takes effect at once.
