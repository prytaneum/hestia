# Hestia plugins

A [Hestia](https://github.com/prytaneum/hestia) plugin repository: one folder per plugin under `plugins/`, built,
signed and published by the workflows in `.github/workflows/`.

## Set it up

1. Edit `name`, `label` and `website` under `[workspace.metadata.hestia]` in `Cargo.toml`.
2. Create the repository's signing key. `--unencrypted` is what lets CI use it without a password:

   ```bash
   hestia plugin keygen --unencrypted
   gh secret set HESTIA_SIGNING_KEY < hestia.key
   ```

   `keygen` writes `hestia.key` (kept out of git) and declares the public key in `Cargo.toml`; commit that change.
   Until the secret exists the `index` workflow skips publishing. Losing the key means readers must remove and re-add
   the repository.
3. Push to `main`. The `index` workflow publishes `hestia-index.json` on the `hestia-index` release, and
   `hestia plugin repo add <owner>/<repo>` lists your plugins as unreleased.

## Release a plugin

Set the new `version` in `plugins/<id>/plugin.toml` and `Cargo.toml`, merge it, then push the tag:

```bash
git tag <id>-v<version> && git push origin <id>-v<version>
```

The `release` workflow tests and packs only that plugin, attaches `<id>-<version>.hplugin` and its signature to the
tag's release and refreshes the index. Users can then install it.

## More plugins

```bash
hestia plugin new <id>     # add plugins/<id>
hestia plugin dev          # run a plugin from its folder and reload it on every rebuild
hestia plugin pack         # build dist/<id>-<version>.hplugin locally
```

The workflows call Hestia's shared ones at the ref named in each file (`feature/plugins` until Hestia releases the
plugin system); change it in `.github/workflows/` to move to a release. The shape is described in the
[repository guide](https://github.com/prytaneum/hestia/blob/main/docs/plugins/repository.md).
