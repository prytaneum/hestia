# Web views

When a page needs something the [declarative nodes](ui.md) cannot draw, such as a chart or a map, return a `webview`
node. It shows an HTML file from your package's `assets/` in a sandboxed frame
([0109](../decisions/0109-plugin-webviews-are-opaque-iframes.md)).

```json
{ "type": "webview", "src": "assets/chart.html", "height": 320 }
```

`src` is an `.html` file under `assets/`; `height` is 1 to 2000 pixels. In Rust: `ui::webview("assets/chart.html", 320)`.
The desktop draws it; the CLI prints a note.

## What the page can and cannot do

- Run its own scripts and styles, loaded from your `assets/` (`.js`, `.css`, images, fonts, `.json`). Reference them
  by relative path.
- It has **no network**, no other frames, no forms, no cookies or storage shared with the app, and no access to the
  app's DOM. Bundle everything it needs.
- It talks to the host with `postMessage`, and only for **your plugin**:

| Request | Does | Reply `result` |
|---|---|---|
| `{ id, type: "run", command, args }` | runs one of your `top` commands | the command's output |
| `{ id, type: "action", action, payload }` | sends a UI action to the page being shown, as a button does | `null` |

Replies are `{ id, ok: true, result }` or `{ id, ok: false, error }`. A message that is not exactly one of these, or is
over 64 KiB, gets no reply.

## A helper to copy

```js
const pending = new Map();

window.addEventListener("message", (event) => {
  if (event.source !== window.parent) return;
  const reply = event.data;
  const waiting = pending.get(reply?.id);
  if (!waiting) return;
  pending.delete(reply.id);
  clearTimeout(waiting.timer);
  reply.ok ? waiting.resolve(reply.result) : waiting.reject(new Error(reply.error));
});

function hestia(type, body, timeoutMs = 30000) {
  const id = crypto.randomUUID();
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      pending.delete(id);
      reject(new Error("the host did not answer"));
    }, timeoutMs);
    pending.set(id, { resolve, reject, timer });
    window.parent.postMessage({ id, type, ...body }, "*");
  });
}

const runCommand = (command, args = {}) => hestia("run", { command, args });
const sendAction = (action, payload = null) => hestia("action", { action, payload });
```

`"*"` is required: the host's origin is not addressable from an opaque-origin frame. The check that matters is on the
host's side, which only accepts messages from the frame it created, and on yours (`event.source === window.parent`).
