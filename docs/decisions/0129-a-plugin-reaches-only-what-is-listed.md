# A plugin reaches a setting, a host or a download only by an entry that names it

*Applies to: [Plugins](../architecture/plugins.md#permissions), [plugin permissions](../plugins/permissions.md)*

A review of the plugin API found four places where a permission opened more than it said. Each was a rule that listed
what was refused, or that checked once where it should have checked every time. They are fixed together because they
share one correction: what a plugin may reach is a list of what it may reach, and anything not on the list is closed.

**A setting is reachable only if the table lists it.** `config:write` was a denylist of five keys, so a plugin holding it
could turn off auto-update, repoint self-update or force offline mode, and every key added later was open by default.
`proto::config::PLUGIN_ACCESS` now marks every setting `Closed`, `Read` or `ReadWrite` for plugins. A key missing from the
table is closed, and an engine test fails until a new setting is listed. `config.get` of a closed key answers
`ConfigKeyDenied` (`forbidden`), a parent object or `config.list` is filtered to the keys a plugin may read, and setting
a parent needs every key under it to be `ReadWrite`. Nothing is `ReadWrite` yet, so `config:write` stays unrestricted
([0092](0092-a-contract-names-the-permission-a-plugin-needs.md)) until a key is handed over on purpose. Keys match in
their canonical form, so `defaults.jvmArgs` is `defaults.jvm-args`.

**A wildcard host matches one label and never a public suffix.** `*.domain` matched names at any depth, and nothing
stopped `*.github.io` or `*.co.uk`, which grant every site registered beneath them. `network_matches` now accepts exactly
one label below the domain, and manifest validation refuses a wildcard whose domain is itself a public suffix
(`PublicSuffixWildcard`), using the `psl` crate behind `proto`'s default `suffix` feature. The SDK builds `proto` without
default features, so a plugin does not carry the list; only the host and the CLI check it.

**A URL a plugin names is fetched under that plugin's policy, on every hop.** The engine followed redirects and resolved
names for a content file, a flavor artifact or a Java build with its ordinary client, so a granted host that redirected
to a loopback or internal address was a blind request from the user's machine, and with `unhashed` the body was
unverified besides. Each plugin-backed provider owns a `Policy` built from its grant (`plugins/vouch.rs`), and the
engine's `Downloader` fetches through it when it holds one: https only, the guarded client whose resolver refuses
addresses inside the machine, redirects walked by hand with the grant checked again at each hop, and `unhashed` held
on every hop. Provenance travels with the provider, never with the URL, so a core download keeps the client it had.
[0122](0122-a-source-declares-its-setting-and-one-vet-checks-every-download.md) makes one `vet_artifact` decide whether a
file may be fetched; this is how the fetch itself is held to the grant.

**A program pin does not move without approval.** A `PATH` program was resolved again on every update, an unattended one
included, and the pin was not in the grant digest. [0120](0120-a-programs-identity-is-separate-from-its-invocations.md)
records the fix: the pin is a path and a hash, it is in the digest, and a changed pin holds an update for the user.

Rejected:

- **Extending the denylist.** It would have closed the five keys named in the review and stayed open for the next one.
- **A permission per setting.** The approval prompt would list launcher internals; one table the engine tests against
  the settings schema keeps the answer in one place.
- **Matching a wildcard at any depth and vetting the registrant.** A suffix list is the only authority on where
  registration starts, and one level is what every wildcard in the plugins written so far needed.
- **Strict redirect handling in the core downloader for everyone.** Core hosts are the launcher's own and Mojang's; the
  rule is about what a third party names, so only a downloader holding a plugin's policy applies it.
