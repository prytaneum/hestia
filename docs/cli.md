# CLI reference — `hestia`

The complete command surface of the `hestia` front-end. See the
[README](../README.md) for a quick start and
[architecture.md](architecture.md) for how the CLI sits over the daemon.

## Grammar at a glance

The grammar is **noun-first** and, for anything that touches a specific entry, **entry-first** — the name sits in one
fixed slot right after the noun:

```bash
hestia server create              # catalogue verbs take no entry
hestia server <name> <action>     # everything else names the entry first
hestia server smp start
hestia server smp config set memory 4G
```

Two cross-cutting shortcuts sit on top: `hestia play` (the happy path) and the verb-first
`hestia start|stop|restart|logs|rename <name>`, which resolve a name across both servers and instances so you need not
recall which kind it is.

Conventions: anything a `create` needs but wasn't given is prompted for on a terminal (piped invocations must pass the
flag); `ls`/`rm` alias every list/remove; `-v`/`-vv` raise log verbosity, `-q` quiets to errors; `--home`
overrides the data directory when `hestia daemon start` spawns the daemon.

## Accounts

Microsoft sign-in; `auth` is an alias for `account`.

```bash
hestia account login             # device-code flow — opens the browser, the code
                                 #   shown and copied to the clipboard
hestia account list              # signed-in accounts ('*' marks the one launches use)
hestia account switch [name]     # pick the account launches use (prompts when omitted)
hestia account logout <name|uuid>
```

## Java runtimes

Runtimes are downloaded from the Java provider named by the `java.provider` setting (default `adoptium`, Eclipse
Temurin) or `--provider`.

```bash
hestia java releases             # release lines the provider ships
hestia java install 21           # resolve, download, verify, extract, register
hestia java install 21 --provider <id>        # resolve from another provider
hestia java list                 # installed runtimes
hestia java uninstall 21
```

## Servers

Fully provisioned at create; run under the daemon; each server claims its own port, so several run side by side.
Catalogue verbs take no entry:

```bash
hestia server create             # the fullscreen wizard — edition (java or
                                 #   bedrock) → provider (flavor) → version
                                 #   (type to filter; Tab pulls snapshots in)
                                 #   → name → settings (skippable) → confirm
                                 #   (EULA); Esc steps back. Arguments prefill
                                 #   it, so `server create --edition bedrock`
                                 #   opens at the provider step; it
                                 #   opens whenever flavor, version, name or
                                 #   the EULA is missing and stdin and stderr
                                 #   are terminals. Settings follow the chosen
                                 #   edition's rules
hestia server create [--edition java|bedrock] vanilla 1.21.1 --eula -n smp
                                 #   --edition defaults to java; fully
                                 #   specified, so no
                                 #   wizard and scriptable (-p pins the game
                                 #   port, --memory 4G
                                 #   sets -Xms/-Xmx; --motd, --max-players,
                                 #   --difficulty, --gamemode, --seed cover the
                                 #   common properties, --prop KEY=VALUE the rest)
hestia server list               # managed servers, their address and state
hestia server versions [--edition bedrock] [flavor]  # game versions a flavor offers
hestia server loaders [--edition bedrock] [flavor] [version]
hestia server flavors            # the available flavors per edition, what each
                                 #   takes, and a line on what it is — all from
                                 #   the daemon
```

Everything that acts on one server is entry-first — `hestia server <name> <action>`:

