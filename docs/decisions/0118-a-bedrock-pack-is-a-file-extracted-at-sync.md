# A Bedrock pack is a file of record, extracted at sync

*Applies to: [Content & modpacks](../architecture/content.md)*

Bedrock add-ons are behaviour and resource packs, shipped as `.mcpack` (a zip)
or `.mcaddon` (several packs). BDS reads them as folders, listed per world by
uuid in `world_behavior_packs.json` and `world_resource_packs.json`. Hestia's
install model keeps a file in the entry's managed dir, with its sha1 and
provenance in `content.json`, and mirrors it into `data/`
([0013](0013-managed-dir-of-record.md)).

**The pack file stays the item.** It lives in the managed dir like a mod, so
update, pin, enable, removal and untracked detection work unchanged. Behaviour
packs use `behavior_packs/`; resource packs keep the existing `resourcepacks/`
rather than a Bedrock-only directory. A `.mcaddon` is split into its packs on
install. At every start, `sync` extracts each enabled pack into
`data/behavior_packs/<uuid>/` or `data/resource_packs/<uuid>/` and rewrites the
world's pack JSON from the enabled set.

**Foreign entries are never touched.** A folder Hestia extracted carries a
`.hestia` marker and is the only kind it replaces or removes. Packs BDS ships are
recorded in `.bds-packs` at install, and a pack the user added by hand is left
as found. In the world's pack JSON only Hestia's own uuids are rewritten; every
other entry is carried over unchanged, and a file that is not valid JSON is left
alone.

**`ContentKind::BehaviorPack` is new; Bedrock resource packs reuse
`ResourcePack`.** The kind means the same thing to the user, and the edition
separates the file formats. What an entry accepts is the edition's base set plus
its flavor's loader: a Bedrock server takes behaviour and resource packs, a Java
server datapacks.

**A content source declares the editions it serves.** `ContentProvider::editions`
defaults to Java and `ContentSource.editions` carries it, so a front-end offers a
source only to entries of those editions and `ensure_serves` refuses the rest
with `ContentSourceEdition`. Modrinth and CurseForge are Java sources, so a
Bedrock entry installs from a local file or a direct link. A source also skips
kinds it does not catalogue rather than failing per kind.

**Rejected.** Storing the extracted directory as the item, which loses the hash,
the provenance and the untracked check. A `BedrockResourcePack` kind. A
per-front-end list of which source serves which edition.
