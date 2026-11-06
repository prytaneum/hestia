# The desktop runs under a CSP, and style-src is left to the page

*Applies to: [Front-ends](../architecture/frontends.md#desktop--hestia-desktop)*

`security.csp` was `null`, so nothing limited what the webview could load. That
was tolerable while the only CSS was the app's own; a plugin theme
([0091](0091-plugins-are-wasm-components.md)) is untrusted CSS, and `url(...)`
or `@import` in it could reach any host.

**The policy** (`crates/desktop/tauri.conf.json`, `app.security.csp`):

```
default-src 'self';
script-src 'self';
style-src 'self' 'unsafe-inline' hestia-plugin: http://hestia-plugin.localhost;
font-src 'self' hestia-plugin: http://hestia-plugin.localhost data:;
img-src 'self' asset: http://asset.localhost hestia-plugin: http://hestia-plugin.localhost
        hestia-image: http://hestia-image.localhost data: blob:;
connect-src 'self' ipc: http://ipc.localhost asset: http://asset.localhost data:;
frame-src hestia-plugin: http://hestia-plugin.localhost
```

**Every custom scheme is listed twice.** WebView2 serves a custom scheme as
`http://<scheme>.localhost`; WebKitGTK and WKWebView use `<scheme>:`. The app
does not set `useHttpsScheme`, so the `http://` form is the Windows one.

**Images never come from a host.** There is no `https:` in `img-src` or
`style-src`. A remote image is fetched by the daemon and served over
`hestia-image` ([Remote images](../architecture/engine.md#remote-images)), so a
stylesheet's `url(https://…)` is refused by the webview. The inventory of every
remote load found no bypass: the markdown renderer, `Thumbnail`, the account
avatar (`api.mineatar.io`) and both skin texture loaders go through
`remoteImage()`; the `https://` strings left in `frontend/src` are links, form
placeholders, and the browser-only mock layer.

**Additions beyond the plan, each from a real load:**

- `connect-src 'self' asset: http://asset.localhost data:` — `readTexturePath`
  `fetch`es a dropped file through the asset protocol, and three.js's
  `GLTFLoader` `fetch`es the bundled player models (a `?url` asset, and
  `data:` buffers embedded in the glTF).
- `img-src blob:` — skin thumbnails are `URL.createObjectURL` blobs.
- `font-src data:` — the bundler may inline small `@fontsource` files.

No `worker-src` (no Web Worker) and no `'wasm-unsafe-eval'` (no WebAssembly) is
needed; add either with the first feature that uses it.

**Tauri adds nonces and hashes to `style-src`, and that would void
`'unsafe-inline'`.** Read in tauri-utils 2.10.1 / tauri 2.12.1:
`html::inject_nonce_token` puts a nonce on every `<style>` in a bundled HTML file
(`html.rs`, selector `"style"`), and `manager/mod.rs` appends `'self'` and the
nonces to `style-src` (`replace_csp_nonce`). Under CSP Level 2+ a nonce or hash in
a directive makes the browser ignore `'unsafe-inline'`, so any `<style>` created
at runtime — which has no nonce — would be blocked. The built app has such
styles: `index.html` carries a static `<style>`, and at runtime sonner
(`createElement('style')`) and CodeMirror's style-mod fallback (no
`adoptedStyleSheets`) inject elements. A React `style={{}}` prop sets CSSOM
properties and is unaffected.

So `app.security.dangerousDisableAssetCspModification` is `["style-src"]` (the list
form exists: `DisabledCspModificationKind::List`). `script-src` keeps Tauri's
hashing and stays `'self'`: the bundle has no inline script, and the only
`script` nonces Tauri adds are for `src="http…"` tags. Inline styles are the
accepted cost; the policy's protection is on what a style can *load*, which
`default-src`/`img-src`/`font-src` still bound.

**`devCsp`** is the same policy plus the Vite dev server (`http://localhost:1420`,
`ws://localhost:1420` for HMR) and `'unsafe-inline'` in `script-src` for the React
refresh preamble. It applies to debug runs only; release builds use `csp`.

Rejected: allowing `https:` in `img-src` (any theme could beacon to a host, and
the proxy already guards private addresses and size); nonces for runtime styles
(every dependency that injects CSS would need to cooperate).

The in-app click-through of every page with devtools open, in both a debug and
a release build, is pending with the owner.