```bash
hestia server smp config list    # memory, jvm-args, and server.properties keys
hestia server smp config set memory 4G          # applies from the next start
hestia server smp config set motd "hi"          # any server.properties key its
                                                #   version knows; a value with a
                                                #   known type or range is checked
hestia server smp update 1.21.4  # move the server to another version (world,
                                 #   ports, config stay, and the data is
                                 #   backed up automatically first; prompts
                                 #   for anything omitted; a downgrade asks
                                 #   for a confirm — --downgrade for scripts;
                                 #   a running server confirms a restart,
                                 #   which the daemon performs around the
                                 #   update — --restart)
hestia server smp backup create  # archive the world + config into the
                                 #   server's backups/ (a running server keeps
                                 #   running; its world saving pauses around
                                 #   the archive)
hestia server smp backup list    # stored backups, newest first
hestia server smp backup restore # replace the data with a backup (prompts for
                                 #   the backup and confirms — --force for
                                 #   scripts; the current jar/libraries stay)
hestia server smp backup remove <backup>
hestia server smp task backup 6h --keep 7  # archive the running server on a
                                 #   schedule; scheduled archives beyond
                                 #   --keep (default 7) are pruned
hestia server smp files ls [dir] # the game directory as a tree; paths are
                                 #   relative to it and never leave it
hestia server smp files cat server.properties
hestia server smp files edit server.properties  # $VISUAL/$EDITOR,
                                 #   saved back if it changed
hestia server smp files write ops.json < ops.json  # create or replace from
                                 #   stdin (text, up to 2 MiB)
hestia server smp files mkdir|mv|cp|rm …  # rm takes several paths; mv and cp
                                 #   never replace what is already there
hestia server smp files upload ./pack.zip --to uploads  # copy local files
                                 #   or directories in, replacing same names
hestia server smp files zip world -o world.zip  # archive files or directories
hestia server smp files unzip world.zip --to restored  # .zip or .tar.gz;
                                 #   beside the archive when --to is omitted
hestia server smp task restart 04:30  # restart every day at that local time
hestia server smp task command 6h say Restarting soon  # a console command on
                                 #   an interval (30m / 6h / 1d, at least 5m)
hestia server smp task list      # ids, schedules, and whether each is paused
hestia server smp task disable|enable|remove <id>  # tasks only run while the
                                 #   server is up
hestia server smp start          # immediate spawn, then attaches the console
                                 #   (-d/--detach returns immediately)
hestia server smp attach         # interactive console: live logs, type to send
                                 #   commands, Esc detaches (alias: console).
                                 #   Tab/Shift-Tab cycle the server's own
                                 #   completions, Ctrl-L clears the view
hestia server smp command say hi # one-shot console command (alias: cmd)
hestia server smp logs -n 50     # captured output (-f streams plainly
                                 #   until Ctrl-C; the server keeps running).
                                 #   -f follows the *server*: it works on a
                                 #   stopped one and rides through a restart
hestia server smp status         # live process state + a running server's ping
hestia server smp info           # descriptor, on-disk folder, and disk footprint
hestia server smp stop           # stop the running server
hestia server smp restart        # stop, then start again
hestia server smp rename cozy    # rename (stopped): rewrites the display name;
                                 #   the id, directory, ports and data stay put
hestia server smp remove         # delete the server (its jar, world and all)
hestia server smp datapack add <slug>   # datapacks install into the server's world
hestia server smp datapack add --file ./pack.zip   # any kind imports a local file
hestia server bds world list     # a server's worlds (the one it runs is marked *)
hestia server bds world import ./w.mcworld --use   # Bedrock, stopped; --use
                                 #   makes it the world the server runs
hestia server bds behaviorpack add ./x.mcpack   # alias: bp; a .mcpack/.mcaddon
                                 #   path or an https link to one
hestia server bds resourcepack add ./x.mcpack
```

What a server takes depends on its flavor: `vanilla` takes datapacks only — `server flavors` prints the set per flavor.
Asking for the wrong kind is refused naming what that server does take.

Only `vanilla` ships, for both editions; other flavors (modloaders, plugin platforms) return with the new plugin system.
A flavor may recommend JVM flags, used unless the entry or `defaults.jvm-args` sets its own; `server <name> info`
reports the effective flags and where they came from.

`bedrock` is Mojang's Bedrock Dedicated Server, for phones, consoles and Windows players. Create one with
`hestia server create --edition bedrock vanilla <version>`; its flavor id is `vanilla`, the same as Java's, told apart by the edition. `server flavors` and
`server list` carry an EDITION column, and the wizard asks for the edition before the provider. A Bedrock server
runs natively, so its wizard has no memory field and `status` shows a `ports` row (game port over TCP, a second over UDP) and no `java` row. It takes
`behaviorpacks` and `resourcepacks` from a local `.mcpack`/`.mcaddon` file or a direct https link. What it refuses, and why:

- `config set memory|jvm-args` — there is no JVM to size.
- `world import` — the server must be stopped, and the file a `.mcworld`.

## Instances

Clients; files materialise at first launch. Same shape: catalogue verbs take no entry, the rest are entry-first.

