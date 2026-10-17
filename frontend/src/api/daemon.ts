/** The `daemon.*` channels. */
import { call, invokeCommand } from './core/ipc';
import type { DaemonStatusResult } from './types/daemon';

export function status(): Promise<DaemonStatusResult> {
  return call('daemon.status');
}

/**
 * Stop the daemon. Without `stopProcesses`, supervised workloads keep running —
 * which is what the shell's stop button means: a running server is not the
 * app's to end. The CLI's third option (ask which) is a terminal affordance;
 * a button has to have already decided.
 */
export async function stop(stopProcesses = false): Promise<boolean> {
  const result = await call('daemon.stop', { stopProcesses });
  return result.stopping;
}

/** Start the daemon via the shell's `start_daemon` command; `ipc_call` never spawns. */
export async function start(): Promise<DaemonStatusResult> {
  await invokeCommand('start_daemon');
  return status();
}

/**
 * Stop then start again — picks up a freshly built `hestiad`; supervised
 * processes are re-adopted.
 */
export async function restart(): Promise<DaemonStatusResult> {
  await stop(false);
  return relaunch();
}

/** Start the daemon once a stopping one has gone. */
export async function relaunch(): Promise<DaemonStatusResult> {
  await invokeCommand('relaunch_daemon');
  return status();
}
