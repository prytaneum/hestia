import { describe, expect, it } from 'vitest';
import type { ServerProperty } from '@/api';
import { matchProperties, propertyControl } from '@/features/servers/lib';

const property = (over: Partial<ServerProperty>): ServerProperty => ({
  key: 'some-key',
  value: '',
  managed: false,
  secret: false,
  locked: false,
  ...over,
});

describe('choosing a control for a property', () => {
  it('reads it off the shape of the default, whatever the key is', () => {
    expect(propertyControl(property({ default: 'true' }))).toBe('toggle');
    expect(propertyControl(property({ default: '25565' }))).toBe('number');
    expect(propertyControl(property({ default: '-1' }))).toBe('number');
    expect(propertyControl(property({ default: 'world' }))).toBe('text');
    expect(propertyControl(property({ default: '' }))).toBe('text');
  });

  it('keeps the default’s shape when the value has drifted from it', () => {
    expect(
      propertyControl(property({ default: 'false', value: 'maybe' })),
    ).toBe('toggle');
  });

  it('falls back to the value for a key the version does not define', () => {
    expect(propertyControl(property({ value: 'true' }))).toBe('toggle');
    expect(propertyControl(property({ value: 'leftover' }))).toBe('text');
  });
});

describe('searching properties', () => {
  const all = [
    property({ key: 'max-players' }),
    property({ key: 'rcon.port' }),
    property({ key: 'server-port' }),
  ];

  it('matches any part of the key, ignoring case', () => {
    expect(matchProperties(all, 'PORT').map((p) => p.key)).toEqual([
      'rcon.port',
      'server-port',
    ]);
  });

  it('keeps everything for an empty search', () => {
    expect(matchProperties(all, '  ')).toHaveLength(3);
  });
});
