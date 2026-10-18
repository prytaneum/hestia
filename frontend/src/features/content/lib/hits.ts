import type { ContentProject, SearchResult } from '@/api';

export const sourceRef = (source: string, id: string) => `${source}:${id}`;

export const projectKey = (project: ContentProject) =>
  sourceRef(project.source, project.id);

export function mergeHits(
  pages: SearchResult[][] | undefined,
): ContentProject[] {
  const seen = new Set<string>();
  const feed: ContentProject[] = [];

  for (const page of pages ?? []) {
    const batch: ContentProject[] = [];
    for (const result of page) {
      for (const hit of result.hits) {
        const id = projectKey(hit);
        if (seen.has(id)) continue;
        seen.add(id);
        batch.push(hit);
      }
    }
    batch.sort((a, b) => b.downloads - a.downloads);
    feed.push(...batch);
  }

  return feed;
}