```bash
hestia instance create           # the fullscreen wizard — flavor →
                                 #   version → name → memory → confirm;
                                 #   arguments prefill it, and it opens
                                 #   whenever flavor, version or name is missing
hestia instance create vanilla 1.21.1 -n play --memory 4G
hestia instance list             # managed instances and their state
hestia instance versions [flavor] # game versions a flavor offers
hestia instance flavors          # the available flavors
hestia instance modded launch    # ensures java/client/libraries/assets, runs,
                                 #   then follows the logs fullscreen
                                 #   (-d/--detach returns immediately)
hestia instance modded launch --new-session   # launch another session while one
                                 #   is already running (off unless
                                 #   instance.multi-session is on — see
                                 #   "Multiple sessions" below)
hestia instance modded update 1.21.4  # move to another version (saves stay,
                                 #   but nothing is backed up — export first
                                 #   if you want a copy; files download at the
                                 #   next launch; a downgrade asks for a
                                 #   confirm)
hestia instance modded config set jvm-args "-XX:+UseG1GC"  # memory / jvm-args
hestia instance modded logs -n 50 # captured output — the newest running session
                                 #   (-f streams plainly until Ctrl-C). -f
                                 #   follows the *instance*, so it picks up
                                 #   the next launch
hestia instance modded info      # descriptor, folder, disk footprint, and each
                                 #   running session (handles)
hestia instance modded worlds    # the save worlds, read from each level.dat:
                                 #   in-game name, folder, version, mode, when
                                 #   it was last played, and its size
hestia instance modded servers   # the multiplayer list (servers.dat), each entry
                                 #   with what it answers right now
hestia instance modded stop      # kill every session (--session <h> targets one)
hestia instance modded restart   # launch again once the old session has exited
                                 #   (--session <h> for one)
hestia instance modded rename mp # rename (stopped): rewrites the display name;
                                 #   the id, directory and saves stay put
hestia instance modded export    # write it out as one archive (stopped)
hestia instance import <file>    # bring one back — any supported archive
hestia instance modded remove    # delete the instance (its saves and all)
```

### Import & export

An instance travels as **one file**. This is what instances have instead of backups: a server is archived in place on a
schedule, an instance is written out on purpose.

```bash
hestia instance modded export                    # → <data home>/exports/<slug>-<stamp>.hestia
hestia instance modded export -o ~/share         # a directory: the name is generated inside it
hestia instance modded export -o ~/cozy.hestia   # or name the file yourself
hestia instance modded export --exclude data/saves   # leave a path out (repeatable)
```

Hestia's own archive is the one format (`--format` takes an id, extension or name the daemon offers, and an unknown one
lists them):

| `--format` | Carries | Use it for |
|---|---|---|
| `hestia` (default) | settings, configs, worlds and local-only files; content from a content source is left out and downloaded on import | keeping a copy, or moving it to another machine, whichever OS it came from |

Import takes a file and works out the rest:

```bash
hestia instance import ~/cozy.hestia             # a hestia archive
hestia instance import ~/cozy.hestia -n "Copy"   # name it something else
```

The format is detected from what is **inside** the file, not its extension — they are all zips, and people rename them.
The command prints what it found before it starts. An archive that is not hestia's is refused as unrecognised. An archive pinning a loader hestia has no flavor for is refused by name rather than silently installed as vanilla.

An export needs the instance **stopped**, and refuses while a content or modpack job is touching it. Paths are resolved
against your shell's working directory, then sent as absolute — the daemon is a separate process and does not share it.

### Playing a world or a server directly

A launch normally stops at the title screen. `--world`, `--server` and `--realm` skip it and join on start (Minecraft's
Quick Play, **1.20 and newer** — an older instance is refused rather than dropped at the menu):

```bash
hestia play modded --world "New World"          # straight into a save
hestia play modded --server smp.example.net     # straight onto a server
hestia play modded --realm 1234567              # straight into a Realm, by its numeric id
hestia instance modded launch --world creative  # the same three flags on launch
```

Whatever a session joins — from these flags or from the game's own menus — is remembered, so the last five targets
across every instance can be gone back to without naming the instance or the target:

```bash
hestia instance recent                          # name, type, target, instance, game mode, when
hestia play --recent                            # pick one and launch straight into it
```

A Realm's id is not shown in-game; the `TARGET` column of `instance recent` is where it surfaces once the Realm has
been joined.

The entry-first spelling picks interactively when the target is omitted, and takes an entry of the instance's own
multiplayer list by name:

```bash
hestia instance modded world play               # prompts with the save worlds
hestia instance modded world play "New World"   # by folder
hestia instance modded server play              # prompts with the multiplayer list
hestia instance modded server play "Mock SMP"   # by list name — or by any address
```

The list is the game's own `servers.dat`, and it can be edited from here:

```bash
hestia instance modded servers                       # position, name, address, live status
hestia instance modded server add smp smp.example.net
hestia instance modded server edit smp --address smp.example.net:25566
hestia instance modded server move smp 1             # to the top of the in-game list
hestia instance modded server remove smp
```

The list's order is the order the game shows, so `move` takes the position a
`servers` listing printed, counting from 1. It resolves to the whole
arrangement, which is what the daemon commits — one rewrite of the game's file
rather than one per move.

