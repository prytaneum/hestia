# A plugin webview is an opaque-origin iframe with a mediated message bridge

*Applies to: [Plugins](../architecture/plugins.md#declarative-ui-and-themes), [Front-ends](../architecture/frontends.md#plugin-pages)*

The declarative vocabulary ([0096](0096-plugin-ui-is-a-declarative-tree.md)) cannot
express a custom chart or a map. The escape hatch is one more node,
`webview { src, height }`, that draws an HTML page from the plugin's own
`assets/` in a frame. The page is untrusted code, so the decision is what
contains it.

**The frame** (`frontend/src/features/plugins/view/webview.tsx`):

```html
<iframe sandbox="allow-scripts" src="hestia-plugin://…/assets/chart.html"
        allow="" referrerpolicy="no-referrer">
```

`allow-scripts` and nothing else. Never `allow-same-origin` (it would give the
page the origin of `hestia-plugin`, shared with every other plugin file and
fetchable by the app), and never `allow-top-navigation`, `allow-popups` or
`allow-forms`. Without `allow-same-origin` the frame has an opaque origin: it
cannot read the app's DOM, storage or cookies, and the app cannot read its.

**The page is served sandboxed too.** `crates/desktop/src/assets.rs` answers an
`.html` file with a `Content-Security-Policy` that starts with
`sandbox allow-scripts`, so the origin stays opaque even if the file is opened on
its own, and then names only the `hestia-plugin` scheme for scripts, styles,
images, fonts and `connect-src`. `default-src 'none'` and no `https:` anywhere:
the page has no network, no nested frames, no forms and no `<base>`. `'self'`
cannot be used because an opaque origin matches nothing, so the scheme is named.
The asset table gained `html` and `js` (`Plugins::asset`); a theme or image
request never needs them, and the page's own policy is what contains them.

**The bridge** (`view/bridge.ts`, `view/webview.tsx`). The frame can reach the
host only through `postMessage`:

- A `message` listener accepts an event only when
  `event.source === iframe.contentWindow`. The sandboxed frame's `event.origin`
  is the string `"null"`, shared by every opaque origin, so it identifies
  nothing; the window reference is what identifies this frame.
- The payload is validated by a pure function, `parseBridgeMessage`: a plain
  object, at most 64 KiB, copied through JSON, with a string `id` and either
  `{type: 'run', command, args}` or `{type: 'action', action, payload}`.
  Anything else is dropped without a reply.
- `run` starts `plugin.run` for the plugin that owns the page (taken from the
  view, never from the message) and replies with the command's output. A command
  that needs an entry is refused by the daemon, since the frame cannot name one.
  The run goes through the job store like any other, so its outcome is reported
  there ([0089](0089-a-job-outcome-is-reported-once.md)).
- `action` sends that plugin's own `plugin.ui.action` for the page being shown,
  and the effects apply as they do for a button. A failed action is toasted by
  the page, and the frame is told `ok: true` only because the view swallows the
  error; a frame that needs failure detail should use `run`.
- The reply goes to `contentWindow.postMessage(reply, '*')`. An opaque-origin
  frame can only be addressed with `'*'`; the window is the one this component
  created.
- The listener is removed on unmount.

`UiNode::validate` (`crates/proto/src/ui.rs`) checks `src` at the edge, when the
engine decodes a render or a `render` effect: relative, under `assets/`, no `..`,
no backslash, colon or empty segment (the same `stays_inside` rule a manifest
path uses), ending in `.html`, and a `height` of 1 to 2000. A tree that fails is
`plugin_ui_invalid`. `Plugins::asset` still applies its own checks when the file
is requested.

**Does the frame reach Tauri's IPC?** CVE-2024-35222 (GHSA-57fm-592m-34r7) was an
iframe from another origin invoking commands. Read in tauri 2.12.5 and
wry 0.57.1 (the versions in `Cargo.lock`):

- Tauri adds `__TAURI_INTERNALS__`, the invoke function and the invoke key through
  `main_frame_script(...)`, i.e. `for_main_frame_only: true`
  (`tauri/src/manager/webview.rs:161-220`). The key is a random 128-bit value
  embedded in a closure of that script (`scripts/ipc-protocol.js:12`), and every
  request must carry it (`webview/mod.rs`, `on_message`: a mismatch returns
  without a reply).
- **Linux (WebKitGTK):** wry honours the flag
  (`wry/src/webkitgtk/mod.rs:826-833`, `UserContentInjectedFrames::TopFrame`), so
  the frame has neither `__TAURI_INTERNALS__` nor the key, and a request without
  the key is dropped. `window.webkit.messageHandlers.ipc` does exist in every
  frame, and wry labels what it receives with the *top-level* URI, not the
  frame's (`webkitgtk/mod.rs:731-748`), so the origin check cannot tell the
  frame apart there. The only barrier on this path is the secrecy of the key,
  which an opaque-origin frame cannot read from its parent.
- **Windows (WebView2) is different, and not verified.** wry documents that
  scripts are always added to subframes regardless of the flag
  (`wry/src/lib.rs:1029`), so the frame receives Tauri's scripts, and with them
  the invoke key. Its custom-protocol invoke would go through
  `http://ipc.localhost`, which the page's CSP refuses (`connect-src`); but
  `ipc.js` falls back to `postMessage` when that fails, and the WebView2 handler
  labels the message with the frame's own URL (`webview2/mod.rs:971-1000`,
  `args.Source()`). Tauri then treats any registered custom scheme as **local**
  (`webview/mod.rs`, `is_local_url`, the `.localhost` branch), so a message from
  `http://hestia-plugin.localhost/...` would resolve the main window's
  capability rather than be refused as remote. WebView2 documents that a child
  frame's `chrome.webview.postMessage` raises `CoreWebView2Frame.WebMessageReceived`,
  not the top-level `CoreWebView2.WebMessageReceived`
  ([reference](https://learn.microsoft.com/en-us/dotnet/api/microsoft.web.webview2.core.corewebview2frame.webmessagereceived)),
  and wry 0.57.1 subscribes only to the top-level event and never to
  `FrameCreated` (`webview2/mod.rs:971`). So the fallback should reach no
  handler, and the page's CSP blocks the fetch path: both barriers hold on
  paper. If a later wry subscribes to frame messages, a plugin page could call
  every command the main window may, so the manual check below stays required
  and is repeated on each wry upgrade.
- A page's CSP allows the whole `hestia-plugin` scheme, so a page can read
  another plugin's served assets (icons, themes). They are already public to the
  app; scoping sources per plugin path was left out because custom-scheme path
  matching in CSP is unverified on both engines.
- `crates/desktop/capabilities/default.json` is for `windows: ["main"]` and has no
  `remote` entry, so a genuinely remote origin is granted nothing; the point
  above is that `hestia-plugin` is not remote to Tauri.
- Not run: a launch of the desktop on Linux with a webview page, and everything
  on Windows.

**Owner check on Windows, before this ships there:** load a plugin page whose
script calls `window.__TAURI_INTERNALS__?.invoke('ipc_call', …)` and
`window.chrome.webview.postMessage(…)`, and confirm both are refused. If either
succeeds, the fixes in order of preference are: render the `webview` node as a
notice on Windows; or load the page through `srcdoc` (its URL `about:srcdoc` is
remote to Tauri, so the ACL refuses it) with a `<base>` and a CSP the app injects.

**Rejected.** `allow-same-origin` (defeats the isolation). Trusting `event.origin`
(always `"null"`). A per-plugin `webview` permission now (a page can do only what
its plugin's commands may, and `plugin.run` is already gated); add one if a page
ever gets a broader bridge. A package manager or SDK for the bridge: a short
snippet in [docs/plugins/webview.md](../plugins/webview.md) is enough.
