import { createContext, type ReactNode, useContext } from 'react';

import { SettingGroup } from '@/components/setting-controls';
import type { SettingsGroup, SettingsMatch } from '@/features/settings/lib';

const MatchCtx = createContext<SettingsMatch | null>(null);

/**
 * Hides whatever the settings search excludes. `match` is null while nothing is
 * being searched for, which every wrapper reads as "show me".
 */
export function SettingsFilter({
  match,
  children,
}: {
  match: SettingsMatch | null;
  children: ReactNode;
}) {
  return <MatchCtx.Provider value={match}>{children}</MatchCtx.Provider>;
}

/** Whether a search is narrowing the page — what a collapsed area opens for. */
export function useFiltering(): boolean {
  return useContext(MatchCtx) !== null;
}

/** One indexed setting. Renders nothing while filtered out. */
export function Setting({ id, children }: { id: string; children: ReactNode }) {
  const match = useContext(MatchCtx);
  if (match && !match.ids.has(id)) return null;
  return children;
}

/** A legend + its fields, gone entirely once none of its settings match. */
export function SettingsSection({
  group,
  legend,
  description,
  children,
}: {
  group: SettingsGroup;
  legend: string;
  description?: ReactNode;
  children: ReactNode;
}) {
  const match = useContext(MatchCtx);
  if (match && !match.groups.has(group)) return null;
  return (
    <SettingGroup legend={legend} description={description}>
      {children}
    </SettingGroup>
  );
}
