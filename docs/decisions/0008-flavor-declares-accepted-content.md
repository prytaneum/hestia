# What an entry takes is a property of its flavor, and the flavor says so

*Applies to: [Minecraft providers](../architecture/minecraft.md)*

Two guards used to hard-code the answer: one refused anything but mods and
datapacks on a server, the other refused mods on vanilla *by name*. Paper breaks
both — it loads plugins, which are neither. The rule is now composed from two
independent facts: what the **flavor's** loader consumes (a `ContentKind` on
`FlavorInfo` — mods for a modloader, plugins for a server
platform, nothing for vanilla) and what the **side** reads for itself (a client
its resourcepacks and shaders; either side the datapacks that are world data
rather than loader content). Adding a flavor stays one impl plus one registry
line, with no edit to the content flows — which is what the old tables cost
every time.

A refusal carries the accepted set (`ContentKindRejected`) instead of a
sentence, because a sentence goes stale the moment a flavor is added; the two
`Unsupported` variants that spelled it out are gone. And the *front-end* gets
the set on the wire — `ServerInfo`/`InstanceInfo` carry `accepts` — rather than
keeping its own copy. It had one (`ACCEPTS` per entry type plus a `flavor ===
'fabric'` test), it was already wrong for neoforge, and that is precisely the
drift the no-drift seam exists to prevent.

**A flavor therefore describes itself on the wire, catalogue included.**
`Flavor` is not `{id, name}`: it carries the `summary` a picker renders and the
`accepts` set an entry of it *would* have, so shipping a flavor is a daemon-side
change alone. The composition itself (`accepted_kinds`) sits beside the provider
trait that defines `Loads`, so the catalogue and an existing entry's `accepts`
cannot disagree. The front-ends were each keeping the missing half: the CLI's
flavor table was `ID`/`NAME` and its picker a bare name, and the desktop looked
up a per-flavor `flavor.<id>_summary` message that a new flavor simply did not
have (rendering blank). Both now read the wire — the desktop still *prefers* its
own translation when it has one and falls back to the daemon's English, so a new
flavor renders in every locale immediately and a translated one stays
translated.

Plugins otherwise reuse the managed-dir model unchanged: `<entry>/plugins/`
mirrored into `data/plugins/`, provenance in `content.json`, `plugins/` added to
the backup exclude set so a restore heals it. **Folia is filtered strictly as
`folia`**, never widened to `paper`: a plugin that never claimed Folia support
breaks on its regionised scheduler, and the catalogue is the only place that is
knowable. Verified against a paper-only plugin, which installs on Paper and is
refused on Folia at the same game version.
