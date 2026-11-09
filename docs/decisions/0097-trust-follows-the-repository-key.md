# Trust follows the repository's key, not its address

*Applies to: [Front-ends](../architecture/frontends.md), [repository spec](../plugins/repository.md)*

A plugin runs with the permissions its user approved, so where a build came from matters. A sha256 recorded at install
proves nothing about who built an update, and the same plugin id can appear in unrelated repositories.

**Each repository declares one minisign public key** in `[workspace.metadata.hestia]`, carried in its index. Adding a
repository pins that key; the index and every build carry a detached `.minisig` made with the matching secret key. Keys
and signatures are the standard minisign text formats, so the `minisign` tool verifies them too. The key id (the
16-hex-digit key number minisign prints) is what the user is shown wherever they are asked to trust a repository.

**A signature is bound to what it signs.** The signed trusted comment is `hestia-build id=<id> version=<version>
file=<file>` for a build and `hestia-index version=<n>` for an index, compared as a whole string against what the
verifier expects. A valid signature therefore cannot be replayed onto another plugin, version, file name or an older
index. `proto::repository` holds both formats, so signer and verifier cannot drift.

**Pinning and rollback.** (Freshness and the spare key are added by [0111](0111-a-repository-publishes-to-its-own-branch.md).) A refresh is refused when the index is signed by another key, declares another `key`, fails
verification, or has a `version` lower than the cached one; the cached index is kept. Nothing is parsed before the
signature verifies. A key rotation is remove + re-add, shown to the user as a new key id.

**Removing a repository leaves its plugins enabled.** The user approved them at install; they show as obsolete and
take no updates until a repository with the signer's key is added again. Updates come only from repositories pinning the
key that signed the installed plugin.

**Revocation is scoped to the signer.** An index's `revoked` list is honoured only for plugins whose recorded signer is
that index's own key: a repository cannot revoke what it did not sign.

**Signing tooling is CLI-only.** `hestia plugin keygen` and `--sign` use the `minisign` crate in `cli`; the engine links
only `minisign-verify`, so the daemon can verify but never sign. Self-update keeps its own compiled-in keys and the
tauri wrapped format.

**Rejected.** Trusting an id, URL, host or repository name as proof of origin (all are chosen by whoever serves the
bytes); a second signature library in the engine; signing keys held by the daemon; trust on a sha256 pin alone.
