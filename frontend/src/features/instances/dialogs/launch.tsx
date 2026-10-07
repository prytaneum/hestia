import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
} from 'react';

import type { InstanceInfo, QuickPlay } from '@/api';
import { AppDialog } from '@/components/dialog';
import { ProvisionProgressView } from '@/features/shared/entry/components';
import { toastWarnings } from '@/lib/warnings';
import { m } from '@/paraglide/messages.js';
import { instanceMutations } from '@/queries/instance';
import { useJobDisplay, useJobMutation } from '@/queries/jobs';

interface LaunchOptions {
  /**
   * `newSession` launches alongside whatever is already running; without it a
   * running instance is refused by the daemon.
   */
  newSession?: boolean;
  quickPlay?: QuickPlay;
}

interface LaunchDialog {
  launch: (instance: InstanceInfo, options?: LaunchOptions) => void;
  /** A Quick Play matches only its own `target`, so one row spins, not all. */
  isLaunching: (id: string, target?: string) => boolean;
}

const Ctx = createContext<LaunchDialog | null>(null);

export function useLaunchDialog(): LaunchDialog {
  const ctx = useContext(Ctx);
  if (!ctx) {
    throw new Error('useLaunchDialog must be used within LaunchDialogProvider');
  }
  return ctx;
}

/**
 * Owns the single launch mutation and the first-launch progress modal. Every
 * way of starting an instance goes through it, Quick Play included. An instance
 * that has never been played (`lastPlayedUnix` unset) shows the modal while it
 * materialises; a re-launch runs silently as a backgrounded job. The modal can
 * be dismissed to push the job to the status bar and keep working.
 */
export function LaunchDialogProvider({
  children,
}: {
  children: React.ReactNode;
}) {
  const mutation = useJobMutation(instanceMutations.launchAny());
  const [target, setTarget] = useState<{ id: string; name: string } | null>(
    null,
  );

  // The provider re-renders on every progress tick of the job it owns, so the
  // context value is memoized: a consumer re-renders when the launch state
  // changes, not when a byte count does.
  const { mutate, isPending, variables } = mutation;
  const launch = useCallback(
    (instance: InstanceInfo, options?: LaunchOptions) => {
      // The session is running either way; the warnings say what it runs
      // against (an unshared saves folder, say), so they follow a backgrounded
      // launch too.
      mutate(
        {
          id: instance.id,
          newSession: options?.newSession,
          quickPlay: options?.quickPlay,
        },
        { onSuccess: (done) => toastWarnings(done.warnings) },
      );
      if (instance.lastPlayedUnix == null) {
        setTarget({ id: instance.id, name: instance.name });
      }
    },
    [mutate],
  );
  const isLaunching = useCallback(
    (id: string, join?: string) =>
      isPending && variables?.id === id && variables.quickPlay?.target === join,
    [isPending, variables],
  );
  const value = useMemo(() => ({ launch, isLaunching }), [launch, isLaunching]);

  const job = mutation.job;
  const open = target != null;
  useJobDisplay(job, open);

  useEffect(() => {
    if (
      target &&
      job &&
      job.status !== 'running' &&
      job.entry?.id === target.id
    ) {
      setTarget(null);
    }
  }, [target, job]);

  return (
    <Ctx.Provider value={value}>
      {children}
      <AppDialog
        open={open}
        onOpenChange={(next) => {
          if (!next) setTarget(null);
        }}
        size="lg"
        title={m['instance.launch.title']({ name: target?.name ?? '' })}
        description={m['instance.launch.preparing']()}
      >
        <ProvisionProgressView
          progress={mutation.progress}
          className="min-h-72"
        />
      </AppDialog>
    </Ctx.Provider>
  );
}
