import { describe, expect, it } from 'vitest';
import { FOOTPRINT, keys, scopedToEntry } from '@/queries/keys';

const startsWith = (key: readonly unknown[], prefix: readonly unknown[]) =>
  prefix.every((part, i) => Object.is(key[i], part));

describe('the query-key hierarchy', () => {
  it('nests every server list and detail under the kind root', () => {
    expect(startsWith(keys.servers.list(), keys.servers.all)).toBe(true);
    expect(startsWith(keys.servers.detail('smp'), keys.servers.all)).toBe(true);
  });

  it('nests an entry’s per-resource keys under its detail', () => {
    const detail = keys.servers.detail('smp');
    for (const key of [
      keys.servers.config('smp'),
      keys.servers.backups('smp'),
      keys.servers.content('smp'),
      keys.servers.logs('smp'),
      keys.servers.ping('smp'),
      keys.servers.modpack('smp'),
    ]) {
      expect(startsWith(key, detail)).toBe(true);
    }
  });

  it('nests a config value under the entry’s config key', () => {
    expect(
      startsWith(
        keys.servers.configValue('smp', 'memory'),
        keys.servers.config('smp'),
      ),
    ).toBe(true);
  });

  it('nests a content list under the entry’s content key', () => {
    expect(
      startsWith(
        keys.servers.contentList('smp', 'mod'),
        keys.servers.content('smp'),
      ),
    ).toBe(true);
  });

  it('keeps the footprint walk outside the entry subtree', () => {
    expect(keys.servers.info('smp')[0]).toBe(FOOTPRINT);
    expect(keys.instances.info('modded')[0]).toBe(FOOTPRINT);
    expect(startsWith(keys.servers.info('smp'), keys.servers.all)).toBe(false);
  });

  it('keeps servers and instances in separate subtrees', () => {
    expect(startsWith(keys.instances.detail('x'), keys.servers.all)).toBe(false);
    expect(startsWith(keys.servers.detail('x'), keys.instances.all)).toBe(false);
  });

  it('separates two entries of the same kind', () => {
    expect(startsWith(keys.servers.detail('a'), keys.servers.detail('b'))).toBe(
      false,
    );
  });

  it('keys a multiplayer ping by address, not by entry', () => {
    expect(keys.instances.serverStatus('a:25565')).toEqual([
      'minecraft',
      'ping',
      'a:25565',
    ]);
  });

  it('keys an archive inspect by path, since no entry exists yet', () => {
    expect(startsWith(keys.transfer.archive('/tmp/x.hestia'), ['transfer'])).toBe(
      true,
    );
  });
});

describe('the reads that own a vanishing id', () => {
  it('covers everything keyed to one entry or session', () => {
    for (const key of [
      keys.servers.detail('smp'),
      keys.servers.config('smp'),
      keys.servers.backups('smp'),
      keys.servers.contentList('smp', 'mod'),
      keys.servers.modpack('smp'),
      keys.servers.info('smp'),
      keys.instances.detail('modded'),
      keys.instances.worlds('modded'),
      keys.instances.logs('modded', 'instance-modded_1'),
      keys.instances.info('modded'),
    ]) {
      expect(scopedToEntry(key)).toBe(true);
    }
  });

  it('leaves the lists and catalogues alone — a miss there is a real failure', () => {
    for (const key of [
      keys.servers.list(),
      keys.instances.list(),
      keys.instances.flavors(),
      keys.instances.serverStatus('a:25565'),
      keys.content.project('modrinth', 'sodium'),
      keys.content.url('https://modrinth.com/mod/sodium'),
      keys.transfer.archive('/tmp/x.hestia'),
    ]) {
      expect(scopedToEntry(key)).toBe(false);
    }
  });
});