One caveat, and hestia says so on the result rather than refusing: a **running game owns that file** and rewrites it
whole when it exits, so an edit made while a session is open is lost with it. Close the game, or add the server from the
in-game multiplayer screen.

### Playing offline

Everything a launch needs is on disk after the first one, so a dropped connection should not be what stops you playing.
Two things follow from that.

A sign-in that is due for rotation and **cannot reach Microsoft** does not fail the launch: the session runs on the
stored token and says so on the result. Singleplayer works; the game refuses multiplayer, which is what an unverified
session is entitled to. A refresh token that Microsoft actively *rejects* is still fatal — that is an expired sign-in,
not a network problem, and it prompts a re-login.

`--offline` asks for that deliberately, and never contacts Microsoft at all:

```bash
hestia play modded --offline     # singleplayer, as the last signed-in account
hestia start modded --offline    # the same, through the entry-agnostic verb
```

Both cases print a warning naming the account, so a session that cannot join a server is never a mystery. An offline
launch needs an account to have been signed in once — it borrows that account's name and uuid so worlds and
per-player data still match.

### Multiple sessions

An instance can run **more than one session at a time**, gated twice. The capability is switched off launcher-wide until
`instance.multi-session` is turned on — two sessions share one `data/`, so it is for users who know what that means —
and each launch then opts in with `--new-session`; without it a running instance is still refused.

```bash
hestia config set instance.multi-session true   # unlock concurrent sessions
hestia play modded               # session 1
hestia play modded               # refused: "already running; pass --new-session"
hestia play modded --new-session # session 2, running alongside session 1
hestia instance modded info      # lists each session with its handle, pid, state
```

Each session writes its own log (`<instance>/logs/session-N.log`), so their output never interleaves. By default `logs`
targets the newest running session and `stop` stops **all** of the instance's sessions — target one with
`--session <handle>` (the handle is the short number `info` shows, or the full process id):

```bash
hestia instance modded stop --session 1      # stop just session 1
hestia instance modded logs --session 2 -f   # follow session 2's output (ends
                                             #   with that session, unlike the
                                             #   instance-wide follow)
hestia instance modded restart --session 1   # replace session 1 (others keep running)

hestia stop modded --session 1               # the shortcuts take it too
hestia logs modded --session 2 -f
```

Sessions share one `data/`, so Minecraft's own `session.lock` arbitrates a singleplayer world (a second session can't
open the same world). Servers stay single — a world has one writer, so `--session` is an instance-only flag.

### Content on an instance

Mods, resource packs, shaders, and datapacks install per entry, from a content source or a local file. No content source ships yet (they return with the new plugin system), so only
`--file` installs today. Every kind
takes a project slug/id, a source page URL, or a local `--file` — or, with no item on a terminal, opens the **fullscreen install session**: a boxed search bar over
live results with a detail pane, space checks any number of items,
`v` pins a version, Enter reviews the batch, and one confirm installs them all as a single job (failures report per
item; the rest proceed):

```bash
hestia instance modded mod add   # fullscreen search → select → review → install
hestia instance modded mod add sodium      # install a mod (resolves required
                                 #   deps; --version pins one; the file is
                                 #   mirrored into the game dir at launch)
hestia instance modded mod add <page-url>  # a source's page URL
hestia instance modded mod add <slug> -S <source>   # from a named source
hestia instance modded mod add --file ./my-mod.jar   # import a local file; one
                                 #   a source recognises is tracked
hestia instance modded mod update my-mod.jar --file ./my-mod-1.1.jar   # a
                                 #   local item takes a new build; enabled,
                                 #   disabled and its worlds carry over
hestia instance modded mod set-version sodium 0.6.0   # re-pin to a version
hestia instance modded mod set-version sodium 0.6.0 -S <source>
                                 # ...taken from another source that carries
                                 #   the installed file
hestia instance modded mod list  # installed mods (+ any untracked jars in the
                                 #   game dir); UPDATE names the version each
                                 #   can move to, and whether it is held
hestia instance modded mod update [sodium ...]   # newest compatible (all but
                                 #   the held ones, or the ones you name — one
                                 #   job either way; naming one releases it)
hestia instance modded mod update --check        # ask upstream now, past the
                                 #   daemon's 30-minute answer cache
hestia instance modded mod hold sodium    # keep it at its version; set-version
                                 #   holds too
hestia instance modded mod unhold sodium  # let it update again
hestia instance modded mod remove sodium [iris ...]   # one call; a name that
                                 #   matches nothing removes none of them
hestia instance modded resourcepack add <slug>   # same verbs for packs/shaders
hestia instance modded shader add <slug>
```

