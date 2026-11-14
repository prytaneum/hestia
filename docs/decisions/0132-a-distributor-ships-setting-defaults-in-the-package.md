# A distributor ships a setting's default inside the package

*Applies to: [Plugins](../architecture/plugins.md), [the manifest](../plugins/manifest.md#settings), [publishing](../plugins/publishing.md)*

The CurseForge source needs an API key. As a core feature it had one baked into official builds
(`HESTIA_CURSEFORGE_API_KEY` at compile time), with the user's own key as the override. As a plugin it declares the key
as a required setting, and a plugin build has no place for a build-time secret: the manifest is committed, and a
`secret` setting has no default. Every user of the official plugin would have had to obtain a key first.

**`pack` and `publish` read a value per declared setting from the environment.** The variable is
`HESTIA_SETTING_<ID>_<KEY>`, the plugin id and the setting key upper-cased with every other character as `_`
(`HESTIA_SETTING_CURSEFORGE_API_KEY`). A value that is set is parsed as the setting's kind, checked exactly as a value
the user stores, and written into the package as `defaults.json`, a flat object by key. A value the setting refuses
fails the pack with an error that names the variable and never the value. Nothing is written when no variable is set,
so a package built without them is the same package as before.

**The host reads it below the user's value.** A setting resolves to what the user stored, else the package's
`defaults.json`, else the manifest's `default`. A source that requires the setting is ready when any of the three fills
it. The file is read from the installed package on every lookup; an entry for an undeclared key, or one its setting
would refuse, is dropped. It is never listed: `plugin.settings.get` reports a secret as set only when the user set it,
and shows no distributed value in clear.

**It is a default, not a secret.** The value sits in the package, which the signature covers and the user can unzip,
exactly as the baked key sat in the binary. It keeps a key out of the repository and the logs; it does not hide it
from whoever installs the plugin.

**Any declared setting may take one.** No manifest key opts in: what a distributor may supply is bounded by what the
manifest declares and by each setting's own checks.

**Rejected.** A value handed over by the Hestia build to official plugins only: a third-party repository has the same
need. A value in the signed index: a bundled or offline-installed plugin would have none until the index was fetched.
A manifest `default` on a `secret`: the manifest is committed and is shown to every client.
