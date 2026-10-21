import { describe, expect, it } from 'vitest';
import {
  type EntryCardModel,
  filterCards,
} from '@/features/shared/entry/components';

const card = (over: Partial<EntryCardModel> = {}): EntryCardModel => ({
  id: 'a',
  name: 'Alpha',
  kind: 'instance',
  flavor: 'fabric',
  version: '1.21.1',
  running: false,
  ready: true,
  subtitle: 'stopped',
  ...over,
});

const cards = [
  card(),
  card({ id: 'b', name: 'Bravo', flavor: 'neoforge', version: '1.20.1' }),
  card({ id: 'c', name: 'Charlie', flavor: 'fabric', version: '1.21.4' }),
];

describe('filterCards', () => {
  it('matches name, flavor and version, case-insensitively', () => {
    expect(filterCards(cards, 'brav').map((c) => c.id)).toEqual(['b']);
    expect(filterCards(cards, 'FABRIC').map((c) => c.id)).toEqual(['a', 'c']);
    expect(filterCards(cards, '1.20').map((c) => c.id)).toEqual(['b']);
  });

  it('ignores surrounding whitespace', () => {
    expect(filterCards(cards, '  alpha  ').map((c) => c.id)).toEqual(['a']);
  });

  it('narrows by flavor alongside the query', () => {
    expect(filterCards(cards, '', 'neoforge').map((c) => c.id)).toEqual(['b']);
    expect(filterCards(cards, 'charlie', 'neoforge')).toHaveLength(0);
  });

  it('treats "all" as no flavor constraint', () => {
    expect(filterCards(cards, '', 'all')).toHaveLength(3);
  });
});
