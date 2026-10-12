/** `server.task.*`: one task list per server, stored as the daemon stores it. */
import type { ServerTask } from '@/api/types';

import { fail, type Handlers, now, ok, str } from '../support';

const tasks = new Map<string, ServerTask[]>([
  [
    'smp',
    [
      {
        id: 'task-restart',
        action: 'restart',
        command: '',
        schedule: '04:30',
        enabled: true,
        lastRunUnix: now(),
      },
    ],
  ],
]);

function tasksOf(server: string): ServerTask[] {
  let mine = tasks.get(server);
  if (!mine) {
    mine = [];
    tasks.set(server, mine);
  }
  return mine;
}

const SCHEDULE = /^(\d+[mhd]|([01]\d|2[0-3]):[0-5]\d)$/;

export const channels: Handlers = {
  'server.task.list': (p) => ({ tasks: tasksOf(str(p, 'server')) }),

  'server.task.save': (p) => {
    const mine = tasksOf(str(p, 'server'));
    const given = (p.task ?? {}) as Partial<ServerTask>;
    if (!SCHEDULE.test(given.schedule ?? '')) {
      fail('bad_request', 'a schedule is an interval or a daily time');
    }
    const task: ServerTask = {
      id: given.id || crypto.randomUUID(),
      action: given.action ?? 'restart',
      command: given.command ?? '',
      schedule: given.schedule ?? '',
      enabled: given.enabled ?? true,
      lastRunUnix: now(),
    };
    const at = mine.findIndex((stored) => stored.id === task.id);
    if (at >= 0) mine[at] = task;
    else mine.push(task);
    return task;
  },

  'server.task.remove': (p) => {
    const mine = tasksOf(str(p, 'server'));
    const at = mine.findIndex((stored) => stored.id === str(p, 'id'));
    if (at < 0) fail('not_found', 'no such task');
    mine.splice(at, 1);
    return ok();
  },
};
