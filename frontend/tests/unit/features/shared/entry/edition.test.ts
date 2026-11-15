import { describe, expect, it } from 'vitest';
import { dialog } from '@/api';
import {
  choiceValues,
  hasRule,
  rulesFor,
} from '@/features/shared/entry/lib';

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
  ].map((catalogue) => ({ ...catalogue, flavors: [] }));

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
