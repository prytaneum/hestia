# A Paper build is a loader version, and Mojang orders the catalogue

*Applies to: [Minecraft providers](../architecture/minecraft.md)*

> Superseded by the official `paper` plugin: core no longer carries the Paper and Folia providers. The rule that a build number is the flavor's loader version holds in the plugin.

Paper and Folia are one self-contained jar per build, so a profile is the
vanilla shape with the jar swapped — the interesting parts are what the PaperMC
API does *not* say. It publishes many builds per game version, and a server
operator routinely needs a specific one (pinning a known-good build, or taking
an experimental one deliberately), which is exactly what `loader_version`
already means for Fabric's loader builds — so a build number goes there rather
than growing a parallel concept. Unpinned resolves to the newest `STABLE` build,
falling back to the newest of any channel: a freshly released game version whose
builds are all experimental would otherwise be uninstallable until PaperMC
promoted one.

Ordering and stability come from **Mojang's manifest**, not PaperMC's. Fill
groups versions under a JSON object keyed by version group, and a parsed object
sorts its keys as strings — which puts `1.9` after `1.21` and would silently
invert `downgrade_between`, the one place ordering is load-bearing. The manifest
is already the ordering ground truth every other flavor is judged against, and
it carries release/snapshot besides, which PaperMC never states. A version
Mojang does not list keeps its place at the end of the list as a snapshot, so an
April Fools' build is still creatable rather than vanishing from the catalogue.

The API itself moved: Fill v2 (`api.papermc.io`) stopped receiving builds at the
end of 2025 and was disabled on 1 July 2026, and v3 refuses a request whose user
agent does not identify its caller — so `common::app::user_agent` now builds one
identity for every outbound request rather than paper alone.
