# Official plugins are the ones signed by Hestia's own key

*Applies to: [Front-ends](../architecture/frontends.md), [repository spec](../plugins/repository.md)*

Official plugins replace core features and may register bare provider ids, so Hestia must know which key is its own. A
signature alone is not enough: [0097](0097-trust-follows-the-repository-key.md) makes every repository sign with its own
key.

**"Official" means exactly: signed by the official-plugins key.** That is a dedicated minisign key held only by the
plugins repository's CI. It sits beside the self-update key in `common::app` (`OFFICIAL_PLUGIN_PUBKEYS`) but is a
different key in a different format, standard `RW…` rather than tauri-wrapped, so it cannot sign an app update and an
update key cannot make a plugin official.

**The set ships empty.** No official key exists yet, so nothing is official and the check fails closed. The real key
goes into `OFFICIAL_PLUGIN_PUBKEYS` once generated. A debug build reads `HESTIA_OFFICIAL_PLUGIN_PUBKEY` instead, for
tests and local runs; release builds ignore it.

**The flag is derived, never stored.** `PluginInfo.official` is true when the record's `signer` (the key id recorded
from the verified package) is the key id of an official key, computed on every read, so an install and an update both
reach it through the signature check. An id, URL, label or store name proves nothing.

**Only the official store may pin the key.** `plugin.repo.add` refuses a new repository whose index declares an official
key (`RepoError::OfficialKey`, answered as `repository_refused`). The official store is added by Hestia itself.

**Bare provider ids are for official plugins.** `Roster::register` takes the owner's `official` flag: an official owner
may register an id without the `<owner>.` prefix, anyone else is refused as before.

Rejected: a flag inside the manifest or the index (self-asserted), and trusting the official store's address.

**Amended 2026-11-10.** The official repository, `prytaneum/plugins`, is preconfigured: seeded into `repositories.json`
at first start and whenever it is missing, pinned to the official keys, and not removable. Seeding is the only path that
pins them; `repo.add` still refuses an index declaring one. Rejected: a special case in verification (the pin alone
does the job) and letting the user remove it (a user without it could not install official plugins by bare id).
