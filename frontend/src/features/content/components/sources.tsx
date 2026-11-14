import { useQuery } from '@tanstack/react-query';
import type { ContentKind, ContentSource } from '@/api';
import type { FilterGroup } from '@/components/filter-menu';
import { Badge } from '@/components/ui/badge';
import { m } from '@/paraglide/messages.js';
import { contentQueries } from '@/queries/content';

/**
 * The sources a kind can be browsed on. `list` is the ones that can serve, so
 * this never offers one that would come back empty; `waiting` is the ones that
 * need a setting first, which the filter shows but cannot select.
 */
export function useSourceOptions(kind: ContentKind | undefined, value: string) {
  const sources = useQuery(contentQueries.sources());
  const ofKind = (sources.data ?? []).filter(
    (s) => !kind || s.kinds.includes(kind),
  );
  const list = ofKind.filter((s) => !s.requiresSetting);
  const waiting = ofKind.filter((s) => s.requiresSetting);
  // A source carried over from another kind may not catalogue this one; fall
  // back to the default rather than searching a source with nothing to answer.
  const active = list.some((s) => s.id === value) ? value : '';
  return { list, waiting, active };
}

/** The source dimension; absent when there is nothing to choose between. */
export function sourceGroup(
  list: ContentSource[],
  active: string,
  onChange: (source: string) => void,
  waiting: ContentSource[] = [],
): FilterGroup | undefined {
  if (list.length + waiting.length < 2 || list.length === 0) return undefined;
  const [first] = list;
  return {
    label: m['content.browse.source'](),
    value: active || first.id,
    neutral: first.id,
    options: [
      ...list.map((s) => ({ value: s.id, label: s.name })),
      ...waiting.map((s) => ({
        value: s.id,
        label: m['content.browse.source_needs_setting']({
          name: s.name,
          setting: s.requiresSetting?.key ?? '',
        }),
        disabled: true,
      })),
    ],
    onChange,
  };
}

/** Which platform a project came from — only worth saying with more than one. */
export function SourceBadge({ source }: { source: string }) {
  const sources = useQuery(contentQueries.sources());
  const list = sources.data ?? [];
  if (!source || list.length < 2) return null;
  return (
    <Badge variant="outline">
      {list.find((s) => s.id === source)?.name ?? source}
    </Badge>
  );
}
