import { describe, expect, it } from 'vitest';
import { invalidationKeys } from '@/queries/invalidation';
import { FOOTPRINT, keys } from '@/queries/keys';

describe('the daemon-topic invalidation map', () => {
  it('refreshes a server’s row and detail on a lifecycle event', () => {
    expect(invalidationKeys('process.started', { id: 'server-smp' })).toEqual([
      keys.processes.list(),
      keys.servers.list(),
      keys.servers.detail('smp'),
    ]);
  });

  it('reads the entry out of a sessioned instance process id', () => {
    expect(invalidationKeys('process.exit', { id: 'instance-modded_3' })).toEqual(
      [
        keys.processes.list(),
        keys.instances.list(),
        keys.instances.recent(),
        keys.instances.detail('modded'),
      ],
    );
  });

  it('falls back to the process list for an unrecognised process id', () => {
    expect(invalidationKeys('process.exit', { id: 'tray-1' })).toEqual([
      keys.processes.list(),
    ]);
  });

  it('refreshes the list after a create, successful or failed', () => {
    expect(invalidationKeys('server.create.done', {})).toEqual([
      keys.servers.list(),
    ]);
    expect(invalidationKeys('server.create.error', {})).toEqual([
      keys.servers.list(),
    ]);
  });

  it('names the updated server from the event payload', () => {
    expect(
      invalidationKeys('server.update.done', { server: { id: 'smp' } }),
    ).toEqual([keys.servers.list(), keys.servers.detail('smp')]);
  });

  it('refreshes only the list when the update names no server', () => {
    expect(invalidationKeys('server.update.done', {})).toEqual([
      keys.servers.list(),
    ]);
  });

  it('resolves a launch through its process id', () => {
    expect(
      invalidationKeys('instance.launch.done', { processId: 'instance-vanilla_1' }),
    ).toEqual([keys.instances.list(), keys.instances.detail('vanilla')]);
  });

  it('sweeps both entry kinds for work that can land in either', () => {
    for (const topic of ['content.done', 'modpack.done']) {
      expect(invalidationKeys(topic, {})).toEqual([
        keys.servers.all,
        keys.instances.all,
      ]);
    }
  });

  it('sweeps the server subtree for a backup, whose topic names no entry', () => {
    expect(invalidationKeys('backup.done', {})).toEqual([keys.servers.all]);
  });

  it('refreshes only the footprint after an export', () => {
    expect(invalidationKeys('instance.export.done', {})).toEqual([[FOOTPRINT]]);
  });

  it('refreshes the runtimes after a java install', () => {
    expect(invalidationKeys('java.install.done', {})).toEqual([
      keys.java.runtimes(),
    ]);
  });

  it('outdates nothing for an unmapped topic', () => {
    expect(invalidationKeys('server.create.progress', {})).toEqual([]);
  });
});
