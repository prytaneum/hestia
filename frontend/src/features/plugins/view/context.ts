import { createContext, useContext } from 'react';

import type { Action } from '@/api/types/ui';

export interface ViewContext {
  plugin: string;
  pending: boolean;
  run: (action: Action, values?: unknown) => Promise<void>;
}

export const ViewCtx = createContext<ViewContext | null>(null);

export function useView(): ViewContext {
  const ctx = useContext(ViewCtx);
  if (!ctx) throw new Error('plugin view used outside a PluginView');
  return ctx;
}
