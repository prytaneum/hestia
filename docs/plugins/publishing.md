# Packing and publishing

*[← Writing a plugin](README.md)*

A release is a `.hplugin` signed by the author's own key and listed in a repository's signed index. Hestia accepts it
only when the index verifies under the key the user pinned for the repository, the file matches the index's digest and
size, and the signature verifies under the author key the index lists. See the
[decision](../decisions/0001-author-signed-packages.md) and [the trust
model](../architecture/plugins.md#trust-and-repositories).

Both keys come from `minisign -G`: one for you as the author, one for the repository.

## `hestia plugin pack`

```bash
hestia plugin pack [dir] --key <author.key> [--out <file>] [--wasm <file>]
```

Builds the component, runs the same checks as [`check`](grant.md) (it fails on a lockfile mismatch or an `@unstable`
import), writes `api = "<version>"` into `plugin.toml` when it has none (the version the component imports) and writes
`<id>-<version>.hplugin`. The package is a zip of exactly `plugin.toml`, `plugin.wasm` and `plugin.minisig`. The
signature covers `hestia-hplugin-v1`, the id, the version and the SHA-256 of the manifest and the component, never the
archive bytes.

## `hestia plugin publish`

A repository is a directory a web server serves: `index.json`, `index.json.minisig` and the packages.

```bash
hestia plugin publish tidy-0.1.0.hplugin --repository ./repo --key repository.key \
    --url https://example.com/plugins/tidy-0.1.0.hplugin --author <author public key> --name example
hestia plugin publish --repository ./repo --key repository.key --refresh
hestia plugin publish --repository ./repo --key repository.key --revoke tidy@0.1.0 --reason "leaks a token"
```

| Flag                   | For                                                                                        |
|------------------------|--------------------------------------------------------------------------------------------|
| `--url`                | the https address the package will be served at; its file name is the package's name there |
| `--author`             | the author's public key (or its file); needed the first time a plugin is published         |
| `--name`               | the repository's name, when it has no index yet                                            |
| `--host`               | a host the index lists as serving packages; the package URL's host must be one             |
| `--allow-widen`        | publish a version that asks for more than the last one did                                 |
| `--project`            | the project whose `plugin.grant` the package is held to (default `.`)                      |
| `--refresh`            | sign the index again, for a new life and a new sequence, without a release                 |
| `--revoke`, `--reason` | list a published version as revoked: clients disable it and never run it again             |

Every `publish` first verifies the index already in the directory under the repository key, so nothing planted there is
signed again, then signs a new one under the next sequence that expires in 29 days. Clients refuse an index that is past
its expiry, lives more than 30 days, is older than one they have seen or is signed for another repository, so refresh it
before it expires. Publishing a package verifies it against the author key, refuses a version already published and a
URL whose host the index does not list, and adds the release (URL, SHA-256, size). It compares the grant with the newest
published version's (whose package must still be in the directory) and refuses one that asks for more unless
`--allow-widen`.

Users add the repository with `hestia plugin repo add <id> <url> --key <repository public key>` and install with `hestia
plugin get <repo> <id>` ([the CLI reference](../cli.md#plugins)).

## Versioning

- **Your plugin's version** is yours. An update must be listed by the repository the plugin was installed through and
  signed by the same author key. It asks the user again when it widens the grant: a new required permission, scope
  entry, required package, export or schedule, or one that ticks more often. A version that fails to load is rolled
  back.
- **The API version** is the `hestia:kernel` package's (`0.1.0` today), one number for the whole API. `api` in the
  manifest is the one you built against; Hestia refuses an `api` that is not what the component imports, and an import
  from a later version or a different `0.y` line than the host serves (`0.0.z` must match exactly). Within `0.y` an
  addition is compatible: an older plugin links against a newer host. A breaking change is `0.y+1`.
- **Unstable and deprecated.** An interface marked `@unstable(feature = ...)` may change in any release; it links only
  for a dev install in developer mode, and `check`, `pack` and `publish` refuse it. A `@deprecated` interface stays for
  at least one `0.y` line; `check` warns when you import one.
