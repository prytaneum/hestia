/**
 * The channels a server and an instance answer identically — per-entry
 * settings and the installed content pool. Both sides name their entry with a
 * different parameter (`server` / `instance`) and nothing else differs, so the
 * handlers are built once and registered twice (see ./instance, ./server).
 */
import type { ContentKind, InstalledContent } from '@/api/types';

import { jobIdOf, startJob } from '../job';
import * as content from '../state/content';
import * as entries from '../state/entries';
import { type Handler, type Handlers, ok, str, strings } from '../support';

/** Resolves the entry a payload names to its stable id. */
export type Resolve = (payload: Record<string, unknown>) => string;

const kindOf = (payload: Record<string, unknown>): ContentKind =>
  (payload.kind as ContentKind) ?? 'mod';

export function configChannels(prefix: string, resolve: Resolve): Handlers {
  return {
    [`${prefix}.config.list`]: (p) => ({
      entries: entries.entrySettings(resolve(p)),
    }),
    [`${prefix}.config.set`]: (p) => {
      entries.setEntrySetting(resolve(p), str(p, 'key'), str(p, 'value'));
      return ok();
    },
  };
}

/** A content job settles on the shared `content.*` topics, keyed by job id. */
function contentJob(
  payload: Record<string, unknown>,
  done: () => InstalledContent[],
): { id: string } {
  return startJob({
    id: jobIdOf(payload, 'content'),
    family: 'content',
    steps: [
      { phase: 'resolving', detail: 'resolving versions' },
      { phase: 'content', detail: 'downloading' },
      { phase: 'content', detail: 'mirroring' },
    ],
    done: () => ({ items: done(), failures: [] }),
  });
}

export function contentChannels(prefix: string, resolve: Resolve): Handlers {
  const add: Handler = (p) => {
    const id = resolve(p);
    const kind = kindOf(p);
    const items = Array.isArray(p.items)
      ? (p.items as Record<string, unknown>[])
      : [];
    return contentJob(p, () =>
      items.flatMap((item) => {
        const path = str(item, 'path');
        if (path) return [content.installFile(id, kind, path)];
        const ref = str(item, 'project') || str(item, 'url');
        return content.install(id, kind, [ref]);
      }),
    );
  };

  return {
    [`${prefix}.content.list`]: (p) => content.listPool(resolve(p), kindOf(p)),

    [`${prefix}.content.add`]: add,

    [`${prefix}.content.remove`]: (p) => {
      content.remove(resolve(p), kindOf(p), strings(p, 'items'));
      return ok();
    },

    [`${prefix}.content.enable`]: (p) => {
      content.setEnabled(
        resolve(p),
        kindOf(p),
        str(p, 'item'),
        p.enabled === true,
      );
      return ok();
    },

    [`${prefix}.content.update`]: (p) =>
      contentJob(p, () =>
        str(p, 'path')
          ? content.updateFromFile(
              resolve(p),
              kindOf(p),
              strings(p, 'items')[0] ?? '',
              str(p, 'path'),
            )
          : content.update(resolve(p), kindOf(p), strings(p, 'items')),
      ),

    [`${prefix}.content.set_version`]: (p) =>
      contentJob(p, () =>
        content.setVersion(
          resolve(p),
          kindOf(p),
          str(p, 'item'),
          str(p, 'version'),
        ),
      ),

    [`${prefix}.content.matches`]: () => ({ matches: [] }),

    [`${prefix}.content.check_updates`]: (p) => ({
      updates: content.updates(resolve(p), kindOf(p)),
    }),
  };
}
