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

  it('nests properties under config and a content list under content', () => {
    expect(
      startsWith(keys.servers.properties('smp'), keys.servers.config('smp')),
    ).toBe(true);
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
