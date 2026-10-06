# The game reports what was played; the launcher only keeps the history

*Applies to: [Servers & instances](../architecture/entries.md#recently-played)*

"Recently played" is the official launcher's Quick Play row: the last few
worlds, servers and Realms, each one click from being rejoined. It needs to know
what a session joined — including a server picked from the in-game list, or a
Realm, neither of which the launch asked for.

**The game is asked, not watched.** Every launch of a 1.20+ instance passes
`--quickPlayPath`, and the client rewrites that file on each login with what it
entered: a type (`singleplayer` / `multiplayer` / `realms`), an id (the world
folder, the address, the Realm id), a display name and the game mode. That is
the official launcher's own input, and it is the only source that covers all
three kinds — a Realm in particular is otherwise invisible from outside the
game, short of a Realms API client.

Three properties of that file shape the rest, each read off the client itself
(`net.minecraft.client.quickplay.QuickPlayLog`, 26.2):

- **It is written on every join, not only a quick-play one.** `handleLogin`
  writes it; a world load, a server connect and a Realms connect each set what
  it will say. A session opened to the title screen reports what it joins from
  there.
- **It holds the latest join only.** The file is deleted and rewritten as a
  one-element array, so it is "what this session is in", never a list. The
  history is therefore the launcher's to keep: each join is folded into the
  instance's `recent.json` as a session exits, and again whenever the list is
  read, so a running session's world shows without waiting for it to end.
- **The path resolves against the game directory, so an absolute one stands.**
  The log goes beside the session's own log — `logs/session-<seq>.quickplay.json`
  under the entry root — one per session, because concurrent sessions of an
  instance would otherwise overwrite each other's latest join, and outside
  `data/`, so a backup and an export leave it behind.

The history is per instance, capped at five, and the list across instances is
the five most recent of those. It lives in the entry root rather than in one
launcher-wide file: it follows a rename and goes with the instance, with no
second index to disagree with the directory. An export skips it — where and when
its owner played is not part of the instance.

A target is left out of the list once it cannot be joined: a world whose folder
is gone, or an instance moved below 1.20. The record keeps it; the list does not
offer a launch that would be refused.

Joining a Realm came along with this, as the third `QuickPlay` variant
([0058](0058-joining-directly-is-a-launch-parameter.md)). A Realm is named by its
numeric id, which the game never shows — the recent list is where a person first
sees one.

## What was rejected

- **Scanning `level.dat` and scraping the game log**, which is how Modrinth's app
  builds its equivalent: worlds by their own `LastPlayed`, servers by matching
  `Connecting to <host>, <port>` in the session output. It works below 1.20 and
  needs no argument, but it is two mechanisms where one suffices, the log line
  carries the *resolved* address rather than the one in the list, and it cannot
  see a Realm at all. A pre-1.20 instance cannot be quick-played anyway, so
  nothing it could list could be launched.
- **A Realms API client.** Listing a player's Realms is the only way to offer one
  that has never been joined from here. "Recently played" does not need that —
  the game reports the id and name of the one that was.
- **Reading the log's own `lastPlayedTime`.** It is the moment the file was
  written, which the file's mtime already says without a date parser.
- **Pinning and custom icons**, the rest of the official row. A pin is state the
  game does not report, so it needs a store and an editing surface of its own;
  the capped list was the request.
