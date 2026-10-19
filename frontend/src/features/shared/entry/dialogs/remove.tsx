import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useRouter } from '@tanstack/react-router';

import { ConfirmDialog } from '@/components/ui/confirm-dialog';
import { notify } from '@/lib/notify';
import { runningSessions } from '@/lib/sessions';
import { m } from '@/paraglide/messages.js';
import { entryMutations } from '@/queries/entry';
import { instanceQueries } from '@/queries/instance';
import { serverQueries } from '@/queries/server';

const STOP_POLL_MS = 500;
const STOP_TIMEOUT_MS = 60_000;

/**
 * Stop and remove, per kind. The daemon refuses to remove a running entry and
 * `stop` only signals the process, so `isRunning` reads past the cache to tell
 * when the exit has actually landed.
 */
function useEntryLifecycle(kind: 'instance' | 'server', id: string) {
  const queryClient = useQueryClient();
  const stopEntry = useMutation({
    ...entryMutations.stop(kind, id),
    meta: { notify: false },
  });
  const removeEntry = useMutation({
    ...entryMutations.remove(kind, id),
    meta: { notify: false },
  });

  const stop = () => stopEntry.mutateAsync({});
  const remove = () => removeEntry.mutateAsync();

  if (kind === 'server') {
    return {
      stop,
      remove,
      isRunning: async () => {
        const servers = await queryClient.fetchQuery({
          ...serverQueries.list(),
          staleTime: 0,
        });
        return servers.some(
          (server) => server.id === id && server.process?.state === 'running',
        );
      },
    };
  }
  return {
    stop,
    remove,
    isRunning: async () => {
      const instances = await queryClient.fetchQuery({
        ...instanceQueries.list(),
        staleTime: 0,
      });
      return instances.some(
        (instance) =>
          instance.id === id && runningSessions(instance).length > 0,
      );
    },
  };
}

/** Confirms, then removes an entry — stopping it first when it is running. */
export function RemoveEntryDialog({
  kind,
  id,
  name,
  running,
  open,
  onOpenChange,
}: {
  kind: 'instance' | 'server';
  id: string;
  name: string;
  running: boolean;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const router = useRouter();
  const lifecycle = useEntryLifecycle(kind, id);
  const server = kind === 'server';

  const stopAndRemove = async () => {
    if (running) {
      await lifecycle.stop();
      const deadline = Date.now() + STOP_TIMEOUT_MS;
      while (await lifecycle.isRunning()) {
        if (Date.now() > deadline) {
          throw new Error(m['entry.remove.stop_timeout']({ name }));
        }
        await new Promise((resolve) => setTimeout(resolve, STOP_POLL_MS));
      }
    }
    await lifecycle.remove();
    if (router.state.location.pathname === `/${kind}s/${id}`) {
      router.navigate({ to: server ? '/servers' : '/instances' });
    }
  };

  const removeLabel = server
    ? m['entry.settings.remove.server']()
    : m['entry.settings.remove.instance']();

  return (
    <ConfirmDialog
      open={open}
      onOpenChange={onOpenChange}
      title={
        server
          ? m['entry.settings.remove.server_title']()
          : m['entry.settings.remove.instance_title']()
      }
      description={
        <>
          {m['entry.settings.remove.description']({ name })}
          {running && ` ${m['entry.remove.stops_first']()}`}
        </>
      }
      destructive
      confirmLabel={removeLabel}
      onConfirm={() => {
        notify
          .promise(stopAndRemove(), {
            id: `remove:${kind}:${id}`,
            loading: m['entry.remove.removing']({ name }),
            success: () => m['app.toast.removed']({ name }),
          })
          .catch(() => undefined);
      }}
    />
  );
}
