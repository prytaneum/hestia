# A plugin repository publishes to its own orphan branch

*Applies to: [Plugins](../architecture/plugins.md), [repository spec](../plugins/repository.md)*

A repository used to publish its index on a rolling `hestia-index` release and one release per `<id>-v<version>` tag
([0094](0094-a-plugin-repository-has-one-shape.md)). The rolling release clobbers its assets, the index carries its own
history by downloading the previous file, revocations are hand-edited into that file, and every build needs a tag.

**Each repository's CI commits its signed packages and index to an orphan `repo` branch of the same repository**, the
way Mihon and Aniyomi extension repositories do, and users add the repository by URL. Git is the state and the audit
trail; a version is published by bumping it on the default branch; nothing is downloaded to regenerate the index.
There is no central registry and no review gate: a repository keeps the one shape and its own key, and Hestia
preconfigures only the official repository.

```
repo (orphan branch, written only by the publish workflow)
├── plugins.json + plugins.json.minisig
└── packages/<id>-<version>.hplugin + .minisig
```

The index is `plugins.json` (it was `hestia-index.json`). `owner/repo` resolves to
`https://raw.githubusercontent.com/<owner>/<repo>/repo/plugins.json`; any other https URL of a `plugins.json` works
too (GitLab, Forgejo, a static host). A build's `file` is relative to the index URL, so a mirror or another host serves
the same tree. A published package is never rebuilt or replaced.

**Freshness and rollback, for every repository.** The index `version` is the signing time in unix seconds, monotonic
with no stored state, and `expires` is `version` + 30 days. The trusted comment is `hestia-index version=<v>
expires=<e>`, so neither can be replayed or stripped. A client verifies the signature against the pinned key before
parsing anything, then checks the version against the cached one and the expiry, and only then caches: an index that
fails a check never replaces the cache (the order `tough` and `go-tuf` once got wrong). An expired index is stale: new
installs and updates from it stop with a typed error until a fresh one is fetched, and installed plugins keep running.
The publish workflow re-signs weekly so `expires` never lapses; GitHub disables a public repository's schedules after
60 days without activity, so the official repository is also watched from Hestia's CI.

**Rotation.** A repository may declare an optional spare `next` key beside `key`. Both are pinned when the repository
is added, and an index signed by the spare verifies; the pins then move to what that index declares. This is how a
repository rotates without users re-adding it. The same holds for the official plugins key: a current key and an
offline spare are compiled into the binary ([0099](0099-official-plugins-are-signed.md)); to rotate, sign with the
spare, then release a build that lists it first and adds a new spare.

**Revocation** comes from the index and is enforced on refresh: an installed plugin whose version the repository
revokes is disabled with the reason shown. The rule of [0097](0097-trust-follows-the-repository-key.md) is unchanged: a
repository revokes only what its own key signed.

**Supersedes** the listing paragraph of 0094 (the rolling release and tags). The shape of the repository, trust by key,
pinning and removal leaving plugins running still hold.

**Rejected.** A central registry with a review gate (one more repository and key for a model Mihon shows is not
needed); keeping the rolling release (clobbered assets, a downloaded previous index); an index without an expiry (a
frozen copy is a freeze attack on any repository).
