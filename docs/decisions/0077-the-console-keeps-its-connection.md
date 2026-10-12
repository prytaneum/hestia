# The console keeps its RCON connection, and completes from the server's own `help`

*Applies to: [Servers & instances](../architecture/entries.md)*

Every RCON call used to open a connection, send one command and drop it. The
vanilla server logs each connect and disconnect, so the players poll alone
wrote two lines to the server log every ten seconds and buried its real output.
The engine now keeps one connection per server (`console::Consoles`) and every
caller — the console, the players poll, the backup's save-off dance — goes
through it. A kept connection is checked for a closed peer before use, so a
restarted server is reconnected to rather than failed against.

Keeping the connection exposed a latent fault: a reply over 4096 characters
arrives as several packets with no end marker, and the client read one. Each
command is now followed by a probe packet of an unknown type, which the server
answers after the last fragment. The probe is sent only once the first fragment
is in, because vanilla drops a connection whose read holds two packets.

Completion has no protocol support — RCON cannot tab-complete — so it is read
from `help <words typed so far>`, which every server answers with the usage of
what may follow. That covers mod and plugin commands and every version, with
nothing hardcoded. The replies are cached beside the connection, since a
server's commands are fixed for a run. Two details are the server's:
replies join their lines with nothing between them, so usages split on `/`; and
a Bukkit server answers `help` with its own index, so `minecraft:help` is tried
first.

`server.command.complete` has no CLI verb of its own
([0041](0041-every-capability-gets-a-verb.md)): it answers a line still being
typed, so it is reached only from the interactive `attach` console and the
desktop's console tab.

**Rejected:** the data generator's `commands.json` knows argument types, but
only for vanilla commands, costs a generator run per version, and would still
need a hardcoded value table per type. **Known limit:** an argument is offered
as its placeholder (`<gamemode>`), except a player argument, which offers who is
online; a redirect past the first word (`execute as … run`) is not followed.
