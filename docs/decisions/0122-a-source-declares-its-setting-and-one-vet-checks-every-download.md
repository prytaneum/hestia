# A source declares the setting it needs, and one vet checks every download a plugin names

*Applies to: [Plugins](../architecture/plugins.md#providers), [Content & modpacks](../architecture/content.md#sources), [provider operations](../plugins/providers.md#content-sources)*

Two things were patched port by port. A source that cannot work until the user sets an API key was hidden from
`content.sources`, so the UI could not say why it was missing, and a first fix asked the guest an `available` operation
after every settings change. And "may the engine fetch this file" had grown three spellings: a flavor artifact needed a
well-formed checksum or an `unhashed` host, a content file only needed *a* checksum, a Java build needed a sha256, and a
pack file named by a source's `resolve-manifest` had no rule at all.

**Availability is declared.** A source's `info` carries `requires-setting`, the key of one of the plugin's own settings.
The host, not the guest, decides: the source serves while that setting holds a value (not unset, not a blank string), and
reads the settings file when asked, so a change made by the user or by the plugin takes effect at once with no refresh
and no guest call. A key the plugin does not declare refuses the source at connect. `content.sources` lists a source that
waits on a setting, with `requiresSetting { plugin?, key }`, so a front-end can say which setting is missing; one
that is disabled is not listed. A call that names it is still refused as `ContentSourceUnavailable`. The built-in
CurseForge reports its config key through the same field.

**One vet.** `Policy::vet_artifact` is the rule for every file a plugin names: https on a granted host, and a
well-formed checksum, or `unhashed` from a host the grant lists as serving unhashed files. Content versions, flavor
profiles, Java builds and the files of a resolved pack all call it; a refusal is a typed `Refused` that carries which
part failed. A pack file may have an empty URL, which is a file its author withholds and which the download reports by
name; its path must also stay inside the entry. The `pack` provider kind uses the same call.

**Identify mounts the files.** `identify` hands the guest `{ sha1, path }` for each local file, mounted read-only for that
call only, so the guest can compute the hash its source indexes by (CurseForge's fingerprint). The host links the files
into a directory of their own beside the plugin's data (copying across filesystems) and removes it after the call; the
guest sees nothing else of the host.

**`resolve-manifest` stays an optional operation** that turns a pack manifest the source owns into a `ResolvedModpack`.

## Not taken

- **An `available` guest operation.** It costs a guest call per settings change, can disagree with the stored setting, and
  cannot be explained to the user. A declared key can.
- **A `reason` string from the guest.** Free text cannot be translated or linked to the setting.
- **A checksum rule per provider.** Four rules drift; the unhashed and host checks belong beside the grant.
