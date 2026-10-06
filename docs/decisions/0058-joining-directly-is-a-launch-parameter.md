# Joining directly is a launch parameter, not a second launch path

*Applies to: [Servers & instances](../architecture/entries.md)*

Opening the launcher, waiting through the title screen, then clicking through to
a world or a server is the part of playing that nobody wants. Every launcher
worth using can skip it, and Minecraft has taken arguments for it since 1.20
(`--quickPlaySingleplayer <folder>`, `--quickPlayMultiplayer <address>`).

The tempting shape is a second entry point: a `playWorld` channel beside
`launch`, or a manager that prepares an instance and spawns it "with a target".
Both mean two things that must stay in step — token rotation, materialisation,
the sync pass, the content mirror, session numbering, the per-session Log4j2
config, the supervisor hand-off — and the second one is always the one that
falls behind.

**Joining is one optional field on the launch.** `InstanceLaunchParams` grew a
`quickPlay`, and the whole of the difference downstream is two arguments
appended after the manifest's own game args. Everything else — the job, its
progress topics, the warnings that ride out on the result — is the launch that
already existed. The desktop's job store shows a direct join exactly as it shows
any launch, because it *is* one.

Three consequences worth writing down:

- **The target is a variant, not a field per kind.** `QuickPlay::World(folder) |
  Server(address) | Realm(id)`. A launch cannot open a world *and* connect to a
  server, so the type says so once rather than every layer re-checking a set of
  options. (`Realm` arrived with the recent list —
  [0074](0074-the-game-reports-what-was-played.md).)
- **A version that cannot honour it is refused.** The manifest declares these
  args behind feature rules that resolve to nothing here, so support is decided
  by version: below 1.20 (and for any version string that does not read as a
  release triple, such as a snapshot) the launch fails with
  `QuickPlayUnsupported`. The alternative — launch anyway, land on the title
  screen — reports success for something that did not happen.
- **Validation happens before the work.** The world folder must exist, the
  address must parse and a Realm id must be a number *before* Java, jars, libraries and assets are materialised.
  A typo should cost a moment, not a download.

The multiplayer list came along with it, because a server target is only useful
if the servers you already play on are reachable by name. `servers.dat` is read
and written directly (uncompressed NBT), which is the one place hestia edits a
file the *game* owns. A running session holds that list in memory and writes it
back whole on exit, so a write underneath one cannot be made durable. Refusing
the edit would be defensible; instead it is made and returned with a
`ServerListInUse` warning, which is the rule the rest of the launcher already
follows ([0026](0026-degraded-outcomes-ride-on-the-result.md)) — say what could
not be guaranteed rather than deciding for the person.

## What was rejected

- **A `playWorld` / `playServer` channel.** Duplicates the launch pipeline for
  two arguments.
- **Silently falling back to a normal launch on old versions.** A success that
  did not do what was asked is worse than a refusal that says why.
- **Resolving `_minecraft._tcp` SRV records for the ping.** A DNS-record
  dependency for a status line; the address is used verbatim, and a domain that
  publishes only SRV shows as offline rather than pulling in a resolver.
- **Owning the multiplayer list.** Mirroring it into a hestia-side store and
  writing `servers.dat` from that would fight the game for a file it rewrites on
  every exit. The file stays the source of truth.
