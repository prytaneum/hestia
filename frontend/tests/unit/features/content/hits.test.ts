import { describe, expect, it } from 'vitest';
import type { ContentProject, SearchResult } from '@/api';
import { mergeHits, projectKey } from '@/features/content/lib';

const project = (
  id: string,
  downloads: number,
  source = 'modrinth',
): ContentProject =>
  ({ id, source, downloads, slug: id, title: id }) as ContentProject;

const page = (...hits: ContentProject[]): SearchResult[] =>
  [{ hits }] as SearchResult[];

describe('mergeHits', () => {
  it('orders by downloads within a page', () => {
    const merged = mergeHits([page(project('a', 10), project('b', 300))]);
    expect(merged.map((p) => p.id)).toEqual(['b', 'a']);
  });

  it('appends a later page instead of reranking what is on screen', () => {
    const merged = mergeHits([
      page(project('a', 10), project('b', 300)),
      page(project('c', 200)),
    ]);
    expect(merged.map((p) => p.id)).toEqual(['b', 'a', 'c']);
  });

  it('lists a project once when pages overlap', () => {
    const merged = mergeHits([
      page(project('a', 10)),
      page(project('a', 10), project('b', 5)),
    ]);
    expect(merged.map((p) => p.id)).toEqual(['a', 'b']);
  });

  it('keeps the same id from two sources apart', () => {
    const merged = mergeHits([
      page(project('sodium', 10, 'modrinth'), project('sodium', 9, 'curseforge')),
    ]);
    expect(merged).toHaveLength(2);
    expect(new Set(merged.map(projectKey)).size).toBe(2);
  });

  it('merges the fan-out over several kinds within one page', () => {
    const merged = mergeHits([
      [{ hits: [project('a', 1)] }, { hits: [project('b', 2)] }],
    ] as SearchResult[][]);
    expect(merged.map((p) => p.id)).toEqual(['b', 'a']);
  });
});
