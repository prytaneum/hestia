# A flavor builds on the vanilla profile, and the host merges it

*Applies to: [Plugins](../architecture/plugins.md#provider-surfaces), [Minecraft providers](../architecture/minecraft.md)*

A modloader is vanilla plus a few things: Fabric's client is vanilla's libraries, natives, assets and arguments with its
own libraries, main class and arguments laid over them, and its server is a launcher jar that fetches the game itself.
Core builds these by reading Mojang's version JSON in the flavor and merging. A plugin flavor that did the same would
carry a second copy of Mojang's schema (libraries and their rules, natives, the asset index, argument templates), keep it
in step with every change, and need Mojang's hosts in its grant for files the user's launcher already trusts.

**The reply says what is added.** `resolve-client` and `resolve-server` answer either `full` (a whole profile, as before)
or `onVanilla`: a `ClientAddition` (`loaderVersion`, `libraries`, `mainClass`, `jvmArgs`, `gameArgs`) or a
`ServerAddition` (`loaderVersion`, `jar`, `jvmArgs`). The host resolves the vanilla flavor's profile for the requested
game version and layers the addition over it (`minecraft/overlay.rs`):

- libraries merge by Maven coordinate without its version, keeping a jar and its natives classifier apart; the addition
  wins and first-seen order holds, because two versions of one artifact on the classpath abort a loader (this is the
  merge core's Fabric and NeoForge already used, now shared);
- the main class is the addition's when it names one;
- JVM and game arguments follow vanilla's;
- the client jar, natives, asset index and Java major are vanilla's, and so is the server's Java major; a server addition's
  `jar` replaces vanilla's, and its `launch` (2026-11-14, [0121](0121-a-build-places-trees-and-the-profile-launches-from-an-args-file.md)) replaces vanilla's jar launch, vetted as a full profile's is.

**Only the addition is vetted.** Its libraries and jar pass the same host, filename and checksum rules as a full profile.
Vanilla's own artifacts are the host's, from Mojang, and are not checked against the plugin's hosts.

**The base is the registered vanilla flavor.** A provider is handed it at registration, so the merge uses the same
resolution as a vanilla entry, including its cached catalogues offline, and a test engine supplies its own.

Rejected:

- **The guest reads Mojang's JSON.** It duplicates the schema and widens every loader plugin's network grant.
- **A `vanilla: bool` on the full profile.** It leaves a stored profile claiming to be incomplete and has no place for what
  the flavor adds as against what it replaces.
- **A flavor-specific merge.** Nothing in the host names Fabric or NeoForge; a layer is data.
