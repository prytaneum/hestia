import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useRouter } from '@tanstack/react-router';
import { toast } from 'sonner';

import { errorMessage } from '@/api';
import { ConfirmDialog } from '@/components/ui/confirm-dialog';
import { runningSessions } from '@/lib/sessions';
import { m } from '@/paraglide/messages.js';
import { instanceMutations, instanceQueries } from '@/queries/instance';
import { serverMutations, serverQueries } from '@/queries/server';

const STOP_POLL_MS = 500;
const STOP_TIMEOUT_MS = 60_000;

/**
 * Stop and remove, per kind. The daemon refuses to remove a running entry and
 * `stop` only signals the process, so `isRunning` reads past the cache to tell
 * when the exit has actually landed.
 */
function useEntryLifecycle(kind: 'instance' | 'server', id: string) {
  const queryClient = useQueryClient();
  const stopServer = useMutation(serverMutations.stop(id));
  const stopInstance = useMutation(instanceMutations.stop(id));
  const removeServer = useMutation(serverMutations.remove(id));
  const removeInstance = useMutation(instanceMutations.remove(id));

  if (kind === 'server') {
    return {
      stop: () => stopServer.mutateAsync(),
      remove: () => removeServer.mutateAsync(),
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
    stop: () => stopInstance.mutateAsync({}),
    remove: () => removeInstance.mutateAsync(),
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
        toast.promise(stopAndRemove(), {
          loading: m['entry.remove.removing']({ name }),
          success: m['app.toast.removed']({ name }),
          error: (error: unknown) => errorMessage(error),
        });
      }}
    />
  );
}
