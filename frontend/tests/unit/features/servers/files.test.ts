import { describe, expect, it } from 'vitest';
import { freeName, nameIssue } from '@/features/servers/lib';

describe('picking a name', () => {
  it('keeps a free name and numbers a taken one before its extension', () => {
    expect(freeName('ops.json', ['eula.txt'])).toBe('ops.json');
    expect(freeName('ops.json', ['ops.json'])).toBe('ops (2).json');
    expect(freeName('ops.json', ['ops.json', 'ops (2).json'])).toBe(
      'ops (3).json',
    );
    expect(freeName('world', ['world'])).toBe('world (2)');
  });

  it('says why a name cannot be used', () => {
    expect(nameIssue('  ', [])).toBe('empty');
    expect(nameIssue('..', [])).toBe('empty');
    expect(nameIssue('a/b', [])).toBe('separator');
    expect(nameIssue('a\\b', [])).toBe('separator');
    expect(nameIssue('world', ['world'])).toBe('taken');
    expect(nameIssue('new.txt', ['world'])).toBeNull();
  });
});
