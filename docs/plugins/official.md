# The official plugins

Core ships mechanisms and one game flavor, vanilla. Everything else a launcher user expects is a plugin built from
`plugins/<id>/` of the official repository (`prytaneum/plugins`), written against the same API and permissions as any
third party's ([0106](../decisions/0106-the-plugin-api-is-mechanisms-not-features.md)). Twelve exist:

| Id | Provides | Permissions it asks, and why | Settings |
|---|---|---|---|
| `modrinth` | content source: mods, modpacks, resource packs, shaders, data packs and plugins from Modrinth | `content:read` to answer searches and lookups; `network:connect` to `api.modrinth.com` and `cdn.modrinth.com` | none |
| `curseforge` | content source: mods, modpacks, resource packs, shaders and data packs from CurseForge | `content:read`; `network:connect` to `api.curseforge.com` and `*.forgecdn.net` | `api-key` (secret, required): CurseForge serves nothing without one |
| `fabric` | flavor `fabric`, for instances and servers | `flavor:provide` to name what the game runs; `network:connect` to `meta.fabricmc.net` (its version metadata, which publishes no checksums) and `maven.fabricmc.net` | none |
| `neoforge` | flavor `neoforge`, for instances and servers; builds the game jar from NeoForge's installer | `flavor:provide`; `network:connect` to `maven.neoforged.net` and `libraries.minecraft.net`; `process:spawn` for the one declared `java` program, run on the entry's own Java | none |
| `paper` | flavors `paper` and `folia`, for servers | `flavor:provide`; `network:connect` to `fill.papermc.io` and `fill-data.papermc.io` | none |
| `spigot` | flavors `spigot` and `bukkit` (CraftBukkit), for servers; compiled on this computer with SpigotMC's BuildTools | `flavor:provide`; `network:connect` to `hub.spigotmc.org` (BuildTools, which publishes no checksum); `process:spawn` for the declared `git` and `buildtools` programs. Needs `git` installed | none |
| `adoptium` | Java distribution: Eclipse Temurin runtimes | `java:provide` to name the runtime Hestia executes; `network:connect` to `api.adoptium.net`, `github.com` and `release-assets.githubusercontent.com` | none |
| `mrpack` | pack format: import and export of Modrinth `.mrpack` | `pack:provide`; `network:connect` to Modrinth's CDN and the code hosts a pack may link files from | none |
| `curseforge-pack` | pack format: import of CurseForge modpacks; its files resolve through the `curseforge` source | `pack:provide` | none |
| `prism` | pack format: import of Prism Launcher, MultiMC and PolyMC instances | `pack:provide` | none |
| `discord` | Discord Rich Presence for what you are playing; reacts to `process.started`, `process.exit` and `ipc.closed`, and retries its connection every 30 seconds | `entry:read` and `process:read` to name the game; `ipc:connect` to `discord-ipc-*`, the local Discord socket | `enabled` (bool, default on): publish to your profile; off clears the card |
| `players` | a *Players* tab on a server and `hestia server <name> players …` commands: who is online, the whitelist, operators and bans | `entry:read` limited to `whitelist.json`, `ops.json` and `banned-players.json`; `process:control` to send console commands to a running server; `network:connect` to `api.mineatar.io` for player heads | none |

A source that is not ready says why instead of failing: CurseForge is listed by `hestia sources` as waiting on its
`api-key`, and is not offered for browsing or installing until the setting holds a value. Set it in Settings > Plugins or
with `hestia plugin config curseforge api-key`. A distributor can also ship a default for a setting inside the package,
which the user's own value overrides ([0132](../decisions/0132-a-distributor-ships-setting-defaults-in-the-package.md)).

## How they reach you

They are signed by Hestia's own key, which is what makes a plugin official
([0099](../decisions/0099-official-plugins-are-signed.md)): only an official plugin registers a provider under its bare
id (`modrinth`, not `modrinth.modrinth`). Every installer carries them, pinned in `plugins.lock`, and the daemon installs
the ones that are missing on its first start with their grant already approved, since you approved the installer that
carries them ([packaging.md](../packaging.md#the-bundled-official-plugins)). One you remove stays removed. The official
repository is preconfigured and cannot be removed; installed official plugins are updated from it, unattended when the
update asks for no more than is already granted (`plugins.auto-update`), and an app update that changes the plugin API
brings compatible builds with it ([0112](../decisions/0112-the-app-and-official-plugins-update-together.md)).

## Without them

A build with none of them installed is the launcher core: vanilla servers and instances only (`server flavors` lists
one flavor), import and export of Hestia's own archive only, no content source (the browse pages are empty and content
installs from a local file), and no Java distribution to download. A Java runtime already on disk still launches;
`java install` and a create that needs a missing runtime fail with a message that a Java distribution plugin is needed.
Installing, disabling or removing any of them takes effect at once, with no restart.
