import { describe, expect, it } from 'vitest';
import {
  contentKinds,
  isContentKind,
  kindBySlug,
  kindInfo,
} from '@/features/shared/content/lib';
import { isContentUrl } from '@/queries/content';

describe('isContentKind', () => {
  it('refuses anything else a search param could carry', () => {
    expect(isContentKind('mods')).toBe(false);
    expect(isContentKind('')).toBe(false);
    expect(isContentKind(undefined)).toBe(false);
    expect(isContentKind(1)).toBe(false);
  });
});

describe('kindBySlug', () => {
  it('round-trips every kind through its route slug', () => {
    for (const kind of contentKinds) {
      expect(kindBySlug(kindInfo[kind].slug)).toBe(kind);
    }
  });
});

describe('isContentUrl', () => {
  it('recognises a pasted project link', () => {
    expect(isContentUrl('https://modrinth.com/mod/sodium')).toBe(true);
    expect(isContentUrl('  http://example.com/x  ')).toBe(true);
  });

  it('treats an ordinary search as a search', () => {
    expect(isContentUrl('sodium')).toBe(false);
    expect(isContentUrl('modrinth.com/mod/sodium')).toBe(false);
    expect(isContentUrl('')).toBe(false);
  });

  it('refuses a link with whitespace inside it', () => {
    expect(isContentUrl('https://example.com/a b')).toBe(false);
  });
});
