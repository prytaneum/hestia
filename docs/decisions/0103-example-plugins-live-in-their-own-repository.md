# Example plugins live in their own repository; test plugins are fixtures

*Applies to: [Cross-cutting foundations](../architecture/common.md), [Build & run](../contributing.md#build--run)*

The plugins written to show the API (`hello`, `static-index`, `tunnel-demo`, `theme-ember-night`) lived under
`examples/plugins/`, and the engine and daemon tests compiled two of them. That made one folder serve two readers: an
author learning from working code, and a test that needs a component built from this commit's SDK.

**Examples live in [prytaneum/plugin-examples](https://github.com/prytaneum/plugin-examples).** It has the plugin
repository shape ([0094](0094-a-plugin-repository-has-one-shape.md)), so it is built the way an author's own
repository is (`hestia plugin check`, `pack`) and reads as one. Until the SDK is released it tracks `hestia-sdk` from
this repository's development branch; then it pins a release tag, like `hestia plugin new`.

**A plugin a test builds is a fixture beside that test** (`crates/<crate>/tests/fixture/plugins/<id>/`, as `probe`
already was). It takes the SDK by path, so a WIT or SDK change and the fixture it breaks are fixed in one commit, and
the tests stay offline.

Rejected: keeping the examples in-tree (they then double as fixtures, and an example written for a reader gets bent
to what a test needs); and building the tests against the external repository at a pinned commit, which puts the
network in `cargo test` and breaks the tests on every SDK change until the other repository catches up.
