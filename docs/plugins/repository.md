# A plugin repository

Every plugin repository has this one shape, whether it holds one plugin or twenty. Hestia never adapts to a repository;
`hestia plugin check` enforces the shape, and `pack` and `publish` run it first. Each plugin is judged on its own: an
invalid one is reported while the others are still checked, packed and published.

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

```toml
[workspace]
members = ["plugins/*"]
exclude = ["plugins/dark-themes"]     # theme-only plugins

[workspace.metadata.hestia]
name = "Acme plugins"                  # the store's name
label = "ACME"                         # the store badge, 1 to 4 characters
website = "https://acme.example"       # https
key = "RWQ..."                         # minisign public key, written by `hestia plugin keygen`
next = "RWR..."                        # optional spare key, kept offline
```

- The directory name equals the manifest `id`.
- A theme-only plugin has no `Cargo.toml`, so cargo refuses a `plugins/*` match for it: it **must** be in `exclude`, and
  a plugin with a `Cargo.toml` must not be.

## The listing

The publish workflow ([publishing.md](publishing.md)) commits the listing and packages to an orphan `repo` branch:

```
repo (orphan branch)
├── plugins.json + plugins.json.minisig
└── packages/<id>-<version>.hplugin + .minisig
```

The index is read from `https://raw.githubusercontent.com/<owner>/<repo>/repo/plugins.json` on GitHub, or any https
address serving the same tree:

```
{ schemaVersion, version, expires, repository, commit, name, label, website, key, next?,
  plugins: [{ id, name, description, icon?, path, version, permissions, commit,
              builds: [{ version, api?, file, sha256 }] }],
  revoked: [{ id, versions, reason }] }
```

- `plugins` is the source as the default branch declares it; `builds` are the packages on the branch. `file` is
  relative to the index URL, so a mirror serves the same tree.
- `api` per build is the caret requirement `pack` wrote; the newest build the host's API satisfies is installed.
- `version` is the signing time in unix seconds and `expires` is `version` plus 30 days.
- A plugin whose `version` has no build is **unreleased**: listed, not installable. `revoked` comes from `revoked.toml`.

A build happens when its author bumps the plugin's `version` on the default branch. A published package is never rebuilt
or replaced: a version already on the branch with another sha256 is refused.

## Adding a repository

Users add one by `owner/repo`, `github.com/owner/repo`, or the https URL of a `plugins.json` (how GitLab, Forgejo or a
static host is added). Hestia fetches the index and its `.minisig` (https only, 8 MiB at most), verifies the signature
against the key the index declares and shows the name, label, website, **key id** and plugin count. Nothing is saved
until the user agrees; saving pins `key` and `next`.

- A refresh checks the pinned keys, never one the index merely declares. A bad signature, a lower `version` than cached
  or a past `expires` is refused and the cached index stays. An index signed by the pinned `next` moves the pins to what
  it declares. An unreachable repository keeps its cached index.
- An expired index is stale: no new installs from it, but installed plugins keep running.
- Repositories pinning the same key are mirrors and show as one catalogue entry; the same id under another key is
  another entry. An entry is `available`, `unreleased` or `incompatible` (its builds need another API).
- Installing picks the newest compatible, unrevoked build, checks its sha256 and `.minisig` against the repository key,
  then runs the usual review and approval on the package's own permissions. The origin is proved by the signature, not
  the host, so a `file` may live anywhere.
- An installed plugin whose version the index revokes is disabled, with the reason as its error.
- A plugin whose signer's key no configured repository lists is **obsolete**: it keeps running and gets no updates. A
  plugin installed from a file, folder or plain link has no signer and is never obsolete.

### The official repository

`prytaneum/plugins` is configured in every install, seeded on first start, pinned to the official keys compiled into the
build, and cannot be removed. It is the only repository that may pin them; a repository the user adds that declares one
is refused. A bare id (`hestia plugin add modrinth`) searches every configured repository; an id offered under several
keys asks which.

### Add from a link

A website can offer one click to add a repository with a `hestia://add-repo` link carrying the percent-encoded index
URL:

```markdown
[![Add to Hestia](https://img.shields.io/badge/Add%20to-Hestia-e8873d?style=flat-square)](hestia://add-repo?url=https%3A%2F%2Fexample.com%2Fplugins.json)
```

The desktop opens Plugins > Repositories with the same confirmation as typing the URL; nothing is added until the user
agrees. The `url` must be https; any other form is ignored. Only the installers register the scheme (NSIS, `.deb`,
`.rpm`); the AppImage and portable archives do not, so there the URL is added by hand
(`hestia plugin repo add <url>`).

## Keys and signatures

A repository is the minisign key it declares. A plugin is **official** when its recorded signer is one of the
official-plugins keys; `repo add` refuses an index that declares one.

```bash
hestia plugin keygen [--out <dir>] [--unencrypted]
hestia plugin pack  --sign <secret key>
```

- `keygen` writes `hestia.key` (secret, password-encrypted unless `--unencrypted`, for a CI secret) and `hestia.pub`
  into `--out`, and sets `key` in `[workspace.metadata.hestia]`. It never overwrites a key. Keep the secret out of the
  repository.
- `next` is an optional spare key. Generate it the same way, keep its secret offline, and add its public half as `next`
  before it is needed, since a client trusts only what it pinned. To rotate, sign with the spare: the index then declares
  it as `key`, and a client that pinned it follows.
- `pack --sign` writes `<id>-<version>.hplugin.minisig` beside each package; `publish --sign` signs each package it
  builds and the index. `--sign` refuses a secret key that is not the declared `key`. `check` requires the declared
  keys to parse.
- The signature's trusted comment binds it to its subject and is compared exactly: `hestia-build id=<id>
  version=<version> file=<file name>` for a build, `hestia-index version=<index version> expires=<expiry>` for an index.
- A client verifies the signature against the pinned keys before parsing anything, then the declared `key`, then
  `version` (lower than cached is refused), then `expires`, and caches the index only after the last step. `revoked` is
  honoured only for plugins signed by the repository's own key.
