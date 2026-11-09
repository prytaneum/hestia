# The BDS catalogue merges Mojang's links with the Bedrock-OSS feed

*Applies to: [Minecraft providers](../architecture/minecraft.md)*

Mojang's download endpoint names the current stable and preview server zips
only, with no hashes, so it cannot pin an old version or support a downgrade.
The Bedrock-OSS `BDS-Versions` feed lists every Linux and Windows build since
1.6 and the minecraft.net path each is served from.

**The catalogue is both, merged** (`minecraft/meta/bedrock.rs`): the two are
fetched together, deduplicated by version id and ordered newest first by version
number. Mojang is authoritative for the newest build and the feed supplies
history; either alone still answers, with a warning, and both missing is an
error. Both responses are cached in `meta/catalogue/` like every catalogue
([0050](0050-offline-is-one-state.md)). A downgrade is judged by position in the
list, as for every flavor.

**Preview builds are `VersionKind::Snapshot`.** They reuse the release/snapshot
filter every picker already has, rather than a new kind.

**Downloads are pinned to one host.** Mojang publishes no hash, so the check is
where the zip comes from: a resolved URL must sit under
`https://www.minecraft.net/bedrockdedicatedserver/` and contain no `..`,
whichever source supplied it, or the resolve is refused. A platform with no BDS
build is `Unsupported::BedrockHost`.

**An update never overwrites what the user owns.** The new zip extracts over
`data/` except `server.properties`, `allowlist.json`, `permissions.json`,
`worlds/` and `config/`, which are kept once they exist; directories merge, so
packs a user added survive. A pre-update backup runs first, as for Java.

**Rejected.** Mojang's endpoint alone, which allows no pinning. The community
feed alone, which makes the newest build depend on a third party. Inventing a
hash. Recreating `data/` on update.