Datapacks load from inside a save world, so an instance datapack names the world (s) it goes into. Run `datapack add`
with no arguments for the fullscreen session — search and check the datapacks, and the review step picks the world (s)
(`w` reopens the picker; space toggles, enter confirms). For scripts, pass the slug and a repeatable `--world`. The same
datapack can live in several worlds at once:

```bash
hestia instance modded datapack add                # 1) search a datapack  2) select world(s)
hestia instance modded datapack add terralith --world Alpha --world Beta
hestia instance modded datapack add --file ./pack.zip --world Alpha
hestia instance modded datapack list      # installed datapacks, with their world
hestia instance modded datapack remove terralith   # removes it from every world
hestia instance modded datapack remove terralith --world Alpha   # only that world's copy
hestia instance modded datapack update [item ...]  # updates it in each world
```

Worlds are shared across instances (linked `saves/` — see below), so a datapack installed into a world is active for
every instance that opens that world. Only the installing instance tracks it; the others list it as untracked world
data.

## Shortcuts

One verb resolves a name across servers and instances, so you need not recall which kind it is (a name matching both
asks you to qualify it).

```bash
hestia play                      # launch an instance — one runs directly, several
                                 #   prompt a pick; follows the logs (-d skips).
                                 #   --new-session runs another alongside a
                                 #   running one (needs instance.multi-session)
hestia start modded              # start a server (attaches its console) or launch
                                 #   an instance (follows its logs); -d/--detach
                                 #   returns immediately
hestia stop modded               # stop whichever it is (all sessions, for an instance)
hestia restart modded            # restart whichever it is (attaches like start)
hestia logs modded -f            # follow its captured output fullscreen (the
                                 #   entry is the subject: a stop is a line in
                                 #   the stream, not the end of it)
```

## Content discovery

Browsing needs a content source; none ships yet, so these verbs report that no source is available. Every browse verb
takes `-S/--source` and defaults to the first source that can serve. `hestia sources` lists every source with the kinds
it catalogues and whether it is ready or needs a setting.

```bash
hestia mod search                # bare, on a terminal: the fullscreen browser
                                 #   (type to search, detail pane, Enter shows
                                 #   versions); filters below seed it
hestia search sodium             # with a query: prints results (alias for
                                 #   `mod search`)
hestia mod search sodium -l fabric -g 1.21.1   # filter by loader / version
hestia modpack search "create"   # browse other kinds: modpack, resourcepack,
hestia resourcepack search faithful            #   shader, datapack, server-plugin
hestia server-plugin search luckperms   # Minecraft server plugins
hestia mod info sodium           # a project's details (downloads, sides, …)
hestia mod versions sodium -l fabric -g 1.21.1  # downloadable versions
hestia mod search jei -S <source>               # search another source
hestia sources                   # the sources that can serve, and what each has
```

## Modpacks

A pack pins its own loader and game version, so installing one *builds* the entry it wants. The one argument takes any
of the three ways to name a pack — an existing path is a pack file, a scheme is a URL, anything
else is a project on the source (`-S`); omit it on a terminal and a searchable picker opens over live search results.

```bash
hestia modpack install fabulously-optimized     # → a new instance
hestia modpack install                          # pick one interactively
hestia modpack install fabulously-optimized --name cozy
hestia modpack install ./pack.zip               # a local pack file
hestia modpack install https://example.com/pack/version/6.2.1
hestia modpack install <pack> -S <source>       # from a named source
hestia modpack install <pack> --server --eula   # → a new server instead
hestia modpack install <pack> --into cozy       # into an existing entry
hestia instance create --modpack <pack>         # the same thing, noun-first
hestia server create --modpack <pack> --eula
```

The pack's mods become ordinary content — `instance <name> mod list` shows them, and each can be updated on its own.
What the entry does with the pack as a whole is entry-first:

```bash
hestia instance cozy modpack status     # which pack, which version
hestia instance cozy modpack update --check   # is there a newer one?
hestia instance cozy modpack update     # → the newest published version
hestia instance cozy modpack update 6.3.0 --downgrade
hestia instance cozy modpack update ./cozy-2.0.zip   # from a pack file
hestia instance cozy modpack remove     # asks: keep its content, or remove it all
hestia instance cozy modpack remove --keep    # forget the pack, keep every mod and file
hestia instance cozy modpack remove --all     # delete them, except files you have edited
hestia server smp modpack status        # servers, the same verbs
```

`update --check` also names the pack version built for the entry's game version when that is not the newest release —
a pre-release, or the line for an older game version — and `update <version>` moves to it.

