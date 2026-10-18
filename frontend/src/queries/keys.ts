/**
 * The hierarchical query-key factory. Every cached read gets its key here so
 * invalidation can sweep by prefix: a kind's `all` covers its lists and every
 * entry, and an entry's per-resource keys nest under `detail(id)` so one
 * sweep refreshes the whole entry. Entries are keyed by their **stable id**
 * (never the renameable display name), so a rename cannot strand a cache key.
 */
import type { QueryKey } from '@tanstack/react-query';
import type {
  ContentKind,
  EntryKind,
  EntryTarget,
  SearchQuery,
  VersionQuery,
} from '../api';

/**
 * Root for the server/instance footprint (`info`) queries. Each is a directory
 * walk, so it lives outside its entry's `all`/`detail` subtree: a lifecycle
 * sweep (start/stop/launch) or a reconnect must never trigger the walk. The
 * ops that actually change on-disk size invalidate it explicitly instead.
 */
export const FOOTPRINT = 'footprint';

/** The half of a server's and an instance's key tree that they share. */
function entryKeys<R extends 'servers' | 'instances'>(
  root: R,
  kind: EntryKind,
) {
  const all = [root] as const;
  const detail = (id: string) => [...all, 'detail', id] as const;
  const content = (id: string) => [...detail(id), 'content'] as const;
  return {
    all,
    list: () => [...all, 'list'] as const,
    flavors: () => [...all, 'flavors'] as const,
    versions: (flavor: string) => [...all, 'versions', flavor] as const,
    loaders: (flavor: string, version: string) =>
      [...all, 'loaders', flavor, version] as const,
    detail,
    info: (id: string) => [FOOTPRINT, kind, id] as const,
    config: (id: string) => [...detail(id), 'config'] as const,
    content,
    contentList: (id: string, contentKind: ContentKind) =>
      [...content(id), contentKind] as const,
    contentUpdates: (id: string, contentKind: ContentKind) =>
      [...content(id), 'updates', contentKind] as const,
    contentMatches: (id: string, contentKind: ContentKind, item: string) =>
      [...content(id), 'matches', contentKind, item] as const,
    modpack: (id: string) => [...detail(id), 'modpack'] as const,
  };
}

