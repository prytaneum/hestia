/**
 * Event-driven cache freshness: daemon topics map to the key prefixes they
 * outdate, so lists stay live even for changes this window didn't make (the
 * CLI, the tray, a schedule). Mutations invalidate their own keys on settle
 * as well — this feed is the cross-client half. A reconnect invalidates
 * everything: the daemon may have changed while we were away.
 */
import type { QueryKey } from '@tanstack/react-query';
import {
  type DaemonEvent,
  type Jobs,
  onConnectionChange,
  onDaemonEvent,
  type Topics,
} from '../api';
import { entryOfProcess } from '../lib/process-id';
import { invalidate, queryClient } from './client';
import { FOOTPRINT, keys } from './keys';

// Running state shows in both the entry's list row and its detail status, so a
// lifecycle event refreshes those two — never the whole `all` subtree (other
// entries, the footprint walk).
function processKeys(payload: { id: string }): QueryKey[] {
  const server = entryOfProcess(payload.id, 'server');
  if (server) return [keys.servers.list(), keys.servers.detail(server)];
  const instance = entryOfProcess(payload.id, 'instance');
  // A session's exit is when what it joined is folded into the recent list.
  if (instance)
    return [
      keys.instances.list(),
      keys.instances.recent(),
      keys.instances.detail(instance),
    ];
  return [];
}

/** A list refresh, plus the named entry's detail when it is known. */
function entryKeys(
  kind: 'servers' | 'instances',
  id: string | null,
): QueryKey[] {
  const out: QueryKey[] = [keys[kind].list()];
  if (id) out.push(keys[kind].detail(id));
  return out;
}

type JobKeys = { [K in keyof Jobs]?: (done: Jobs[K]['done']) => QueryKey[] };

// The job event carries only a job id, so the entry is unknowable cross-client;
// sweep the whole entry subtree (refreshes an open Content/Backups tab). The
// footprint walk lives outside `all` and is never triggered.
const sweepServers = (): QueryKey[] => [keys.servers.all];
const sweepEntries = (): QueryKey[] => [keys.servers.all, keys.instances.all];

const JOB_DONE: JobKeys = {
  'server.create': () => [keys.servers.list()],
  'server.update': (done) => entryKeys('servers', done.server.id),
  'instance.update': (done) => entryKeys('instances', done.instance.id),
  'instance.launch': (done) =>
    entryKeys('instances', entryOfProcess(done.processId, 'instance')),
  'java.install': () => [keys.java.runtimes()],
  'server.backup.create': sweepServers,
  'server.backup.restore': sweepServers,
  'content.add': sweepEntries,
  'content.update': sweepEntries,
  'content.set_version': sweepEntries,
  // A pack install can have *created* its entry, so neither list is known to
  // be current — and the CLI or another window may have run it.
  'instance.modpack.install': sweepEntries,
  'server.modpack.install': sweepEntries,
  'modpack.update': sweepEntries,
  // An import creates an instance; an export changes the data home's
  // footprint when it lands in the daemon's own exports/.
  'instance.import': () => [keys.instances.all],
  'instance.export': () => [[FOOTPRINT]],
};

const JOB_FAILED: { [K in keyof Jobs]?: () => QueryKey[] } = {
  // A failed create removes the half-provisioned record.
  'server.create': () => [keys.servers.list()],
};

function doneKeys<K extends keyof Jobs>(
  channel: K,
  done: Jobs[K]['done'],
): QueryKey[] {
  return JOB_DONE[channel]?.(done) ?? [];
}

function jobKeys(event: Topics['job']): QueryKey[] {
  if (event.state === 'done') return doneKeys(event.channel, event);
  if (event.state === 'failed') return JOB_FAILED[event.channel]?.() ?? [];
  return [];
}

// A plain call names only the scope it changed, never an entry, so a scope
// refreshes everything under it — as an entry-wide job does. The footprint walk
// stays out of `Entries` for the same reason it stays out of a job sweep.
const CHANGED: Record<Topics['changed']['scope'], QueryKey[]> = {
  entries: [keys.servers.all, keys.instances.all],
  accounts: [keys.accounts.all, keys.skins.all],
  skins: [keys.skins.all],
  config: [keys.config.all, keys.content.all],
  java: [keys.java.all],
  sync: [keys.sync.all],
  screenshots: [keys.screenshots.all],
  cache: [keys.cache.all, [FOOTPRINT]],
};

/** The key prefixes a daemon event outdates — exported for the regression test. */
export function invalidationKeys(event: DaemonEvent): QueryKey[] {
  switch (event.topic) {
    case 'process.started':
    case 'process.exit':
      return processKeys(event.payload);
    case 'job':
      return jobKeys(event.payload);
    case 'changed':
      return CHANGED[event.payload.scope];
    default:
      return [];
  }
}

// The queries whose answer depends on reaching upstream. The version lists are
// named by their own prefix rather than the entry subtree they nest under: a
// reconnect should refresh the catalogues, not walk every entry's footprint.
const UPSTREAM_KEYS: QueryKey[] = [
  keys.content.all,
  keys.java.all,
  keys.update.all,
  keys.skins.all,
  [...keys.servers.all, 'versions'],
  [...keys.instances.all, 'versions'],
];

let wasOffline = false;

function sweepOnReturnToOnline(status: Topics['net.state']): void {
  const offline = status.state === 'offline';
  if (wasOffline && !offline) for (const key of UPSTREAM_KEYS) invalidate(key);
  wasOffline = offline;
}

let started = false;

/** Install the feed once, at app bootstrap. */
export function startInvalidation(): void {
  if (started) return;
  started = true;
  onDaemonEvent((event) => {
    for (const key of invalidationKeys(event)) invalidate(key);
    // Coming back online is the other reconnect: everything upstream-backed
    // failed or answered from a cached catalogue while we were out.
    if (event.topic === 'net.state') sweepOnReturnToOnline(event.payload);
  }).catch(() => {
    // Outside the Tauri shell there are no daemon events to hear.
  });
  onConnectionChange((state) => {
    // The daemon may have changed while we were away, so refresh everything —
    // except the footprint walks, which don't drift on their own and are
    // expensive; they refetch on next mount if stale.
    if (state === 'connected')
      void queryClient.invalidateQueries({
        predicate: (query) => query.queryKey[0] !== FOOTPRINT,
      });
  }).catch(() => {
    // Same: no bridge, nothing to watch.
  });
}
