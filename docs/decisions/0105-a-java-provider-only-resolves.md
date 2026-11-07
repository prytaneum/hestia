# A Java provider only resolves; the host downloads, verifies and extracts

*Applies to: [The engine](../architecture/engine.md#java-runtimes)*

Adoptium was the one hardcoded source of Java runtimes. Other distributions (Zulu, Corretto, GraalVM) should be plugins,
but a runtime is the native binary Hestia executes for every server and game, so a plugin that picks it has full access
to the computer.

**A provider answers two questions and nothing else.** `majors` lists the feature versions on offer; `resolve` maps
`{ major, os, arch }` to `{ vendor, version, url, sha256, archive }`, or `null` when there is no build for that platform.
`Java` keeps the install pipeline as it was: it downloads the URL, checks the sha256, extracts the archive and registers
the runtime. The guest never downloads or extracts.

**The host vets every build at the edge.** The URL must be https on one of the plugin's granted network hosts (the same
check content files and flavor artifacts pass), the sha256 must be 64 hex characters, the archive kind must be `tar.gz`
or `zip`, and the vendor, which names the install directory, must be a plain name. A build failing any of these is
refused whole.

**Built-in and plugin providers share one trait and one roster.** `engine::java::JavaProvider` is implemented by
Adoptium (id `adoptium`) and by the wrapper around a plugin's `java` provider (`proto::provider::java`: `majors` and
`resolve`, asked through `provider.invoke`; id `<plugin>.<id>`), registered
through the same roster as content sources and flavors. The `java.provider` setting names the default; `hestia java
install --provider <id>` overrides it for one install. An id nobody registered fails as `JavaProviderUnknown`, naming it.
The setting is not checked when set, since a plugin's provider exists only while the plugin is enabled.

**Contributing one needs `java:provide`, an unrestricted permission**, with `network:connect`, for the reason
`flavor:provide` is unrestricted ([0092](0092-a-contract-names-the-permission-a-plugin-needs.md)).

Rejected: a provider that downloads and extracts itself (the guest would write executables to disk and the checksum
would be advisory); a `java.install` that takes a bare URL (no registry, so no trust boundary to check against); and
making the provider list part of `java.releases` params (the catalogue follows the configured provider, as the install
default does).
