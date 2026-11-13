# An unhashed download is declared, and the user approves the host

*Applies to: [Plugins](../architecture/plugins.md#provider-surfaces)*

> Superseded in part by [0117](0117-a-grant-is-permission-to-scope.md): the hosts are the `unhashed` scope of `network:connect` (beside its `hosts`), not top-level `network` and `unhashed` lists, and `Grant.unhashed` is `Grant::unhashed()`.

A plugin flavor's artifacts are vetted before the host downloads them: a granted host, a safe filename and a checksum
the download is verified against. Some upstreams publish nothing to check a file against. Fabric's server launcher is
generated per request by its meta service, and two of the loader's own libraries carry no hash in its profile. Core
named such files with no checksum and downloaded them anyway; a plugin that did the same would have no way to tell the
host it meant to.

**The profile says so, and the manifest allows the host.** `Artifact` gains `unhashed`, a flag a profile sets instead of
omitting its checksum silently. The manifest lists the hosts that may serve such a file in `unhashed`, each also in
`network`, and `vet` accepts an unhashed artifact only when its URL host is one of them. A missing checksum without the
flag is refused, and so is the flag on a host not listed.

**The list is part of the grant.** `Grant.unhashed` is approved at install like `network`, shown by the CLI and the
desktop under its own heading, and counted by `Grant::widening_over`, so an update that adds a host asks again.

Rejected:

- **Letting a missing checksum mean "unhashed".** A flavor that forgot a hash would download unverified code without
  anyone having decided so.
- **Having the guest download the file and hash it.** That trusts whatever the server returned on that first fetch while
  appearing to verify it.
- **A permission.** The host is a fact about one plugin's network list, not a capability class; the grant already lists
  such facts.
