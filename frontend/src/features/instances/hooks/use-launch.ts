import { useCallback } from 'react';

import type { QuickPlay, Replace } from '@/api';
import { instanceMutations } from '@/queries/instance';
import { runJobMutation, useIsLaunching } from '@/queries/jobs';

interface LaunchOptions {
  /**
   * `newSession` launches alongside whatever is already running; without it a
   * running instance is refused by the daemon.
   */
  newSession?: boolean;
  quickPlay?: QuickPlay;
  /** The daemon stops these sessions and waits them out before launching. */
  replace?: Replace;
}

const launchJob = instanceMutations.launchAny();

/**
 * Every way of starting an instance, Quick Play included. The job store owns
 * its progress, notification and retry, so no provider has to stay mounted.
 */
export function useLaunch() {
  const isLaunching = useIsLaunching();
  const launch = useCallback(
    (id: string, options: LaunchOptions = {}) =>
      runJobMutation(launchJob, { id, ...options }),
    [],
  );
  const restart = useCallback(
    (id: string, session: string) =>
      launch(id, { replace: { kind: 'one', session } }),
    [launch],
  );
  return { launch, restart, isLaunching };
}
