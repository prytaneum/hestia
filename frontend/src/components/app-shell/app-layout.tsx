import { Outlet, useLocation } from '@tanstack/react-router';

import { FileDropOverlay } from '@/components/app-shell/file-drop-overlay';
import { OfflineOverlay } from '@/components/app-shell/offline-overlay';
import { PlayBar } from '@/components/app-shell/play-bar';
import { SearchProvider } from '@/components/app-shell/search-context';
import { Sidebar } from '@/components/app-shell/sidebar';
import { StatusBar } from '@/components/app-shell/status-bar';
import { TopNav } from '@/components/app-shell/top-nav';
import { LaunchDialogProvider } from '@/features/instances/dialogs';
import { TourProvider } from '@/features/onboarding';
import { WelcomeDialog } from '@/features/onboarding/welcome';

export function AppLayout() {
  const { pathname } = useLocation();

  return (
    <SearchProvider>
      <LaunchDialogProvider>
        <TourProvider>
          <div className="flex h-screen w-screen flex-col overflow-hidden bg-background text-foreground">
            <TopNav />

            <div className="flex min-h-0 flex-1">
              <Sidebar />

              <div className="flex min-w-0 flex-1 flex-col">
                <main className="flex-1 overflow-y-auto">
                  <Outlet />
                </main>
                {pathname === '/' && <PlayBar />}
                <StatusBar />
              </div>
            </div>
          </div>
          <WelcomeDialog />
          <FileDropOverlay />
          <OfflineOverlay />
        </TourProvider>
      </LaunchDialogProvider>
    </SearchProvider>
  );
}
