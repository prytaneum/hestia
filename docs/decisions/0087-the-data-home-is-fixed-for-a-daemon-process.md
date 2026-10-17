# The data home is fixed for the life of a daemon process

*Applies to: [The engine](../architecture/engine.md), [The daemon](../architecture/daemon.md)*

`config set home` used to repoint a running daemon: ten subsystems held their
directory behind a `Mutex<PathBuf>` and a `reload()`, and the engine re-resolved
the home on the fly. That only moved the paths, not the state. Logging and crash
reporting stay on the old home, the supervisor never adopted the new home's
records, and the job managers, schedulers and in-memory caches kept serving the
old one. Boot is the one path that settles a home correctly, so the daemon now
takes it.

**The home is resolved once, at construction.** Every subsystem holds a plain
`PathBuf`; there is no `Engine::set_data_home`, no `reload()`, and the catalogue
store's root is set once.

**Moving the home is a restart the daemon drives.** The `home` handler refuses
while a server, game session or job is running (`WorkRunning`), validates the
directory (absolute, creatable), writes the pointer, replies, and requests its
own stop without stopping processes. The CLI and the desktop wait for that
connection to close, then start the daemon; any other front-end reconnects. The
endpoint is scoped per install, not per home, so a start before the close would
reach the daemon on its way out.
Workloads outlive the daemon ([0034](0034-workloads-outlive-the-daemon.md)),
which is what makes the restart cheap.

Rejected: leaving the stop to the client that issued the change. If it died, the
daemon would keep serving the old home while every newly started front-end
resolved the new one.
