# Content updates are checked when they matter, not on a timer

*Applies to: [Content & modpacks](../architecture/content.md)*

A user should learn that a mod has an update without pressing "check". Polling
every entry on an interval is the obvious way, and the wrong one: a library of
twenty instances with a hundred mods each costs thousands of requests a cycle
against a platform that allows 300 a minute per IP, and most of those answers
are wanted by nobody. Modrinth's own launcher checks an instance when its page
opens and keeps each answer for 30 minutes; Prism checks only when asked. Both
look up many files at once by hash.

**A check runs at the moments its answer is used.** The daemon checks every
entry at startup and when the network comes back, which feeds the library
badges, `hestia updates` and the notification. Opening an entry's content tab,
or `<kind> list` in the CLI, checks that entry, the way a mod menu does. Launching
an instance checks before the game starts, because that is when an update
takes effect. There is no timer.

**Answers are cached by what they depend on.** The engine keeps one answer per
installed release, game version and loader, for a TTL of 30 minutes. A check
inside the TTL asks upstream nothing, and an answer past it is still listed
until a check replaces it. The installed version is part of the key, so an
update retires its own answer and nothing has to invalidate the cache.

**Detection is bulk; the version is still the update's own pick.** Items are
grouped by source, game version and loader across every entry. Modrinth's
`/version_files/update` takes all of a group's hashes in one request, release
channel first. CurseForge's bulk `/mods` returns each mod's newest file per
game version and loader. Only an item the bulk answer says has moved is
resolved exactly, through the same release-first, never-backwards pick an
update uses. A source with no bulk lookup resolves each item that way.

**Auto-update is opt-in and never moves what the user pinned.**
`content.auto-update` applies updates as an instance launches (first session
only, since another one has the mirror in use). For a stopped, idle server it
applies them as soon as a check finds them, so `server.start` stays an instant
call. A held item is passed by. So is anything a modpack installed, because the
pack's own version decides those. Choosing a version holds an item, and updating
it by name releases the hold. Items auto-update cannot move ride on the launch
as `ContentNotUpdated`, and the game still starts.

**Rejected.** A fixed polling interval: it spends requests on answers nobody
reads and still misses the moment one is wanted. A per-front-end check: the CLI
is short-lived, and two front-ends would ask the same questions twice. Making
`server.start` a job so a server could update right before it starts: every
caller, including scheduled restarts, would change for a moment that applying
when stopped already covers. Treating a bulk answer as the final version: it
does not know the item is installed ahead of the newest release.
