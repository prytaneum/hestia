import { WarningCircleIcon } from '@phosphor-icons/react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { useNavigate } from '@tanstack/react-router';
import { useMemo } from 'react';

import { errorMessage } from '@/api';
import type { Action, Effect } from '@/api/types/ui';
import { Empty } from '@/components/empty';
import { Bone } from '@/components/skeleton';
import { notify } from '@/lib/notify';
import { m } from '@/paraglide/messages.js';
import { pluginMutations, pluginQueries } from '@/queries/plugin';

import { ViewCtx } from './view/context';
import { UiView } from './view/render';

const TOAST = {
  success: notify.success,
  warning: notify.warning,
  error: notify.error,
} as const;

/**
 * One plugin page: asks the plugin for its tree, draws it, and sends its
 * actions back. `entry` is the server or instance an entry page sits on.
 */
export function PluginView({
  plugin,
  page,
  entry,
}: {
  plugin: string;
  page: string;
  entry?: string;
}) {
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const render = pluginQueries.render(plugin, page, entry);
  const tree = useQuery(render);
  const action = useMutation(pluginMutations.uiAction());

  const view = useMemo(() => {
    const apply = (effect: Effect) => {
      switch (effect.type) {
        case 'render':
          queryClient.setQueryData(render.queryKey, effect.tree);
          break;
        case 'toast':
          (TOAST[effect.tone as keyof typeof TOAST] ?? notify.info)(
            effect.text,
          );
          break;
        case 'navigate':
          void navigate({
            to: '/plugins/$plugin/$page',
            params: { plugin, page: effect.page },
          });
          break;
        case 'refresh':
          void queryClient.invalidateQueries({ queryKey: render.queryKey });
          break;
      }
    };
    return {
      plugin,
      pending: action.isPending,
      run: async (clicked: Action, values: unknown = {}) => {
        try {
          const effects = await action.mutateAsync({
            plugin,
            page,
            action: clicked,
            values,
            context: entry ? { entry } : {},
          });
          for (const effect of effects) apply(effect);
        } catch (error) {
          notify.failure(error, { id: `plugin-action-${plugin}` });
        }
      },
    };
  }, [action, entry, navigate, page, plugin, queryClient, render.queryKey]);

  if (tree.isPending) {
    return (
      <div className="space-y-3">
        <Bone className="h-6 w-48" />
        <Bone className="h-24" />
      </div>
    );
  }
  if (!tree.data) {
    return (
      <Empty icon={WarningCircleIcon} description={errorMessage(tree.error)}>
        {m['plugins.load_failed']()}
      </Empty>
    );
  }
  return (
    <ViewCtx.Provider value={view}>
      <UiView node={tree.data} />
    </ViewCtx.Provider>
  );
}
