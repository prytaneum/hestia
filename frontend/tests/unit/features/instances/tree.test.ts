/**
 * The export dialog's two conversions: the daemon's flat listing into a tree,
 * and the checkboxes back into exclusion paths. Both are worth pinning — a
 * mistake here silently ships (or drops) somebody's worlds, and neither shows
 * up as a type error.
 */
import { describe, expect, it } from 'vitest';
import type { ArchiveEntry } from '@/api';
import {
  buildTree,
  excludedRoots,
  selectedBytes,
} from '@/features/instances/lib';

function entry(path: string, sizeBytes: number, directory = false): ArchiveEntry {
  return {
    path,
    name: path.split('/').pop() ?? path,
    directory,
    sizeBytes,
    referenced: false,
  };
}

/** A listing shaped like the daemon's: sorted by path, directories included. */
const LISTING: ArchiveEntry[] = [
  entry('content.json', 100),
  entry('data', 3000, true),
  entry('data/options.txt', 200),
  entry('data/saves', 2800, true),
  entry('data/saves/New World', 2000, true),
  entry('data/saves/Testing', 800, true),
  entry('mods', 5000, true),
  entry('mods/sodium.jar', 5000),
];

describe('buildTree', () => {
  it('nests each node under its parent', () => {
    const tree = buildTree(LISTING);

    expect(tree.map((n) => n.path)).toEqual(['content.json', 'data', 'mods']);
    const data = tree[1];
    expect(data.children.map((n) => n.path)).toEqual([
      'data/options.txt',
      'data/saves',
    ]);
    expect(data.children[1].children.map((n) => n.name)).toEqual([
      'New World',
      'Testing',
    ]);
  });

  it('keeps a node whose parent is missing rather than dropping it', () => {
    const tree = buildTree([entry('data/orphan.txt', 10)]);
    expect(tree.map((n) => n.path)).toEqual(['data/orphan.txt']);
  });

  it('has nothing to build from an empty listing', () => {
    expect(buildTree([])).toEqual([]);
  });
});

describe('excludedRoots', () => {
  it('sends only the topmost unchecked path', () => {
    const excluded = new Set([
      'data/saves',
      'data/saves/Testing',
      'mods/sodium.jar',
    ]);
    expect(excludedRoots(excluded)).toEqual(['data/saves', 'mods/sodium.jar']);
  });

  it('does not mistake a name prefix for a parent', () => {
    const excluded = new Set(['data/save', 'data/saves']);
    expect(excludedRoots(excluded)).toEqual(['data/save', 'data/saves']);
  });

  it('sends nothing when everything is checked', () => {
    expect(excludedRoots(new Set())).toEqual([]);
  });
});

describe('selectedBytes', () => {
  const tree = buildTree(LISTING);

  it('totals every node when nothing is excluded', () => {
    expect(selectedBytes(tree, new Set())).toBe(8100);
  });

  it('subtracts an excluded leaf from its ancestors', () => {
    expect(selectedBytes(tree, new Set(['data/saves/Testing']))).toBe(7300);
  });

  it('takes a whole subtree out with its directory', () => {
    expect(selectedBytes(tree, new Set(['data/saves']))).toBe(5300);
    expect(selectedBytes(tree, new Set(['data']))).toBe(5100);
  });

  it('counts nothing when every root is excluded', () => {
    const all = new Set(['content.json', 'data', 'mods']);
    expect(selectedBytes(tree, all)).toBe(0);
  });
});
