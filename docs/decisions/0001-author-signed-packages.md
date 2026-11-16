# A plugin is signed by its author's key, listed in a signed repository index

*Applies to: [Architecture](../architecture.md#the-kernel)*

**Decision.** A release is a `.hplugin` zip signed by the plugin author's own minisign key. A repository's index, signed
by the repository's pinned key, lists per plugin the author key and each release's digest. A release is accepted only
when the index verifies, the file matches its listed digest and the signature verifies under the listed author key; an
install pins the repository and the author key, and an update must carry the same ones. Only a key in
`trust::OFFICIAL_KEYS` is official.

**Why.** Official and community signatures can never be confused, because official is a property of a compiled-in key
and an index only names keys. One compromised author cannot sign another's plugin, and a compromised repository key can
list releases but not sign them, so it cannot move an installed plugin to a new author.

**Rejected.** Trusting a release only by the digest the repository's index lists: the repository key alone would then
vouch for every package it hosts, official or not. Signing packages with the repository key: one key would stand for
every author, so a stolen key forges any plugin of the repository.
