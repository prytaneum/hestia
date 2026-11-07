# Publishing a plugin repository

A repository is published by three commands: `check`, `pack` and `index`. On GitHub, two reusable workflows run them; on
GitLab, Forgejo or any static host, run the same commands from your CI and serve the files over https. The shape the
commands expect is in [repository.md](repository.md).

## On GitHub

Start from [prytaneum/plugin-template](https://github.com/prytaneum/plugin-template) (the output of
`hestia plugin new`): one example plugin, a pull request check, and two workflows that call Hestia's reusable ones.

```yaml
# .github/workflows/index.yml: on push to the default branch
jobs:
  index:
    permissions: { contents: write }
    uses: prytaneum/hestia/.github/workflows/plugin-index.yml@<tag>
    with: { hestia-ref: refs/tags/<tag> }
    secrets: { HESTIA_SIGNING_KEY: "${{ secrets.HESTIA_SIGNING_KEY }}" }
```

`plugin-release.yml` is called the same way on a `<id>-v<version>` tag push.

- `<tag>` is the Hestia release the workflow and the `hestia` CLI come from. `uses:` cannot hold an expression, so the
  ref is written twice. `hestia-ref` is read by `cargo install --rev`, which takes a commit or a full ref
  (`refs/heads/<branch>`, `refs/tags/<tag>`), never a bare branch name.
- The secret is passed by name: `secrets: inherit` only reaches workflows of the same organisation.
- The caller job grants `contents: write`; a called workflow can only narrow its caller's permissions.
- `HESTIA_SIGNING_KEY` holds the file `hestia plugin keygen --unencrypted` wrote (`gh secret set HESTIA_SIGNING_KEY <
  hestia.key`). `plugin-index.yml` skips publishing, with a warning, while the secret is unset; `plugin-release.yml`
  fails.

**Index** (every push to the default branch). No builds:

1. `hestia plugin check`
2. `hestia plugin index --previous <current index> --sign <key>`; the first run has no previous index (the download
   answers 404), so `--previous` is left out.
3. Replace `hestia-index.json` and `hestia-index.json.minisig` on the `hestia-index` release, creating it if missing.

**Release** (a `<id>-v<version>` tag):

1. Refuse a tag whose version differs from `plugins/<id>/plugin.toml`.
2. `cargo test -p <id>` (a theme-only plugin has no crate and skips it), then `hestia plugin pack <id> --sign <key>`.
3. Attach `<id>-<version>.hplugin` and its `.minisig` to the tag's release.
4. Run step 2 and 3 of the index with `--add dist/<id>-<version>.hplugin`, from the default branch's sources.

## On any other host

Do the same from your CI, with the `hestia` CLI and the Rust `wasm32-wasip2` target installed:

```bash
hestia plugin check
hestia plugin pack <id> --sign hestia.key                 # on a release; writes dist/<id>-<version>.hplugin(.minisig)
hestia plugin index --previous <current index url> --repository <https url of the source> \
  --add dist/<id>-<version>.hplugin --sign hestia.key     # --add only on a release
```

Then upload `dist/hestia-index.json` and `dist/hestia-index.json.minisig` to one fixed https URL, and every
`.hplugin` with its `.minisig` where the index says they are. `index` derives each package's download URL in the GitHub
layout, `<repository>/releases/download/<id>-v<version>/<id>-<version>.hplugin`; a host with another layout serves
packages at those paths. Users add the repository by the URL of `hestia-index.json`
(`hestia plugin repo add <url>`); Hestia fetches `<url>.minisig` beside it.

Keep the secret key out of the repository and write it to a file only for the job.
