import { useQuery } from '@tanstack/react-query';
import { Link } from '@tanstack/react-router';
import {
  type ContentKind,
  type ContentSource,
  HestiaError,
  type RequiredSetting,
} from '@/api';
import type { FilterGroup } from '@/components/filter-menu';
import { Badge } from '@/components/ui/badge';
import { settingLocation } from '@/features/settings/lib';
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

function settingLink(setting: RequiredSetting) {
  return { to: '/settings', search: settingLocation(setting) } as const;
}

/**
 * Under a source-unavailable error: a link to where the setting it names is
 * edited. Nothing for any other error, or one that names no setting.
 */
export function SourceSettingLink({ error }: { error: unknown }) {
  if (
    !(error instanceof HestiaError) ||
    error.info?.kind !== 'content_source_unavailable' ||
    !error.info.setting
  ) {
    return null;
  }
  return (
    <Link
      {...settingLink(error.info.setting)}
      className="text-xs underline underline-offset-2"
    >
      {m['content.browse.set_source_setting']({
        setting: error.info.setting.key,
      })}
    </Link>
  );
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
      ...waiting.flatMap((s) =>
        s.requiresSetting
          ? [
              {
                value: s.id,
                label: m['content.browse.source_needs_setting']({
                  name: s.name,
                  setting: s.requiresSetting.key,
                }),
                link: settingLink(s.requiresSetting),
              },
            ]
          : [],
      ),
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
