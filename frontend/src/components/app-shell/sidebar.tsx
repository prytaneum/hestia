import { useLocation } from '@tanstack/react-router';

import { anchor } from '@/features/onboarding';

import { AccountMenu } from './account-menu';
import { isActive, NavLink, nav, settingsItem } from './nav';
import { PinnedSection } from './pinned-section';

export function Sidebar() {
  const { pathname } = useLocation();

  return (
    <nav className="flex w-52 shrink-0 flex-col border-r border-border bg-sidebar">
      <div className="min-h-0 flex-1 overflow-y-auto">
        <div className="space-y-0.5 p-2" {...anchor('nav')}>
          {nav.map((item) => (
            <NavLink
              key={item.to}
              item={item}
              active={isActive(pathname, item)}
            />
          ))}
        </div>

        <PinnedSection pathname={pathname} />
      </div>

      <div className="flex h-[108px] flex-col gap-0.5 border-t border-border p-2">
        <NavLink
          item={settingsItem}
          active={isActive(pathname, settingsItem)}
        />
        <AccountMenu />
      </div>
    </nav>
  );
}
