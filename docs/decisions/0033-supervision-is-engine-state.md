# Supervision is engine state, and one stop reaches the whole tree

*Applies to: [The engine](../architecture/engine.md)*

The supervisor lived in the daemon, which made two things impossible. Its
directory is `<data_home>/processes/`, engine-owned like every other registry
here, which the daemon could not construct from the engine's own home. And every
engine flow that shells out (NeoForge's processor chain, the `server.properties`
schema run, a Spigot build) had to spawn a bare child, because it could not
reach the supervisor at all — three ad-hoc spawns with no containment, no
records and no adoption. Moving it into the engine settles both: the only thing
it could not know is where its events go, and that is a one-method
[`ProcessEvents`] sink the daemon supplies at boot, so the engine still does not
know a socket exists.

That merge exposed the bug the split had hidden. The supervisor started every
process as its own group leader and then signalled the *pid*, so stopping a
workload orphaned whatever it had spawned. Termination is now the tree — the
negated pid on POSIX, a kill-on-close job object on Windows, since it has no
group that cascades — for servers and game sessions as much as for builds. The
regression test is `crates/engine/tests/process.rs`: it fails against the old
single-pid kill.
