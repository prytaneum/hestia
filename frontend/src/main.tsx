import { QueryClientProvider } from '@tanstack/react-query';
import { RouterProvider } from '@tanstack/react-router';
import { MotionConfig } from 'motion/react';
import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import { ErrorBoundary } from './components/error-boundary';
import { watchOpenedArchives } from './features/instances/hooks';
import { LocaleProvider } from './hooks/locale';
import { installCrashHandlers } from './lib/crash';
import { initDesktopShell } from './lib/desktop';
import { watchFileDrops } from './lib/file-drop';
import { log } from './lib/log';
import { reportReady } from './lib/splash';
import { queryClient, startInvalidation, startJobs } from './queries';
import { startSessionTracking } from './queries/sessions';
import { getRouter } from './router';
import './styles.css';

// Browser dev only: fake the Tauri bridge before anything calls `invoke()`.
// Stripped from the desktop release, and skipped when the real shell is present.
if (import.meta.env.DEV && !('__TAURI_INTERNALS__' in window)) {
  await (await import('./mock')).installBrowserMock();
}

installCrashHandlers();
log.info({ mode: import.meta.env.MODE }, 'ui starting');
initDesktopShell();
startInvalidation();
startJobs().catch(() => {
  // Outside the Tauri shell there is no daemon to mirror.
});
watchOpenedArchives();
watchFileDrops();
startSessionTracking();
const router = getRouter();

const rootElement = document.getElementById('app');
if (rootElement && !rootElement.innerHTML) {
  createRoot(rootElement).render(
    <StrictMode>
      <ErrorBoundary>
        <QueryClientProvider client={queryClient}>
          <LocaleProvider>
            <MotionConfig reducedMotion="user">
              <RouterProvider router={router} />
            </MotionConfig>
          </LocaleProvider>
        </QueryClientProvider>
      </ErrorBoundary>
    </StrictMode>,
  );
  reportReady();
}
