import { describe, expect, it } from 'vitest';
import { dialog, type Flavor } from '@/api';
import {
  choiceValues,
  groupByEdition,
  hasRule,
  rulesFor,
} from '@/features/shared/entry/lib';

const flavor = (id: string, edition: Flavor['edition']): Flavor => ({
  id,
  name: id,
  summary: '',
  edition,
  accepts: [],
  requires: [],
});

describe('flavors by edition', () => {
  it('groups in the order the daemon lists each edition', () => {
    const groups = groupByEdition([
      flavor('vanilla', 'java'),
      flavor('bedrock', 'bedrock'),
      flavor('paper', 'java'),
    ]);
    expect(groups.map((g) => g.edition)).toEqual(['java', 'bedrock']);
    expect(groups[0].flavors.map((f) => f.id)).toEqual(['vanilla', 'paper']);
  });
});

describe('edition rules', () => {
  const all = [
    {
      edition: 'java' as const,
      rules: [
        { key: 'hardcore', kind: { type: 'boolean' as const } },
        {
          key: 'gamemode',
          kind: { type: 'choice' as const, values: ['survival', 'spectator'] },
        },
      ],
    },
    { edition: 'bedrock' as const, rules: [] },
  ];

  it('reads the rules of the named edition only', () => {
    expect(hasRule(rulesFor(all, 'java'), 'hardcore')).toBe(true);
    expect(hasRule(rulesFor(all, 'bedrock'), 'hardcore')).toBe(false);
    expect(rulesFor(all, undefined)).toEqual([]);
  });

  it('lists the values of a choice rule', () => {
    const java = rulesFor(all, 'java');
    expect(choiceValues(java, 'gamemode')).toEqual(['survival', 'spectator']);
    expect(choiceValues(java, 'hardcore')).toBeNull();
  });
});

describe('local content files', () => {
  it('takes Bedrock packs only for an entry that accepts them', () => {
    expect(dialog.contentExtensions(['mod'])).toEqual(['jar', 'zip']);
    expect(dialog.contentExtensions(['behavior_pack'])).toContain('mcaddon');
  });
});
