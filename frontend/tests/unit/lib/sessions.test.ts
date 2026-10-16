import { describe, expect, it } from 'vitest';
import type { InstanceInfo, ProcessInfo } from '@/api';
import { runningSessions } from '@/lib/sessions';

const session = (
  id: string,
  state: ProcessInfo['state'],
  startedUnix: number,
): ProcessInfo => ({ id, state, startedUnix }) as ProcessInfo;

const instance = (sessions?: ProcessInfo[]): InstanceInfo =>
  ({ id: 'modded', sessions }) as InstanceInfo;

describe('runningSessions', () => {
  it('is empty when the instance has never been launched', () => {
    expect(runningSessions(instance())).toEqual([]);
    expect(runningSessions(instance([]))).toEqual([]);
  });

  it('drops sessions that are no longer running', () => {
    const live = session('instance-modded_2', 'running', 20);
    const gone = session('instance-modded_1', 'exited', 10);
    expect(runningSessions(instance([live, gone]))).toEqual([live]);
  });

  it('orders by launch time, oldest first', () => {
    const second = session('instance-modded_2', 'running', 200);
    const first = session('instance-modded_1', 'running', 100);
    expect(
      runningSessions(instance([second, first])).map((s) => s.id),
    ).toEqual(['instance-modded_1', 'instance-modded_2']);
  });
});
