/**
 * What a server and an instance answer identically. Per-entry settings name
 * their entry with a different parameter (`server` / `instance`), so those
 * handlers are built once and registered twice (see ./instance, ./server). The
 * installed content pool and the modpack verbs name theirs with an
 * `EntryTarget`, and are single channels (see `contentChannels`, ./modpack).
 */
import type { ContentKind, EntryTarget, InstalledContent } from '@/api/types';

import { jobIdOf, startJob } from '../job';
import * as content from '../state/content';
import * as entries from '../state/entries';
import { type Handler, type Handlers, ok, str, strings } from '../support';

/** Resolves the entry a payload names to its stable id. */
export type Resolve = (payload: Record<string, unknown>) => string;

/** The entry a payload's `target` names. */
export function targetEntry(payload: Record<string, unknown>): {
  id: string;
  name: string;
} {
  const { kind, entry = '' } = (payload.target ?? {}) as Partial<EntryTarget>;
  return kind === 'server'
    ? entries.findServer(entry)
    : entries.findInstance(entry);
}

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

function contentJob(
  channel: 'content.add' | 'content.update' | 'content.set_version',
  payload: Record<string, unknown>,
  done: () => InstalledContent[],
): { id: string } {
  return startJob({
    id: jobIdOf(payload, 'content'),
    channel,
    steps: [
      { phase: 'resolving', detail: 'resolving versions' },
      { phase: 'content', detail: 'downloading' },
      { phase: 'content', detail: 'mirroring' },
    ],
    done: () => ({ items: done(), failures: [] }),
  });
}

export function contentChannels(): Handlers {
  const resolve = (payload: Record<string, unknown>) => targetEntry(payload).id;
  const add: Handler = (p) => {
    const id = resolve(p);
    const kind = kindOf(p);
    const items = Array.isArray(p.items)
      ? (p.items as Record<string, unknown>[])
      : [];
    return contentJob('content.add', p, () =>
      items.flatMap((item) => {
        const path = str(item, 'path');
        if (path) return [content.installFile(id, kind, path)];
        const ref = str(item, 'project') || str(item, 'url');
        return content.install(id, kind, [ref]);
      }),
    );
  };

  return {
    'content.list': (p) => content.listPool(resolve(p), kindOf(p)),

    'content.add': add,

    'content.remove': (p) => {
      content.remove(resolve(p), kindOf(p), strings(p, 'items'));
      return ok();
    },

    'content.enable': (p) => {
      content.setEnabled(
        resolve(p),
        kindOf(p),
        str(p, 'item'),
        p.enabled === true,
      );
      return ok();
    },

    'content.update': (p) =>
      contentJob('content.update', p, () =>
        str(p, 'path')
          ? content.updateFromFile(
              resolve(p),
              kindOf(p),
              strings(p, 'items')[0] ?? '',
              str(p, 'path'),
            )
          : content.update(resolve(p), kindOf(p), strings(p, 'items')),
      ),

    'content.set_version': (p) =>
      contentJob('content.set_version', p, () =>
        content.setVersion(
          resolve(p),
          kindOf(p),
          str(p, 'item'),
          str(p, 'version'),
        ),
      ),

    'content.matches': () => ({ matches: [] }),

    'content.check_updates': (p) => ({
      updates: content.updates(resolve(p), kindOf(p)),
    }),
  };
}
