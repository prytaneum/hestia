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
key = "RWQ..."                         # minisign public key (a placeholder until `hestia plugin keygen`)
```

- The directory name equals the manifest `id`.
- A theme-only plugin keeps `plugin.toml` and `themes/` and has no `Cargo.toml`. Cargo refuses a `plugins/*` match
  without one, so it **must** be listed in `exclude`; a plugin with a `Cargo.toml` must not be.

## The listing

CI regenerates `hestia-index.json` from `[workspace.metadata.hestia]` and `plugins/*/plugin.toml` on every push to the
default branch and publishes it at one fixed https URL. On GitHub that is a rolling release tagged `hestia-index`:
`<repository>/releases/download/hestia-index/hestia-index.json`. Users add a repository by that URL. The index is not
signed yet; signing arrives with `hestia plugin keygen`.

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