The other direction is `hestia instance cozy update <game-version>`: when the entry runs a pack built for another game
version, the pack moves with it to its build for the target, and the command says so. `--keep-pack` changes the game
version alone. A pack with no build for the target stays put, with a warning that its mods may not load.

An update carries the entry's game version with it — that is what updating a pack means — so a pack built for an older
version needs `--downgrade`. A pack file is the only way a pack no source published updates, and any pack accepts one;
a file holding a different pack needs `--other-pack`. On a terminal both are asked rather than refused. Files the pack wrote into the game directory that you have since edited are never
overwritten or deleted; the command says which it kept.

## Content updates

```bash
hestia updates                   # every entry's known content updates: entry,
                                 #   item, current and latest version, held
hestia updates --apply           # move them, entry by entry, kind by kind;
                                 #   held items stay, a running entry is refused
```

The daemon checks for content updates when it starts, when the network comes back, as an instance launches and when an
entry's content is listed, and keeps each answer for 30 minutes — so `hestia updates` answers at once without asking
upstream. `instance list` and `server list` carry an UPDATES column, and `server <name> status` an `updates` row, from the
same answers. With `content.auto-update` on, an instance's updates apply as it launches (held items and a modpack's own
content excepted) and a stopped server's as soon as they are found.

## Jobs

```bash
hestia job list                  # running jobs: id, channel, entry, age, stage
hestia job cancel <id>           # cancel one by the id `job list` shows
```

## Plugins

```bash
hestia plugin list               # every plugin: id, name, version, state, requires
hestia plugin enable <id>        # also enables what it requires, first
hestia plugin disable <id>       # refused while an enabled plugin requires it
```

## Download cache

```bash
hestia cache info                # size and entry count
hestia cache list                # cached blobs
hestia cache clear               # evict everything
```

## Shared settings

Five things can be kept the same across your instances: your **game settings**, your **multiplayer list**, your
**command history**, your **creative hotbars** and your **screenshots**. The first four are merged at every launch, when
a session exits, and over idle instances whenever the catalogue changes — key by key for the settings, row by row for the
server list, slot by slot for the hotbars — so two instances changing different things both survive. Screenshots are the
exception: nothing is copied, they are simply read where the game wrote them. Worlds are never shared.

Each one is off until you turn it on, and turning it on asks which instance the shared copy starts from — that choice is
the point, since letting whichever instance launched first decide is how settings go missing:

```bash
hestia sync status                    # what is shared, and where each instance stands
hestia sync on options --from cozy    # start the shared settings from 'cozy'
hestia sync on servers                # asks which instance, unless only one has a list
hestia sync off commands              # stop sharing; every instance keeps what it has
hestia sync on screenshots            # read every instance's shots as one listing
hestia sync on resourcepacks          # install your packs into every instance they fit
```

The shared game settings can be read and changed without opening the game, and any one of them can be dropped out of
sharing so every instance keeps its own value for it:

```bash
hestia sync options                       # every shared setting and its value
hestia sync options set fov 90            # applies at each instance's next launch
hestia sync options local guiScale        # each instance keeps its own guiScale
hestia sync options share guiScale        # …and back
```

One instance can differ from the rest without leaving: it can keep a whole unit to itself, or pin individual settings
local while sharing everything else. Nothing here moves a file — the next launch simply reconciles differently.

```bash
hestia instance modded sync            # where this instance stands
hestia instance modded sync off options  # it keeps its own settings
hestia instance modded sync on options   # …and back
hestia instance modded sync local fov    # keep one setting to this instance
hestia instance modded sync share fov    # …and back
```

Settings cross every version: a keybind saved as a number before 1.13 and one saved as a name after it are the same
setting, and a value is written in each instance's own spelling. The distances and driver-level toggles start out pinned
local, since a weaker machine has to be free to disagree. A unit whose file the version never wrote is skipped instead —
the command history needs 1.20.2, the hotbars 1.12 — and `sync status` says so on that instance's row.

The shared pack library is a list of identities, never files or versions — a pack is installed into every instance it
fits, and loses its place everywhere at once:

```bash
hestia sync packs                      # every pack the instances share
hestia sync packs disable cozy         # installed everywhere, loaded nowhere
hestia sync packs rm cozy              # instances lose it at their next pass
```

Screenshots are listed and deleted where they lie:

```bash
hestia screenshot list                   # every instance that shares them, newest first
hestia screenshot list --instance modded # one instance, shared or not
hestia screenshot rm modded 2026-09-01_18.42.10.png
```

Sync is **instance-only**: a server's configuration is per-server infrastructure, managed through
`server <name> config …` and
`server.properties`, and is never shared.

## Configuration

Typed settings, stored as JSON.

