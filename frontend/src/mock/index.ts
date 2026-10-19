/**
 * The browser fixture daemon.
 *
 * Dev-only: it fakes `window.__TAURI_INTERNALS__` so the frontend runs in a
 * plain browser with no daemon and no Tauri shell behind it. Stripped from the
 * desktop build, and skipped when the real shell is present.
 *
 * It is laid out the way the thing it replaces is:
 *
 * | Here            | Stands in for                          |
 * |-----------------|----------------------------------------|
 * | `state/`        | `crates/engine` — the mutable world     |
 * | `channels/`     | `crates/daemon/src/services/` — one module per domain |
 * | `commands/`     | `crates/desktop/src/commands/` + the bundled plugins |
 * | `router.ts`     | the daemon's router and the shell's bridge |
 * | `job.ts`/`bus.ts` | the job managers and the event hub    |
 *
 * Adding a channel is one line in its domain's map; adding a domain is a
 * module plus a line in `channels/index.ts`. Fixtures are typed against the
 * generated `proto` mirrors, so a wire change breaks the typecheck here rather
 * than silently serving a stale shape.
 */
import { channels as instanceChannels } from './channels/instance';
import { installInternals } from './internals';
import { listInstances } from './state/entries';

declare global {
  interface Window {
    /** Dev console: `__hestiaMock.foreignLaunch()` starts a launch no webview asked for. */
    __hestiaMock?: { foreignLaunch: () => void };
  }
}

export async function installBrowserMock(): Promise<void> {
  installInternals();
  window.__hestiaMock = {
    foreignLaunch: () => {
      const [instance] = listInstances();
      instanceChannels['instance.launch']?.({ instance: instance.id });
    },
  };
  console.info('[mock] running against the fixture daemon (browser dev mode)');
}
