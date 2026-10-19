/**
 * What a server and an instance answer identically, each a single channel that
 * names its side in an `EntryTarget`: the `entry.*` verbs, the installed
 * content pool (`contentChannels`) and the modpack verbs (./modpack).
 */
import type {
  ContentKind,
  EntryTarget,
  InstalledContent,
  JobSubject,
} from '@/api/types';
import { serverProcessId } from '@/lib/process-id';
import { jobIdOf, startJob, subjectOf } from '../job';
import * as catalog from '../state/catalog';
import * as content from '../state/content';
import * as entries from '../state/entries';
import * as processes from '../state/processes';
import { type ChannelHandlers, ok, str, strings } from '../support';

/** Resolves the entry a payload names to its stable id. */
export type Resolve = (payload: Record<string, unknown>) => string;

function entryOf(target: Partial<EntryTarget> = {}): {
  id: string;
  name: string;
} {
  const entry = target.entry ?? '';
  return target.kind === 'server'
    ? entries.findServer(entry)
    : entries.findInstance(entry);
}

/** The entry a payload's `target` names. */
export function targetEntry(payload: Record<string, unknown>): {
  id: string;
  name: string;
} {
  return entryOf(payload.target as Partial<EntryTarget> | undefined);
}

/** The job subject for the entry a payload's `target` names. */
export function targetSubject(payload: Record<string, unknown>): JobSubject {
  const target = payload.target as Partial<EntryTarget> | undefined;
  return subjectOf(
    target?.kind === 'server' ? 'server' : 'instance',
    targetEntry(payload).id,
  );
}

const kindOf = (payload: Record<string, unknown>): ContentKind =>
  (payload.kind as ContentKind) ?? 'mod';

export function entryChannels(): ChannelHandlers {
  return {
    'entry.flavors': (p) =>
      p.kind === 'server'
        ? { flavors: catalog.serverFlavors, rules: catalog.serverRules }
        : { flavors: catalog.instanceFlavors, rules: [] },
    'entry.versions': (p) => catalog.versionsFor(str(p, 'flavor')),
    'entry.loaders': (p) => catalog.loaders(str(p, 'flavor')),

    'entry.rename': (p) => {
      const { id, name } = entries.rename(entryOf(p.target), str(p, 'name'));
      return { id, name };
    },

    'entry.remove': (p) => {
      const list =
        p.kind === 'server' ? entries.listServers() : entries.listInstances();
      entries.removeEntry(entryOf(p).id, list);
      return ok();
    },

    'entry.stop': (p) => {
      const { id } = entryOf(p.target);
      if (p.target?.kind === 'server') processes.stop(serverProcessId(id));
      else processes.stopSessions(id, p.session || undefined);
      return ok();
    },

    'entry.logs': (p) => {
      const { id } = entryOf(p.target);
      const process =
        p.target?.kind === 'server'
          ? serverProcessId(id)
          : p.session || processes.sessionsOf(id).at(-1)?.id || '';
      return processes.logs(process, p.tail);
    },

    'entry.config.list': (p) => ({
      entries: entries.entrySettings(entryOf(p).id),
      rules: [],
    }),
    'entry.config.set': (p) => {
      const { id } = entryOf(p.target);
      entries.setEntrySetting(id, str(p, 'key'), str(p, 'value'));
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
    subject: targetSubject(payload),
    steps: [
      { phase: 'resolving', detail: 'resolving versions' },
      { phase: 'content', detail: 'downloading' },
      { phase: 'content', detail: 'mirroring' },
    ],
    done: () => ({ items: done(), failures: [] }),
  });
}

export function contentChannels(): ChannelHandlers {
  const resolve = (payload: Record<string, unknown>) => targetEntry(payload).id;
  const add = (p: Record<string, unknown>) => {
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

    'content.matches': () => [],

    'content.check_updates': (p) => content.updates(resolve(p), kindOf(p)),
  };
}
