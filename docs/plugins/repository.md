# A plugin repository

Every plugin repository has this one shape, whether it holds one plugin or twenty. Hestia never adapts to a
repository; `hestia plugin check` enforces the shape, and `pack` and `publish` run it first. The decision behind it is
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
├── revoked.toml        [[revoked]] id, versions, reason   (optional)
├── .github/workflows/  the shared publish workflow (GitLab/Forgejo: the equivalent)
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
next = "RWR..."                        # optional spare key, kept offline; see Keys and signatures
```

- The directory name equals the manifest `id`.
- A theme-only plugin keeps `plugin.toml` and `themes/` and has no `Cargo.toml`. Cargo refuses a `plugins/*` match
  without one, so it **must** be listed in `exclude`; a plugin with a `Cargo.toml` must not be.

## The listing

The publish workflow commits the listing and the packages to an orphan `repo` branch of the repository itself, and
nothing else writes there ([0111](../decisions/0111-a-repository-publishes-to-its-own-branch.md)):

```
repo (orphan branch)
├── plugins.json + plugins.json.minisig
└── packages/<id>-<version>.hplugin + .minisig
```

The index is read by raw URL, `https://raw.githubusercontent.com/<owner>/<repo>/repo/plugins.json` on GitHub, or from
any https address that serves the same tree. It is signed (see [Keys and signatures](#keys-and-signatures)).

```
{ schemaVersion, version, expires, repository, commit, name, label, website, key, next?,
  plugins: [{ id, name, description, icon?, path, version, permissions, commit,
              builds: [{ version, api?, file, sha256 }] }],
  revoked: [{ id, versions, reason }] }
```

- `plugins` is the source as the default branch declares it; `builds` are the packages on the branch.
- `file` is a path relative to the index URL (`packages/<id>-<version>.hplugin`), so a mirror serves the same tree.
- `api` per build is the caret requirement `pack` wrote; the newest build the host's API satisfies is the one installed.
- `version` is the signing time in unix seconds and `expires` is `version` plus 30 days. `repository` and `commit`
  trace the listing to its source.
- A plugin whose `version` has no build is **unreleased**: listed, not installable.
- `revoked` comes from `revoked.toml` on the default branch.

## Builds

A build happens when its author bumps the plugin's `version` on the default branch: the publish workflow packs and
signs every plugin whose version has no package on the branch yet, then regenerates the index. There is no tag. A
published package is never rebuilt or replaced: a version already on the branch with another sha256 is refused. The
same run, weekly, re-signs the index so `expires` never lapses.

The workflow, and the same steps for another host, are in [publishing.md](publishing.md).

## Adding a repository

Users add a repository by `owner/repo` or `github.com/owner/repo` (read at the `repo` branch URL above) or by any https
URL of a `plugins.json`, which is how a repository on GitLab, Forgejo or a static host is added. Hestia fetches the
index and its `<url>.minisig` (https only, every redirect https, 8 MiB at most), verifies the signature against the key
the index declares and shows the repository's name, label, website, **key id** and plugin count. Nothing is saved until
the user agrees; saving pins `key` and `next`. The list lives in `<data home>/plugins/repositories.json`, the last good
index of each beside the other catalogue caches.

- A refresh checks the pinned keys, never one the index merely declares. A bad signature, a lower `version` than the
  cached one or a past `expires` is refused and the cached index stays; an index signed by the pinned `next` verifies
  and moves the pins to what it declares. An unreachable repository (404, offline) keeps its cached index and warns.
  Only adding treats a missing index as "not a Hestia repository".
- An expired index is stale: no new installs from it until a fresh one is fetched; installed plugins keep running.
- The catalogue merges every repository by plugin id and key: repositories pinning the same key are mirrors and show as
  one entry naming each of them, the same id under another key is another entry. Each entry is `available` (a build of
  the latest version matches this Hestia's API), `unreleased` (the latest version has no build yet) or `incompatible`
  (its builds need another API); an older compatible build is named when there is one.
- Installing picks the newest compatible, unrevoked build, downloads it from its `file` resolved against the index URL
  (any host: the signature, not the host, proves the origin), checks its sha256 and its `.minisig` against the
  repository's key, then runs the usual review and approval. The approval uses the permissions of the package itself; an
  index that lists others is reported as a warning. The install records the signer's key id and the repository.
- On every refresh an installed plugin whose version the index revokes is disabled, with the reason as its error.
- A plugin whose signer's key no configured repository lists is **obsolete**: it keeps running and gets no updates.
  Removing a repository makes its plugins obsolete until a repository with that key is added again. A plugin installed
  from a file, folder or plain link has no signer and is never obsolete.

### Add from a link

A repository's website can offer one click to add it, with a `hestia://add-repo` link carrying the index URL:

```
hestia://add-repo?url=https%3A%2F%2Fexample.com%2Fplugins.json
```

The desktop app opens Settings > Plugins and shows the same confirmation as typing the URL does (name, label, website,
key id, plugin count, and whether it is already configured). Nothing is added until the user agrees. The `url` must be
https; any other scheme, another host or path under `hestia://`, or a missing `url` is ignored. To paste into a README
or site:

```markdown
[![Add to Hestia](https://img.shields.io/badge/Add%20to-Hestia-e8873d?style=flat-square)](hestia://add-repo?url=https%3A%2F%2Fexample.com%2Fplugins.json)
```

Percent-encode the index URL. Only the installers register the scheme (NSIS on Windows, `.deb` and `.rpm` on Linux);
the AppImage and the portable archives do not, so the link does nothing there and the URL has to be added by hand
(`hestia plugin repo add <url>`). This lives here rather than in a separate publishing page because this is the page
for repository authors.

## Keys and signatures

A repository is the minisign key it declares ([0097](../decisions/0097-trust-follows-the-repository-key.md)).

A plugin is **official** when its recorded signer is one of the official-plugins keys, a current key and an offline
spare ([0099](../decisions/0099-official-plugins-are-signed.md)); `repo add` refuses an index that declares one.

```bash
hestia plugin keygen [--out <dir>] [--unencrypted]
hestia plugin pack  --sign <secret key>
```

- `keygen` writes `hestia.key` (secret, encrypted with a password unless `--unencrypted`, for a CI secret) and
  `hestia.pub` into `--out` (the current directory), and sets `key` in `[workspace.metadata.hestia]`. It never
  overwrites a key. Keep the secret key out of the repository.
- `next` is an optional spare key. Generate it the same way, keep its secret offline, and add its public half as `next`
  before it is needed: a client trusts only what it pinned. To rotate, sign with the spare; the index then declares it as
  `key`, and a client that pinned it follows.
- `pack --sign` writes `<id>-<version>.hplugin.minisig` beside each package; the publish workflow signs the index the
  same way. Publish each `.minisig` next to its file. `--sign` refuses a secret key that is not the declared `key`.
- Keys and signatures are standard minisign text: the key is the `RW...` base64 string, a signature is the `.minisig`
  file. `check` requires the declared keys to parse.
- The **trusted comment** binds a signature to its subject and is compared exactly:

  | Signs | Trusted comment |
  |---|---|
  | a build | `hestia-build id=<id> version=<version> file=<file name>` |
  | an index | `hestia-index version=<index version> expires=<expiry>` |

- A client checks an index in this order and caches it only after the last step: the signature against the pinned
  key(s) (nothing is parsed before it verifies), the declared `key`, the `version` against the one it cached (lower is
  refused), then `expires`. A failed check never replaces the cache.
- `revoked` is honoured only for plugins signed by the repository's own key.