```bash
hestia config get <key>          # read a setting
hestia config set <key> <value>  # write a setting
hestia config list               # every setting
hestia config get home           # resolved data directory
hestia config set home <dir>     # persist the data dir (empty reverts) and restart the daemon; refused while anything runs
hestia config get autostart      # true if the daemon starts at login
hestia config set autostart true # register the daemon to start at login
hestia config set instance.multi-session true # allow an instance to run several sessions
hestia config set content.auto-update true    # apply content updates on launch,
                                              #   and to a stopped server when found
hestia config set network.offline true        # stop reaching the network at all
```

`network.offline` pins the launcher offline. Nothing is attempted while it is set, so anything that needs upstream
refuses immediately with a typed error naming the service instead of waiting out a connect timeout. Version catalogues
already fetched are still answered from their cached copy, so creating an instance on a version you have picked before
keeps working. `hestia daemon status` reports the current state on its `network` line — `online`, `checking`,
`offline — upstream is unreachable`, or `offline (pinned by network.offline)`. The unpinned states are observed from the
requests the daemon actually makes, so the line never disagrees with what a command is about to do.

The data directory is resolved as: `--home` → `$HESTIA_HOME` → a persisted pointer (`config set home`) → the platform
default (`~/.hestia`, or
`%APPDATA%\Hestia` on Windows). **Debug builds** anchor the default at
`target/<profile>/data` so development never populates the real per-user directory.

### Modpack corrections

Modpacks routinely declare client-only mods as server-compatible, which is how a pack that plays fine as an instance
breaks as a server. Hestia ships the same correction table `itzg/docker-minecraft-server` maintains, applied to
**server**
installs only:

```bash
hestia config set modpack.default-excludes false      # trust the pack's own env
hestia config set modpack.force-include-files sodium  # install one anyway
hestia config set modpack.exclude-files "mod-a, mod-b # why"
hestia config set modpack.overrides-exclusions "config/**"
```

The three list keys take comma- or newline-separated entries with `#` comments, matched case-insensitively against any
part of a file's path — so a
docker-mc-server exclude list pastes in unchanged.
`overrides-exclusions` takes ant-style patterns (`?` one character, `*` a run within one path segment, `**` a run across
segments). Anything held back is reported on the install result, naming each file.

## Daemon lifecycle

Servers and instances keep running across daemon stops/restarts and are re-adopted by the next daemon.

```bash
hestia daemon status             # is the daemon running, and what is it supervising
hestia daemon start              # start it
hestia daemon restart            # restart it (workloads survive)
hestia daemon stop               # asks about running workloads (piped: --all/--keep)
hestia daemon stop --all         # stop supervised processes too
hestia daemon stop --keep        # leave them running (script-safe)
```

Underneath the entry verbs sits the supervisor's own view — every workload the daemon is running, across servers and
instances, keyed the way the supervisor keys them. Reach for it when the entry-scoped verbs cannot answer: a server's
`status` cannot show you an instance session, and neither shows a process whose entry was removed out from under it.

```bash
hestia process list              # everything supervised, with state and pid
hestia process status server-<id>   # one process (exits 3 when not running)
hestia process logs instance-<id>_1 -n 50   # its captured output
hestia process stop server-<id>  # SIGTERM, then a hard kill after the grace
```

A plain `hestia daemon stop` with a server or instance running does **not**
guess: "stop the launcher" says nothing about the server, so it prompts on a terminal and, when piped, exits non-zero
naming both flags — a script has to state which it meant. With nothing running there is nothing to ask and it stops
immediately. The tray's **Quit** and the desktop's stop button always mean
`--keep`: neither can ask, and neither should end a running server on your behalf.

## Updating Hestia

```bash
hestia update                    # check, confirm, download and apply
hestia update --yes              # no confirmation prompt
```

The installer is verified against a compiled-in signing key before it runs; an artifact that fails to verify is
discarded rather than applied. The desktop has its own updater under Settings (it can replace a running binary and
restart into it, which the CLI path cannot).

## Global flags

Accepted in any position.

```bash
hestia -v java list              # debug logging: what the command is doing
hestia -vv java list             # …plus the wire: every frame this process sent
                                 #   and received, with its channel, correlation
                                 #   id, size and round-trip time
hestia -q java list              # errors only on the console
hestia --home /path/to/dir config get home
hestia --json server list        # machine-readable output, see below
hestia --version
```

Diagnostics also land in `logs/hestia.log` regardless of the console level.
`-vv` is specifically for a CLI-versus-daemon disagreement: it shows the conversation from this side, which the daemon's
own logs cannot. Payloads are never logged — they carry access tokens and RCON passwords — so a frame reports its size,
not its contents.

