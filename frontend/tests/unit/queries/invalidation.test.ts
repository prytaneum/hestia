import { describe, expect, it } from 'vitest';
import { invalidationKeys } from '@/queries/invalidation';
import { FOOTPRINT, keys } from '@/queries/keys';

describe('the daemon-topic invalidation map', () => {
  it('refreshes a server’s row and detail on a lifecycle event', () => {
    expect(invalidationKeys('process.started', { id: 'server-smp' })).toEqual([
      keys.servers.list(),
      keys.servers.detail('smp'),
    ]);
  });

  it('reads the entry out of a sessioned instance process id', () => {
    expect(invalidationKeys('process.exit', { id: 'instance-modded_3' })).toEqual(
      [
        keys.instances.list(),
        keys.instances.recent(),
        keys.instances.detail('modded'),
      ],
    );
  });

  it('outdates nothing for an unrecognised process id', () => {
    expect(invalidationKeys('process.exit', { id: 'tray-1' })).toEqual([]);
  });

  it('refreshes the list after a create, successful or failed', () => {
    for (const state of ['done', 'failed']) {
      expect(
        invalidationKeys('job', { channel: 'server.create', state }),
      ).toEqual([keys.servers.list()]);
    }
  });

  it('names the updated server from the event payload', () => {
    expect(
      invalidationKeys('job', {
        channel: 'server.update',
        state: 'done',
        server: { id: 'smp' },
      }),
    ).toEqual([keys.servers.list(), keys.servers.detail('smp')]);
  });

  it('refreshes only the list when the update names no server', () => {
    expect(
      invalidationKeys('job', { channel: 'server.update', state: 'done' }),
    ).toEqual([keys.servers.list()]);
  });

  it('resolves a launch through its process id', () => {
    expect(
      invalidationKeys('job', {
        channel: 'instance.launch',
        state: 'done',
        processId: 'instance-vanilla_1',
      }),
    ).toEqual([keys.instances.list(), keys.instances.detail('vanilla')]);
  });

  it('sweeps both entry kinds for work that can land in either', () => {
    for (const channel of ['content.add', 'modpack.update']) {
      expect(invalidationKeys('job', { channel, state: 'done' })).toEqual([
        keys.servers.all,
        keys.instances.all,
      ]);
    }
  });

  it('sweeps the server subtree for a backup, whose event names no entry', () => {
    expect(
      invalidationKeys('job', { channel: 'server.backup.create', state: 'done' }),
    ).toEqual([keys.servers.all]);
  });

  it('refreshes only the footprint after an export', () => {
    expect(
      invalidationKeys('job', { channel: 'instance.export', state: 'done' }),
    ).toEqual([[FOOTPRINT]]);
  });

  it('refreshes the runtimes after a java install', () => {
    expect(
      invalidationKeys('job', { channel: 'java.install', state: 'done' }),
    ).toEqual([keys.java.runtimes()]);
  });

  it('outdates nothing while a job is still reporting progress', () => {
    expect(
      invalidationKeys('job', { channel: 'server.create', state: 'progress' }),
    ).toEqual([]);
  });
});
