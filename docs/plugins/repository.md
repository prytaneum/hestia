# A plugin repository

Every plugin repository has this one shape, whether it holds one plugin or twenty. Hestia never adapts to a
repository; `hestia plugin check` enforces the shape, and `pack` and `index` run it first. The decision behind it is
[0094](../decisions/0094-a-plugin-repository-has-one-shape.md).

```
<repo>/
├── Cargo.toml          workspace, members = ["plugins/*"], plus [workspace.metadata.hestia]
├── Cargo.lock
├── plugins/<id>/
│   ├── plugin.toml
│   ├── Cargo.toml
│   ├── src/
│   ├── assets/  themes/  CHANGELOG.md     (optional)
├── .github/workflows/  the shared index and release workflows (GitLab/Forgejo: the equivalent)
└── README.md
```

The workspace `Cargo.toml` and each `plugins/<id>/plugin.toml` are the whole contract; there is no other manifest.

```toml
[workspace]
members = ["plugins/*"]
exclude = ["plugins/dark-themes"]     # theme-only plugins, see below

[workspace.metadata.hestia]
name = "Acme plugins"                  # the store's name
label = "ACME"                         # the store badge, 1 to 4 characters
website = "https://acme.example"       # https
key = "RWQ..."                         # minisign public key, written by `hestia plugin keygen`
```

- The directory name equals the manifest `id`.
- A theme-only plugin keeps `plugin.toml` and `themes/` and has no `Cargo.toml`. Cargo refuses a `plugins/*` match
  without one, so it **must** be listed in `exclude`; a plugin with a `Cargo.toml` must not be.

## The listing

