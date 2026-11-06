# Plugin assets are served by the daemon, not read by the webview

*Applies to: [Front-ends](../architecture/frontends.md#desktop--hestia-desktop)*

A plugin theme ([0091](0091-plugins-are-wasm-components.md)) is a stylesheet
plus the fonts and images it names. The webview has to load those files from
somewhere, and they are untrusted.

**The `hestia-plugin` scheme.** The desktop registers it beside `hestia-image`
(`crates/desktop/src/assets.rs`), and the handler turns a request into a
`plugin.asset` call over the shared bridge client: `{plugin, path}` in,
`{mime, bytes}` (base64) out. The response carries that mime,
`Cache-Control: no-cache` (a plugin under development changes under the page),
`nosniff`, and for SVG the same `script-src 'none'` header the image proxy
sends. `Access-Control-Allow-Origin: *` is there because a stylesheet's
`@font-face` fetches in CORS mode. The bridge stays generic: this is shell
plumbing, not a per-feature Tauri command ([0046](0046-desktop-bridge-is-one-generic-command.md)).

**The daemon decides, in one place** (`Plugins::asset`):

- the plugin must be installed and enabled;
- the path is relative, free of `..`, and under `assets/` or `themes/`;
- the resolved file, after canonicalising, still lies beneath that directory, so
  a link inside the package cannot reach `plugin.toml` or anything outside it;
- the extension is in a fixed table (css, woff2, woff, ttf, otf, png, jpg, jpeg,
  gif, webp, avif, svg, json) and the mime comes from it, never from the file;
- at most 8 MiB. Base64 makes that about 10.7 MiB on the wire, under the 16 MiB
  `MAX_FRAME` of `crates/ipc`.

The channel names no permission (`PLUGIN = None`, listed in `DENIED`): it is the
shell's, not something a plugin calls ([0092](0092-a-contract-names-the-permission-a-plugin-needs.md)).

**Why not the asset protocol reading `plugins/<id>/package/`.** The icon and
screenshot loaders already do that for the app's own files, but it would put
the enabled check, the package-root check and the size cap in the webview's
scope config, where a disabled plugin's files stay readable and a link in a
package is followed by the protocol. Going through the daemon keeps all three
beside the code that owns the plugin's state.

**Why not the app origin.** Serving plugin CSS from `tauri://localhost` would
let it share an origin with the app: same-origin fetches of app resources, and
a CSP that cannot tell the app's stylesheets from a plugin's (R-07). A separate
scheme gets its own origin and its own CSP entries ([0093](0093-the-desktop-runs-under-a-csp.md)).

**Both URL forms.** WebView2 serves a custom scheme as
`http://hestia-plugin.localhost/…` and the others as `hestia-plugin://localhost/…`
(R-25). `pluginAsset()` in `frontend/src/lib/plugin-asset.ts` builds the URL in
the platform's form from `convertFileSrc`, keeping the slashes real so a
stylesheet's relative `url()` resolves beside it. The handler reads
`/<id>/<path>` in either form, and also `hestia-plugin://<id>/<path>`.

Rejected: a size cap only in the handler (the CLI and tests would bypass it);
streaming large files (nothing a theme needs is near the cap).