## Machine-readable output (`--json`)

`--json` makes any command print one JSON document on stdout (UTF-8, newline-terminated, no colour, no spinners, no
progress). Human messages never mix in; diagnostics stay in `logs/hestia.log`.

```json
{"data": <value>, "warnings": [{"kind": "…", "message": "…", "hint": "…"}]}
```

`data` follows what the command shows:

| the command shows           | `data` is                                                                             |
|-----------------------------|---------------------------------------------------------------------------------------|
| a table                     | an array of objects keyed by snake_case column (`GAME VERSION` is `game_version`)     |
| a list with nothing in it   | `[]`, never a message                                                                 |
| a key/value block           | one object keyed by snake_case key                                                    |
| a line or note              | `{"message": "…"}`                                                                    |
| several of the above        | an array of each, in the order shown                                                  |
| nothing                     | `null`                                                                                |

Table and block values are typed, not the strings the terminal shows:

| a value that is…                  | in JSON                                                                          |
|-----------------------------------|----------------------------------------------------------------------------------|
| a count, a pid, a port            | a number                                                                         |
| a size                            | a number of bytes (`1288490188`, not `1.2 GB`)                                   |
| a duration                        | a number of seconds                                                              |
| a point in time                   | an RFC 3339 string in UTC (`2026-11-15T07:02:53Z`), not `3d ago`                 |
| a yes/no                          | `true` or `false`                                                                |
| a state or kind                   | its wire token (`running`, `data_pack`, `release`), not the label shown          |
| a list (`mods, datapacks`)        | an array of tokens                                                               |
| nothing (`-`)                     | `null`                                                                           |
| anything else                     | a string                                                                         |

Where a column's header reads badly as a key (a unit, a marker, an abbreviation), the table names its key: `backup
list` calls its `AGE` column `created`, and `account list` calls its unnamed marker column `default`. `warnings` carries what a result reports as degraded, with the tag of its `kind`
(the same vocabulary as the wire's `WarningInfo`) and the hint for fixing it; a warning a view raises itself has a null
`kind` and `hint`.

A command that reports the state of one thing is one object with `state` first: `hestia --json daemon status` is
`{"state": "running", "pid": 4242, "uptime": 93, "network": "online", "home": "…", "log": "…"}`,
and a stopped daemon is `{"state": "stopped"}` (still exit code `3`). `network` is `online`, `offline` or `unknown`,
`null` when the daemon could not say.

A failure prints one object on **stderr**, with the same exit code as without the flag. `kind` is the daemon's error
tag (`entry_not_found`, `sign_in_required`, …), `code` its coarse category (`not_found`, `unauthorized`,
`bad_request`, `forbidden`, `offline`, `handler_error`, …); a refusal made by the CLI itself, such as a missing
argument, is `local`/`local`. `hint` is the next step a person can take, present only when there is one (a signed-out
account, the daemon not running); without `--json` it is
printed under the error:

```json
{"error": {"kind": "sign_in_required", "code": "unauthorized", "message": "sign in with a Microsoft account first", "hint": "run `hestia account login`"}}
```

`--json` can follow the subcommand or precede it. It never prompts: a command that would ask something fails like it
does without a terminal, naming the argument or flag that answers it. Interactive sessions (`server <name> attach`, the
resource monitor) refuse it, and the content browser and the paged table fall back to their plain output, so
`hestia mod search sodium --json` returns the hits.

`hestia logs <name> -f --json` is a stream: newline-delimited JSON, one object per line, flushed as it arrives
(`{"message": "…"}` per line of output). Its history, printed before following starts, uses the same lines. A stream has
no `data` envelope.

## Exit codes

A state query has two honest answers, and a shell reads only the exit code — so
"the daemon is stopped" must not look like "the daemon is running", nor like
"I could not tell". The vocabulary is systemd's:

| code | meaning                                                                 |
|------|-------------------------------------------------------------------------|
| `0`  | the command did what was asked; a state query found the subject running |
| `3`  | the query was answered and the subject is **not** running               |
| `1`  | the command failed — no daemon, invalid input, or the operation errored |
| `2`  | usage error (an unknown flag or subcommand)                             |

```bash
if hestia daemon status >/dev/null; then echo up; fi   # true only when running
hestia server smp status >/dev/null; case $? in
  0) echo running ;;
  3) echo stopped ;;
  *) echo "could not ask" ;;
esac
```

Only verbs that assert whether one subject is running use `3` — `daemon status`
and `server <name> status`. Verbs that *describe* rather than assert (`info`,
`sync status`, every `list`) always exit `0`: "inactive" is not a claim they make.
