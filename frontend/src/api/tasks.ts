/**
 * The `server.task.*` channels: what the daemon runs on a schedule while a
 * server is up. A schedule is an interval (`30m`, `6h`, `1d`) or a daily time
 * (`04:30`, the daemon machine's clock).
 */
import { call } from './core/ipc';
import type { ServerTask } from './types/tasks';

export async function list(server: string): Promise<ServerTask[]> {
  const result = await call<{ tasks: ServerTask[] }>('server.task.list', {
    server,
  });
  return result.tasks;
}

/** Creates the task when it carries no id, replaces the one it names else. */
export function save(
  server: string,
  task: Partial<ServerTask>,
): Promise<ServerTask> {
  return call<ServerTask>('server.task.save', { server, task });
}

export async function remove(server: string, id: string): Promise<void> {
  await call('server.task.remove', { server, id });
}
