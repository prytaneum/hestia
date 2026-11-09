# Publishing a plugin repository

A repository is published by one command, `hestia plugin publish`, run by one workflow on every push to the default
branch and weekly. It commits the signed packages and the index to an orphan `repo` branch of the repository itself, and
users add the repository by its URL ([0111](../decisions/0111-a-repository-publishes-to-its-own-branch.md)). The shape
the command expects is in [repository.md](repository.md).

## On GitHub

Start from [prytaneum/plugin-template](https://github.com/prytaneum/plugin-template) (the output of
`hestia plugin new`): one example plugin, a pull request check, and one workflow that calls Hestia's reusable one.

```yaml
# .github/workflows/publish.yml: on push to the default branch, and weekly
jobs:
  publish:
    permissions: { contents: write }
    uses: prytaneum/hestia/.github/workflows/plugin-publish.yml@<tag>
    with: { hestia-ref: refs/tags/<tag> }
    secrets: { HESTIA_SIGNING_KEY: "${{ secrets.HESTIA_SIGNING_KEY }}" }
```

- `<tag>` is the Hestia release the workflow and the `hestia` CLI come from. `uses:` cannot hold an expression, so the
  ref is written twice. `hestia-ref` is read by `cargo install --rev`, which takes a commit or a full ref.
- The secret is passed by name: `secrets: inherit` only reaches workflows of the same organisation.
- The caller job grants `contents: write`; a called workflow can only narrow its caller's permissions.
- `HESTIA_SIGNING_KEY` holds the file `hestia plugin keygen --unencrypted` wrote (`gh secret set HESTIA_SIGNING_KEY <
  hestia.key`). With the secret unset the workflow warns and publishes nothing.
- There is no tag to push: bump `version` in `plugins/<id>/plugin.toml` on the default branch to release it.

**Publish** (push to the default branch, and weekly):

1. Check out the default branch and the `repo` branch (created orphan on the first run).
2. `hestia plugin publish --out <repo checkout> --sign <key>`: runs `check`, packs and signs every plugin whose version
   has no package in `packages/` yet, refuses a version already there with another sha256, then regenerates
   `plugins.json` from `packages/`, the sources and `revoked.toml`, with `expires` set 30 days out, and signs it.
3. Commit the result to the `repo` branch as a bot and push.

The weekly run changes nothing but the signature and `expires`. GitHub disables a public repository's scheduled
workflows after 60 days without activity, so a repository nobody touches goes stale: its index expires and new installs
stop until the workflow runs again.

## On any other host

Do the same from your CI, with the `hestia` CLI and the Rust `wasm32-wasip2` target installed: run `hestia plugin
publish --out <dir> --sign hestia.key` over a checkout of the branch, then serve `<dir>` over https so that
`plugins.json` and `plugins.json.minisig` sit side by side and each package is at its `file` path under them. Users add
the repository by the URL of `plugins.json` (`hestia plugin repo add <url>`); Hestia fetches `<url>.minisig` beside it.

Keep the secret key out of the repository and write it to a file only for the job.