export const keys = {
  app: {
    all: ['app'] as const,
    info: () => [...keys.app.all, 'info'] as const,
  },
  daemon: {
    all: ['daemon'] as const,
    status: () => [...keys.daemon.all, 'status'] as const,
  },
  host: {
    all: ['host'] as const,
    info: () => [...keys.host.all, 'info'] as const,
  },
  net: {
    all: ['net'] as const,
    status: () => [...keys.net.all, 'status'] as const,
  },
  config: {
    all: ['config'] as const,
    list: () => [...keys.config.all, 'list'] as const,
    value: (key: string) => [...keys.config.all, 'value', key] as const,
  },
  prefs: {
    all: ['prefs'] as const,
    list: () => [...keys.prefs.all, 'list'] as const,
  },
  icons: {
    all: ['icons'] as const,
    list: () => [...keys.icons.all, 'list'] as const,
    config: (id: string) => [...keys.icons.all, 'config', id] as const,
  },
  cache: {
    all: ['cache'] as const,
    info: () => [...keys.cache.all, 'info'] as const,
  },
  accounts: {
    all: ['accounts'] as const,
    list: () => [...keys.accounts.all, 'list'] as const,
  },
  java: {
    all: ['java'] as const,
    releases: () => [...keys.java.all, 'releases'] as const,
    runtimes: () => [...keys.java.all, 'runtimes'] as const,
  },
  servers: {
    ...entryKeys('servers', 'server'),
    ping: (id: string) => [...keys.servers.detail(id), 'ping'] as const,
    logs: (id: string, tail?: number) =>
      [...keys.servers.detail(id), 'logs', tail ?? null] as const,
    properties: (id: string) =>
      [...keys.servers.config(id), 'properties'] as const,
    backups: (id: string) => [...keys.servers.detail(id), 'backups'] as const,
    players: (id: string) => [...keys.servers.detail(id), 'players'] as const,
    complete: (id: string, line: string) =>
      [...keys.servers.detail(id), 'complete', line] as const,
    tasks: (id: string) => [...keys.servers.detail(id), 'tasks'] as const,
    files: (id: string) => [...keys.servers.detail(id), 'files'] as const,
    fileList: (id: string, path: string) =>
      [...keys.servers.files(id), 'list', path] as const,
    fileContent: (id: string, path: string) =>
      [...keys.servers.files(id), 'content', path] as const,
  },
  // An archive is identified by its path, not by any entry — an inspect
  // happens before there is an instance to key against.
  transfer: {
    all: ['transfer'] as const,
    archive: (path: string) => ['transfer', 'archive', path] as const,
  },
  instances: {
    ...entryKeys('instances', 'instance'),
    recent: () => ['instances', 'recent'] as const,
    worlds: (id: string) => [...keys.instances.detail(id), 'worlds'] as const,
    servers: (id: string) => [...keys.instances.detail(id), 'servers'] as const,
    // Keyed by address, not by entry: the same server can sit in several
    // instances' lists, and its status is the same one either way.
    serverStatuses: () => ['minecraft', 'ping'] as const,
    serverStatus: (address: string) =>
      [...keys.instances.serverStatuses(), address] as const,
    logs: (id: string, session?: string, tail?: number) =>
      [
        ...keys.instances.detail(id),
        'logs',
        session ?? null,
        tail ?? null,
      ] as const,
    exportContents: (id: string) =>
      [...keys.instances.detail(id), 'export-contents'] as const,
  },
  content: {
    all: ['content'] as const,
    sources: () => [...keys.content.all, 'sources'] as const,
    search: (query: Partial<SearchQuery>) =>
      [...keys.content.all, 'search', query] as const,
    searchPaged: (kinds: ContentKind[], query: string, source: string) =>
      [...keys.content.all, 'search', 'paged', source, kinds, query] as const,
    project: (source: string, project: string) =>
      [...keys.content.all, 'project', source, project] as const,
    url: (url: string) => [...keys.content.all, 'url', url] as const,
    versions: (query: Partial<VersionQuery>) =>
      [...keys.content.all, 'versions', query] as const,
  },
  skins: {
    all: ['skins'] as const,
    list: (account: string) => [...keys.skins.all, 'list', account] as const,
  },
  screenshots: {
    all: ['screenshots'] as const,
    list: (instance: string) =>
      [...keys.screenshots.all, 'list', instance] as const,
  },
  sync: {
    all: ['sync'] as const,
    config: () => [...keys.sync.all, 'config'] as const,
    status: () => [...keys.sync.all, 'status'] as const,
    options: () => [...keys.sync.all, 'options'] as const,
    sources: (unit: string) => [...keys.sync.all, 'sources', unit] as const,
  },
  update: {
    all: ['update'] as const,
    check: () => [...keys.update.all, 'check'] as const,
  },
};

/** The key scope of one side, for a verb that exists on both. */
export function entryScope(kind: EntryKind) {
  return kind === 'server' ? keys.servers : keys.instances;
}

export function entryTarget(kind: EntryKind, id: string): EntryTarget {
  return { kind, entry: id };
}

/**
 * Is the key a read scoped to one entry — something the daemon can drop while
 * a view still holds its id (removed here, from the CLI, or by the tray)? Such
 * a read answering `not_found` is that id going away, which the surface
 * renders as absence itself, so ./client keeps it out of the toaster rather
 * than stacking one per query.
 */
export function scopedToEntry(key: QueryKey): boolean {
  const [root, second] = key;
  if (root === FOOTPRINT) return true;
  return (root === 'servers' || root === 'instances') && second === 'detail';
}
