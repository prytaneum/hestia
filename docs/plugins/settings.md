# Settings

A plugin declares typed settings in its manifest (`[[settings]]`, see [manifest.md](manifest.md#settings)); the user
sets them from the app or `hestia plugin config`. The plugin reads them, and may write them too. Values live in
`plugins/<id>/settings.json`, owner-only, because one may be a secret.

## Reading

```rust
let key: Option<String> = hestia::settings::get("api-key")?;
```

`get` returns what the user set, else the manifest's default, else `None`; a secret arrives in clear, to this plugin
only. A plugin is told when the user changes a key through `Plugin::on_settings_changed(key)`.

## Writing

```rust
hestia::settings::set("token", &"s3cret")?;
hestia::settings::clear("token")?;
```

A plugin writes only its own settings, for a token it obtained through its own sign-in flow or a choice it wants to
remember. The host validates a write exactly as it validates the user's:

- a key the manifest does not declare is `Error::NotFound`;
- a value of the wrong kind, a `select` option that is not listed, or a string over 4096 bytes is `Error::Invalid`;
- `clear` (or a `null` value) removes the key, so `get` returns the default again.

A secret is stored like any other setting and never logged. The write is serialised with the user's, so neither loses
the other's key. The user's windows are told that the plugin's settings changed (a `changed` event for the `plugins`
scope naming the plugin).

The plugin's own `Plugin::on_settings_changed` does not run for its own write, so setting a key from there, or from an
event handler that a setting change leads to, cannot loop. It still runs for a change the user makes.

The calls are `settings.get` and `settings.set` in `crates/sdk/wit/settings.wit`.
