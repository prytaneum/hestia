import type { ContentVersion } from '@/api';

export function filterVersions(
  versions: ContentVersion[],
  search: string,
): ContentVersion[] {
  const q = search.trim().toLowerCase();
  return versions.filter(
    (v) =>
      !q ||
      v.versionNumber.toLowerCase().includes(q) ||
      v.gameVersions.some((g) => g.toLowerCase().includes(q)),
  );
}
