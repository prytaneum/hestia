# Design decisions

Every non-trivial architectural choice in Hestia, recorded next to the reason it
was made — what changed, what it replaced, and what was rejected. The
[architecture](../architecture.md) pages describe *what the system is*; these
describe *why it is that*.

Each entry is one decision, numbered in the order it was written down, and links
back to the subsystem page it explains. The numbering stays contiguous: a
superseded decision is rewritten in place, and one whose feature is removed is
deleted with the rest renumbered behind it, so the list never has holes in it.

**Adding one:** when you make a call that a future reader would otherwise have to
reverse-engineer, add a file here (`NNNN-kebab-title.md`, next number free), open
with an *Applies to* line pointing at its subsystem page, and list it below. State
the alternative you rejected — that is usually the part nobody can recover later.

---

### [The socket boundary](../architecture/wire.md)

- **0001** — [The envelope seam fails closed, so no decode site can forget the version check](0001-envelope-fails-closed.md)
- **0026** — [A degraded outcome rides on the result, never only in the log](0026-degraded-outcomes-ride-on-the-result.md)
- **0027** — [A warning the user did not cause is a bug in the launcher, not a notice](0027-warnings-the-user-did-not-cause.md)
- **0028** — [Everything serialized is camelCase, except the `config.*` key vocabulary and upstream DTOs](0028-camelcase-except-the-config-vocabulary.md)
- **0032** — [A job is cancelled by asking, at safe checkpoints — never by disconnecting](0032-jobs-are-cancelled-by-asking.md)
- **0045** — [A session routes events to its listeners, so event-driven calls do not exclude each other](0045-a-session-routes-events-to-its-listeners.md)
- **0063** — [An endpoint is scoped exactly as the data home behind it](0063-an-endpoint-is-scoped-like-its-data-home.md)
- **0083** — [A job is its start contract, and every job reports on one topic](0083-a-job-is-its-contract-and-reports-on-one-topic.md)
- **0084** — [The TypeScript channel table is generated from the contracts, and a contract owns its timeout](0084-the-ts-channel-table-is-generated-from-the-contracts.md)
- **0085** — [A mutating call names the state it changes, and the registrar publishes it on one topic](0085-a-mutating-call-names-what-it-changed.md)
- **0086** — [An entry's tree has one claim, held for as long as a job or handler changes it](0086-an-entry-has-one-claim.md)

### [Cross-cutting foundations](../architecture/common.md)

- **0002** — [Two log files, because one file cannot be both readable and complete](0002-two-log-files.md)
- **0003** — [A crash must survive the process that had no console](0003-crash-reports-survive-the-process.md)
- **0054** — [Materialised game files live under one `meta/` root](0054-meta-root-for-materialised-files.md)

### [The engine](../architecture/engine.md)

- **0024** — [A temp artifact is only valid while its job holds the claim — so a restart invalidates every one](0024-temp-artifacts-are-reclaimed-at-startup.md)
- **0033** — [Supervision is engine state, and one stop reaches the whole tree](0033-supervision-is-engine-state.md)
- **0034** — [Workloads outlive the daemon by design](0034-workloads-outlive-the-daemon.md)
- **0035** — [A finished process is labelled, not merely unrecorded](0035-a-finished-process-is-tombstoned.md)
- **0037** — [Following logs is scoped to the entry, not to one run of it](0037-following-logs-is-entry-scoped.md)
- **0060** — [A managed document carries its own schema version, and an unreadable one is set aside](0060-a-managed-document-carries-its-schema-version.md)
- **0062** — [The daemon owns self-update; the shell asks like everything else](0062-the-daemon-owns-self-update.md)
- **0066** — [A channel picks the feed, not an entry inside one](0066-a-channel-picks-the-feed-not-the-entry.md)
- **0067** — [Reachability is observed from real traffic, and offline is a state the whole system reads](0067-reachability-is-observed-not-asked.md)
- **0068** — [The tray speaks the tray protocol, not a library that might be installed](0068-the-tray-speaks-the-tray-protocol.md)

### [The daemon](../architecture/daemon.md)

- **0029** — [No Service-class-per-prefix — but one registrar function per domain](0029-one-registrar-per-domain.md)
- **0030** — [Instances are gated on a signed-in account, in the router](0030-instance-surface-gated-on-an-account.md)
- **0031** — [An aggregation point is a directory, not a file](0031-an-aggregation-point-is-a-directory.md)
- **0036** — [Stopping the daemon has three meanings; the front-end picks one, the wire carries two](0036-stopping-the-daemon-has-three-meanings.md)
- **0061** — [A job declares what differs; the runner owns the rest](0061-a-job-declares-what-differs.md)
- **0059** — [Discord presence belongs to the daemon, and is a loop rather than a hook](0059-discord-presence-is-a-daemon-loop.md)

### [Minecraft providers](../architecture/minecraft.md)

- **0004** — [NeoForge's game jar is built, not downloaded — so a flavor can install](0004-neoforge-builds-its-own-jar.md)
- **0005** — [A Paper build is a loader version, and Mojang orders the catalogue](0005-paper-build-is-a-loader-version.md)
- **0006** — [A flavor states what it needs, before the user commits to it](0006-flavor-states-its-requirements.md)
- **0007** — [Spigot and CraftBukkit are compiled here, because no one may ship them](0007-spigot-is-compiled-locally.md)
- **0008** — [What an entry takes is a property of its flavor, and the flavor says so](0008-flavor-declares-accepted-content.md)
- **0009** — [A flavor may recommend JVM flags; the user still outranks it](0009-flavor-recommends-jvm-flags.md)

### [Servers & instances](../architecture/entries.md)

- **0018** — [The entry root is hestia's; `data/` is the game's](0018-entry-root-versus-data-dir.md)
- **0019** — [Sync is a closed catalogue, and each unit names its own mechanism](0019-sync-is-a-closed-catalogue.md)
- **0065** — [A synced unit reconciles against a baseline, not a clock](0065-sync-reconciles-against-a-baseline.md)
- **0069** — [A unit shares only what its game version writes](0069-a-unit-shares-only-what-its-version-writes.md)
- **0070** — [The merge writes the game's own file, not its own rendering of it](0070-the-merge-writes-the-games-own-file.md)
- **0071** — [Options sync through a typed catalogue, not raw key-value pairs](0071-options-sync-through-a-typed-catalogue.md)
- **0072** — [A pack syncs by identity, not by file — and only where it fits](0072-a-pack-syncs-by-identity.md)
- **0073** — [Screenshots aggregate rather than sync — nothing is copied](0073-screenshots-aggregate-rather-than-sync.md)
- **0082** — [Config values pass one typed schema, and property values come from a curated table](0082-config-values-pass-one-typed-schema.md)
- **0020** — [The id is an opaque uuid; the directory is the slug — decoupled](0020-id-is-a-uuid-directory-is-a-slug.md)
- **0021** — [Backups follow docker-mc-backup, minus what the launcher already owns](0021-backups-follow-docker-mc-backup.md)
- **0022** — [A world describes itself; a directory listing does not](0022-a-world-describes-itself.md)
- **0023** — [An unfinished record says which kind of unfinished, so recovery can act on it](0023-server-phase-over-a-ready-bool.md)
- **0025** — [The properties schema is generated, not maintained — and it is not the file](0025-properties-schema-is-generated.md)
- **0038** — [An instance runs many sessions; a server runs one](0038-an-instance-runs-many-sessions.md)
- **0039** — [Per-session logs come from a generated Log4j2 config, not a captured pipe](0039-per-session-log4j-config.md)
- **0053** — [Server provisioning is front-loaded by design](0053-server-provisioning-is-front-loaded.md)
- **0055** — [The server console is RCON, not a stdin pipe](0055-the-console-is-rcon-not-a-pipe.md)
- **0058** — [Joining directly is a launch parameter, not a second launch path](0058-joining-directly-is-a-launch-parameter.md)
- **0074** — [The game reports what was played; the launcher only keeps the history](0074-the-game-reports-what-was-played.md)

### [Content & modpacks](../architecture/content.md)

- **0010** — [Content is normalized behind one trait, following Prism's `ResourceAPI`](0010-one-content-provider-trait.md)
- **0011** — [A modpack is three things at once, and each goes where it already belongs](0011-modpack-decomposes-into-existing-parts.md)
- **0012** — [A pack's `env.server` is a claim, not a fact — so a server install corrects it](0012-pack-server-declarations-are-corrected.md)
- **0013** — [Installed content is managed-dir-of-record, mirrored into `data/`](0013-managed-dir-of-record.md)
- **0014** — [Enable/disable, update-check, and pin extend the same model](0014-enable-update-check-and-pin.md)
- **0015** — [A local-file import is inspected, not trusted](0015-local-imports-are-inspected.md)
- **0016** — [Datapacks are managed like every other kind, mirrored per world](0016-datapacks-are-world-of-record.md)
- **0064** — [A record of the user's is mutated, never rebuilt](0064-a-record-is-mutated-not-rebuilt.md)
- **0075** — [A local file is identified by its hash](0075-a-local-file-is-identified-by-its-hash.md)
- **0076** — [A pack file is identified wherever it came from](0076-a-pack-file-is-identified-wherever-it-came-from.md)
- **0078** — [A pack updates from a file, and leaves an entry two ways](0078-a-pack-updates-from-a-file.md)
- **0079** — [A local item updates from a file](0079-a-local-item-updates-from-a-file.md)
- **0077** — [The console keeps its RCON connection, and completes from the server's own `help`](0077-the-console-keeps-its-connection.md)

### [Accounts & skins](../architecture/accounts.md)

- **0017** — [Skins follow Modrinth's shape, minus its couplings — and skip the CLI](0017-skins-follow-modrinth-minus-couplings.md)

### [Import & export](../architecture/transfer.md)

- **0057** — [An archive format is a module, not a branch — and the launcher matches on the recipe](0057-an-archive-format-is-a-module.md)
- **0081** — [The archive carries the instance's identity and references](0081-the-archive-carries-identity-and-references.md)

### [Front-ends](../architecture/frontends.md)

- **0040** — [Entry-first, with verb-first shortcuts for the hot path](0040-entry-first-cli-grammar.md)
- **0041** — [Every daemon capability gets a scriptable verb, or a written reason it has none](0041-every-capability-gets-a-verb.md)
- **0042** — [`-vv` buys wire visibility, not more volume](0042-vv-buys-wire-visibility.md)
- **0043** — [A state query answers through its exit code, not only its stdout](0043-state-queries-answer-through-exit-codes.md)
- **0044** — [Interaction is fullscreen; bare progress is one line](0044-fullscreen-interaction-inline-progress.md)
- **0046** — [The desktop bridge is one generic command, not a facade mirror](0046-desktop-bridge-is-one-generic-command.md)
- **0047** — [Messages are organised on one axis — where the string is rendered — and split one file per root](0047-messages-organised-by-render-surface.md)
- **0048** — [Sign-in is the one bespoke shell command — it must be](0048-sisu-sign-in-is-a-shell-command.md)
- **0049** — [Front-end preferences are desktop-local, in the data home — not the daemon](0049-desktop-prefs-live-in-the-data-home.md)
- **0050** — [Offline is one state, not a failure per read — and the shell brings its own daemon up](0050-offline-is-one-state.md)
- **0051** — [The daemon spawns the tray; the tray outlives the daemon](0051-the-daemon-spawns-the-tray.md)
- **0052** — [The tray and desktop must not share a GApplication id](0052-tray-and-desktop-app-ids.md)
- **0056** — [Job progress paints once per frame, because the store re-renders synchronously](0056-job-progress-paints-once-per-frame.md)
- **0080** — [Only the shell runs in the AppImage's environment](0080-only-the-shell-runs-in-the-appimage-environment.md)
- **0087** — [The data home is fixed for the life of a daemon process](0087-the-data-home-is-fixed-for-a-daemon-process.md)