CI regenerates `hestia-index.json` from `[workspace.metadata.hestia]` and `plugins/*/plugin.toml` on every push to the
default branch and publishes it at one fixed https URL. On GitHub that is a rolling release tagged `hestia-index`:
`<repository>/releases/download/hestia-index/hestia-index.json`. Users add a repository by that URL. The index is signed
(see [Keys and signatures](#keys-and-signatures)).

```
{ schemaVersion, version, repository, commit, name, label, website, key,
  plugins: [{ id, name, description, icon?, path, version, permissions, commit,
              builds: [{ version, api?, url, sha256 }] }],
  revoked: [{ id, versions, reason }] }
```

- `plugins` is the source as the default branch declares it; `builds` accumulates the packages CI built.
- `api` per build is the caret requirement `pack` wrote; the newest build the host's API satisfies is the one installed.
- `version` grows by one per regeneration. `repository` and `commit` trace the listing to its source.
- A plugin whose `version` has no build is **unreleased**: listed, not installable.
- `revoked` is edited by hand in the previous index and carried over unchanged.

## Builds

A build happens on its author's demand: push the tag `<id>-v<version>`, even in a one-plugin repository. CI builds only
that plugin, attaches `<id>-<version>.hplugin` to the tag's release, then refreshes the index. A released version is
never rebuilt: re-adding the same id and version with another sha256 is refused.

## `hestia plugin index`

```bash
hestia plugin index --previous <https-url|file> --add dist/*.hplugin [--repository <https-url>]
```

Writes `dist/hestia-index.json`. `commit` is `git rev-parse HEAD`; `repository` is `--repository` or the `origin`
remote (a `git@host:owner/repo` remote becomes https). Each added package's download URL is
`<repository>/releases/download/<id>-v<version>/<id>-<version>.hplugin`, the GitHub layout. `--previous` carries its
builds and revocations over.

## Adding a repository

Users add a repository by `owner/repo` or `github.com/owner/repo` (read at the release asset above) or by any https URL
of a `hestia-index.json`, which is how a repository on GitLab, Forgejo or a static host is added. Hestia fetches the
index and its `<url>.minisig` (https only, every redirect https, 8 MiB at most), verifies the signature against the key
the index declares and shows the repository's name, label, website, **key id** and plugin count. Nothing is saved until
the user agrees; saving pins that key. The list lives in `<data home>/plugins/repositories.json`, the last good index of
each beside the other catalogue caches.

- A refresh checks the pinned key, never the one the index now declares. A changed key, a bad signature or a lower
  `version` than the cached one is refused and the cached index stays; an unreachable repository (404, offline) keeps its
  cached index and warns. Only adding treats a missing index as "not a Hestia repository".
- The catalogue merges every repository by plugin id and key: repositories pinning the same key are mirrors and show as
  one entry naming each of them, the same id under another key is another entry. Each entry is `available` (a build of
  the latest version matches this Hestia's API), `unreleased` (the latest version has no build yet) or `incompatible`
  (its builds need another API); an older compatible build is named when there is one.
- Installing picks the newest compatible, unrevoked build, downloads it from the URL the index gives (any host: the
  signature, not the host, proves the origin), checks its sha256 and its `.minisig` against the repository's key, then
  runs the usual review and approval. The approval uses the permissions of the package itself; an index that lists
  others is reported as a warning. The install records the signer's key id and the repository.
- A plugin whose signer's key no configured repository lists is **obsolete**: it keeps running and gets no updates.
  Removing a repository makes its plugins obsolete until a repository with that key is added again. A plugin installed
  from a file, folder or plain link has no signer and is never obsolete.

### Add from a link

A repository's website can offer one click to add it, with a `hestia://add-repo` link carrying the index URL:

```
hestia://add-repo?url=https%3A%2F%2Fexample.com%2Fhestia-index.json
```

The desktop app opens Settings > Plugins and shows the same confirmation as typing the URL does (name, label, website,
key id, plugin count, and whether it is already configured). Nothing is added until the user agrees. The `url` must be
https; any other scheme, another host or path under `hestia://`, or a missing `url` is ignored. To paste into a README
or site:

```markdown
[![Add to Hestia](https://img.shields.io/badge/Add%20to-Hestia-e8873d?style=flat-square)](hestia://add-repo?url=https%3A%2F%2Fexample.com%2Fhestia-index.json)
```

Percent-encode the index URL. Only the installers register the scheme (NSIS on Windows, `.deb` and `.rpm` on Linux);
the AppImage and the portable archives do not, so the link does nothing there and the URL has to be added by hand
(`hestia plugin repo add <url>`). This lives here rather than in a separate publishing page because this is the page
for repository authors.

## Keys and signatures

A repository is the minisign key it declares ([0097](../decisions/0097-trust-follows-the-repository-key.md)).

A plugin is **official** when its recorded signer is the official-plugins key ([0099](../decisions/0099-official-plugins-are-signed.md)); `repo add` refuses an index that declares that key.

```bash
hestia plugin keygen [--out <dir>] [--unencrypted]
hestia plugin index --sign <secret key> ...
hestia plugin pack  --sign <secret key>
```

- `keygen` writes `hestia.key` (secret, encrypted with a password unless `--unencrypted`, for a CI secret) and
  `hestia.pub` into `--out` (the current directory), and sets `key` in `[workspace.metadata.hestia]`. It never
  overwrites a key. Keep the secret key out of the repository.
- `index --sign` writes `hestia-index.json.minisig` beside the index; `pack --sign` writes
  `<id>-<version>.hplugin.minisig` beside each package. Publish each `.minisig` next to its file. `--sign` refuses a
  secret key that is not the declared `key`.
- Keys and signatures are standard minisign text: the key is the `RW...` base64 string, a signature is the `.minisig`
  file. `check` requires the declared key to parse.
- The **trusted comment** binds a signature to its subject and is compared exactly:

  | Signs | Trusted comment |
  |---|---|
  | a build | `hestia-build id=<id> version=<version> file=<file name>` |
  | an index | `hestia-index version=<index version>` |

- A client refuses an index signed by another key than the pinned one, declaring another `key`, or with a lower
  `version` than the one it cached.
- `revoked` is honoured only for plugins signed by the index's own key.
