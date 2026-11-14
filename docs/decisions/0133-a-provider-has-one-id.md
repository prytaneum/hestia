# A provider has one id, and a bare name is a lookup shorthand

*Applies to: [Plugins](../architecture/plugins.md#providers), [provider guide](../plugins/providers.md), [official plugins](../plugins/official.md)*

> Supersedes [0116](0116-an-official-provider-shadows-the-builtin.md) and the bare-id rule of
> [0099](0099-official-plugins-are-signed.md); the rest of 0099 (what "official" means, the key, the preconfigured
> repository) stands.

A provider a plugin contributed (a flavor, a content source, a pack format, a Java distribution) got its id by two
paths: a plugin signed by the official key registered the bare `fabric` and could shadow a core built-in of that id,
and any other plugin was namespaced as `fabric.fabric`. A developer-mode `--official` flag added a third. The same
provider therefore had a different id depending on who signed it, an entry record could name an id that another build
registered differently, and the shadowing machinery (two roster entries per id, a liveness check, a fallback) existed
for built-ins that core no longer ships.

**One id.** Every plugin provider is registered as `<plugin-id>.<provider-id>`, whoever signed it. There is no bare
registration for plugins and no shadowing. Core's own providers (the vanilla flavor, the `hestia` archive format) keep
their bare ids; no plugin can take them, since a plugin id is part of every id it registers.

**A bare name is a lookup shorthand, resolved in one place.** `Roster::resolve` is the only code that turns a name
into a provider, for every kind. A full id means exactly itself. A bare name `x` means, in order: core's provider `x`;
else the provider `x` of the one official plugin that has it; else the only registered provider with that
provider-id; else a typed failure that lists full ids (`ProviderUnknown` with what is available, `ProviderAmbiguous`
with the candidates). It applies wherever a name comes in: a CLI argument, a wire request, an entry record, the content
index, a pack's loader through its interchange key, `java.provider`, a default source.

**Stored ids are full ids.** What core persists and sends (entry records, a content record's source, `ServerInfo.flavor`,
lists) is the id of the provider that served, because a provider's own profile and versions carry its full id. There is
no migration (greenfield); a stored bare name still resolves through the shorthand.

**"Official" keeps only its trust meaning**: a bundled install without a prompt, unattended updates, the badge. Its one
effect on ids is the tie-break above. `devOfficial` and `plugin dev --official` are deleted; a dev install is a plugin
like any other. While developer mode is on, a dev install ranks as official for that tie-break, and only where no
signed official plugin provides the name, so a port can be tried before it is signed and can never displace a signed
one. Nothing else reads it.

**Interchange keys** go through the same order: a loader key one flavor already claimed stays with it, and the roster
lists core first, then official plugins, then dev installs, then the rest, so an official plugin's claim of a key wins
over another plugin's. A flavor that declares no loader of its own is filed under its provider-id (`fabric`), not its
full id.

**A Hestia archive names its loader by the provider-id** (`fabric`), so it reads the same wherever another plugin
supplies that flavor; import resolves it as any bare name, and a full id in an archive resolves to exactly itself.

**Front-ends show the short name when it is unambiguous**: the provider-id when exactly one registered provider has it,
the full id otherwise (`commands/names.rs` in the CLI, `useProviderNamer` in the desktop).

Why this shape. Terraform addresses every provider as `namespace/type`; `hashicorp` is only the default namespace you
may omit, not a different kind of provider. VS Code extension ids are always `publisher.name`, never a bare name for
the favoured ones. npm's flat, first-come package names needed scopes bolted on later to stop squatting and
confusion. Trust and naming are separate questions, so the id is the same for everyone and trust only breaks a tie in
the shorthand.

Rejected: flat first-come names, where whoever installs first owns `paper` and a third party can capture a name an
official plugin would later want; and keeping two registration paths, which keeps an id dependent on a signature and
keeps the shadowing code.
