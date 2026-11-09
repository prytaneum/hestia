# The server console is RCON, not a stdin pipe

*Applies to: [Servers & instances](../architecture/entries.md)*

A supervised process is deliberately decoupled from the daemon's lifetime — no
`kill_on_drop`, no pipes back — which decides how a console can reach it. A
stdin pipe exists only between a parent and the child it spawned, so it cannot
be re-established for an adopted process, and it dies with every daemon
restart. RCON is re-establishable TCP state: any daemon can connect to any
running server whose port and password it knows, both of which the server's
record persists. So `server <name> command` and the interactive `attach` speak
the vanilla remote-console protocol over localhost rather than writing to the
process.

Log streaming needed nothing new for the same reason: output already lives in
files the process writes itself, tailed into `process.output` events.

One caveat is inherited from vanilla: rcon has no bind-address setting, so the
listener is network-reachable, and the per-server random password is the only
barrier. It never appears in logs.

See also [0114](0114-a-bedrock-console-is-a-reattachable-stdin-endpoint.md): a Bedrock server has no RCON, and
its console is a stdin endpoint the supervisor keeps across daemon restarts.
