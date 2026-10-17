import { describe, expect, it } from 'vitest';
import type { DaemonEvent, ProvisionProgress } from '@/api';
import { listServers } from '@/mock/state/entries';
import { invalidationKeys } from '@/queries/invalidation';
import { keys } from '@/queries/keys';

const server = listServers()[0];

const progress: ProvisionProgress = {
  phase: 'resolving',
  current: 1,
  total: 2,
  detail: '',
  item: 0,
  items: 0,
};

const created: DaemonEvent = {
  topic: 'job',
  payload: {
    id: 'job-1',
    channel: 'server.create',
    state: 'done',
    server,
    warnings: [],
  },
};

describe('the daemon-topic invalidation map', () => {
  it('refreshes a server’s row and detail on a lifecycle event', () => {
    expect(
      invalidationKeys({
        topic: 'process.started',
        payload: { id: 'server-smp', pid: 1 },
      }),
    ).toEqual([keys.servers.list(), keys.servers.detail('smp')]);
  });

  it('reads the entry out of a sessioned instance process id', () => {
    expect(
      invalidationKeys({
        topic: 'process.exit',
        payload: { id: 'instance-modded_3', state: 'exited', success: true },
      }),
    ).toEqual([
      keys.instances.list(),
      keys.instances.recent(),
      keys.instances.detail('modded'),
    ]);
  });

  it('outdates nothing for an unrecognised process id', () => {
    expect(
      invalidationKeys({
        topic: 'process.exit',
        payload: { id: 'tray-1', state: 'exited', success: true },
      }),
    ).toEqual([]);
  });

  it('refreshes the list after a create, successful or failed', () => {
    expect(invalidationKeys(created)).toEqual([keys.servers.list()]);
    expect(
      invalidationKeys({
        topic: 'job',
        payload: {
          id: 'job-1',
          channel: 'server.create',
          state: 'failed',
          error: { kind: 'internal', detail: 'boom' },
        },
      }),
    ).toEqual([keys.servers.list()]);
  });

  it('names the updated server from the event payload', () => {
    expect(
      invalidationKeys({
        topic: 'job',
        payload: {
          id: 'job-2',
          channel: 'server.update',
          state: 'done',
          server,
          warnings: [],
        },
      }),
    ).toEqual([keys.servers.list(), keys.servers.detail(server.id)]);
  });

  it('resolves a launch through its process id', () => {
    expect(
      invalidationKeys({
        topic: 'job',
        payload: {
          id: 'job-3',
          channel: 'instance.launch',
          state: 'done',
          processId: 'instance-vanilla_1',
          pid: 7,
          warnings: [],
        },
      }),
    ).toEqual([keys.instances.list(), keys.instances.detail('vanilla')]);
  });

  it('sweeps both entry kinds for work that can land in either', () => {
    expect(
      invalidationKeys({
        topic: 'job',
        payload: {
          id: 'job-4',
          channel: 'content.add',
          state: 'done',
          items: [],
          failures: [],
        },
      }),
    ).toEqual([keys.servers.all, keys.instances.all]);
  });

  it('refreshes both entry kinds when another client changed an entry', () => {
    expect(
      invalidationKeys({ topic: 'changed', payload: { scope: 'entries' } }),
    ).toEqual([keys.servers.all, keys.instances.all]);
  });

  it('refreshes the skins that follow a changed account', () => {
    expect(
      invalidationKeys({ topic: 'changed', payload: { scope: 'accounts' } }),
    ).toEqual([keys.accounts.all, keys.skins.all]);
  });

  it('outdates nothing while a job is still reporting progress', () => {
    expect(
      invalidationKeys({
        topic: 'job',
        payload: {
          id: 'job-1',
          channel: 'server.create',
          state: 'progress',
          ...progress,
        },
      }),
    ).toEqual([]);
  });
});
