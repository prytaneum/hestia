# Publishing a plugin repository

One command, `hestia plugin publish`, run by one workflow on every push to the default branch and weekly, commits the
signed packages and the index to an orphan `repo` branch of the repository itself. Users add the repository by its URL.
The shape it expects is in [repository.md](repository.md).

## On GitHub

Start from [prytaneum/plugin-template](https://github.com/prytaneum/plugin-template) (the output of
`hestia plugin new`): an example plugin, a pull request check and one workflow that calls Hestia's reusable one.

```yaml
# .github/workflows/publish.yml: on push to the default branch, and weekly
jobs:
  publish:
    permissions: { contents: write }
    uses: prytaneum/hestia/.github/workflows/plugin-publish.yml@<tag>
    with: { hestia-ref: refs/tags/<tag> }
    secrets: { HESTIA_SIGNING_KEY: "${{ secrets.HESTIA_SIGNING_KEY }}" }
```

- `<tag>` is the Hestia release the workflow and the `hestia` CLI come from; it is written twice because `uses:` cannot
  hold an expression.
- Pass the secret by name; `secrets: inherit` only reaches workflows of the same organisation.
- `HESTIA_SIGNING_KEY` holds the file `hestia plugin keygen --unencrypted` wrote
  (`gh secret set HESTIA_SIGNING_KEY < hestia.key`). With it unset the workflow warns and publishes nothing. Generate
  the key with `hestia plugin keygen`: an unencrypted key from the `minisign` 0.12 CLI fails its checksum.
- There is no tag to push. Bump `version` in `plugins/<id>/plugin.toml` on the default branch to release it.

Each run:

1. Checks out the default branch and the `repo` branch (created orphan on the first run).
2. Runs `hestia plugin publish --out <repo checkout> --sign <key>`: `check`, then packs and signs every valid plugin
   whose version has no package in `packages/` yet, refuses a version already there with another sha256, regenerates
   `plugins.json` from the packages, sources and `revoked.toml` with `expires` 30 days out, and signs it. An invalid
   plugin gets no new build, keeps its listed builds, and fails the run once the index is written. Packing refuses a
   component built against another WIT, such as a stale `Cargo.lock`; fix it with `cargo update -p hestia-sdk`.
3. Commits the result to the `repo` branch as a bot and pushes.

`raw.githubusercontent.com` caches each file for about five minutes, so a client may briefly read a new `plugins.json`
with the old signature and refuse it. It keeps its last good copy and the next refresh succeeds. The weekly run only
re-signs and moves `expires`; GitHub disables scheduled workflows of a public repository after 60 days without
activity, after which the index expires and new installs stop until the workflow runs again.

## Setting defaults

A setting your users should not have to fill in, such as an API key your project holds, takes a default at pack time.
Set `HESTIA_SETTING_<ID>_<KEY>` (plugin id and setting key in upper case, other characters as `_`:
`HESTIA_SETTING_CURSEFORGE_API_KEY`) in the environment of `hestia plugin pack` or `publish`. On GitHub, put one
`HESTIA_SETTING_<ID>_<KEY>=value` line per setting in a repository secret named `HESTIA_SETTING_DEFAULTS` and pass it to
the workflow beside `HESTIA_SIGNING_KEY`; lines naming other variables are skipped. The value is checked like a user's
and written into the package as `defaults.json`; the user's own value wins. Anyone who installs the plugin can read it,
so it keeps a key out of your repository but does not keep it secret.

## On any other host

From your CI, with the `hestia` CLI and the `wasm32-wasip2` target installed, run
`hestia plugin publish --out <dir> --sign hestia.key` over a checkout of the branch, and serve `<dir>` over https so
`plugins.json` and `plugins.json.minisig` sit side by side and each package is at its `file` path. Users add it with
`hestia plugin repo add <url of plugins.json>`. Write the secret key to a file only for the job.

## Updates

Hestia checks for newer builds a minute after the daemon starts and every six hours. What reaches users:

- A plugin installed from a repository (or a link signed by a known key) is checked against every configured repository
  that pins its signing key. The newest build this Hestia's API can run is offered; a newer version with no build yet is
  not reported.
- An unsigned link is checked only on request. A file, folder, dev folder, or obsolete plugin is never checked.
- An update is verified against the key that signed the installed plugin. If it asks for more than was approved
  (`widened`), nothing changes until the user approves that grant; an unattended update never widens one.
- `data/` is kept. If the new version fails to initialise, or traps or is disabled within its first minute, the previous
  package is restored.
