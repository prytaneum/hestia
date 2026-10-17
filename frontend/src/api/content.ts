/**
 * The `content.*` channels. The catalogue half is discovery on a source
 * platform (Modrinth, CurseForge): an empty `source` selects the default
 * source, and `sources()` answers only those that can serve, so one missing its
 * API key never appears. The entry half (`add` … `setVersion`) acts on the
 * content installed in a server or an instance, the side named by an
 * `EntryTarget`; an instance target is refused until an account is signed in.
 */
import { call } from './core/ipc';
import { type JobRun, runJob } from './core/jobs';
import type {
  ContentAddItem,
  ContentAddSpec,
  ContentDone,
  ContentInspectResult,
  ContentKind,
  ContentListResult,
  ContentMatch,
  ContentProject,
  ContentSource,
  ContentUpdate,
  ContentVersion,
  ResolvedUrl,
  SearchQuery,
  SearchResult,
  VersionQuery,
} from './types/content';
import type { EntryTarget } from './types/error';

export type ContentAddInput = Partial<Omit<ContentAddSpec, 'items'>> & {
  items: Partial<ContentAddItem>[];
};

export async function sources(): Promise<ContentSource[]> {
  const result = await call('content.sources');
  return result.sources;
}

export function search(query: Partial<SearchQuery>): Promise<SearchResult> {
  return call('content.search', query);
}

export function project(
  projectId: string,
  source = '',
): Promise<ContentProject> {
  return call('content.project', { source, project: projectId });
}

/** The project (and pinned version) a source page URL names. */
export function resolveUrl(url: string): Promise<ResolvedUrl> {
  return call('content.resolve_url', { url });
}

export async function versions(
  query: Partial<VersionQuery>,
): Promise<ContentVersion[]> {
  const result = await call('content.versions', query);
  return result.versions;
}

/** Wire value of `proto::content::LOCAL_SOURCE`. */
export const LOCAL_SOURCE = 'file';

/** Classify a daemon-local file for import (detected kind + validity). */
export function inspect(path: string): Promise<ContentInspectResult> {
  return call('content.inspect', { path });
}

/** Refused on a running or busy entry. */
export function add(
  target: EntryTarget,
  spec: ContentAddInput,
  job: JobRun,
): Promise<ContentDone> {
  return runJob('content.add', { target, ...spec }, job);
}

export function list(
  target: EntryTarget,
  kind: ContentKind,
): Promise<ContentListResult> {
  return call('content.list', { target, kind });
}

/** Uninstalls the named items in one call; a name matching nothing takes none. */
export async function remove(
  target: EntryTarget,
  kind: ContentKind,
  items: string[],
  worlds: string[] = [],
): Promise<void> {
  await call('content.remove', { target, kind, items, worlds });
}

/**
 * `items` empty updates every platform-sourced item of the kind; a `path`
 * updates the one named local item from that file.
 */
export function update(
  target: EntryTarget,
  kind: ContentKind,
  items: string[],
  path: string,
  job: JobRun,
): Promise<ContentDone> {
  return runJob('content.update', { target, kind, items, path }, job);
}

/** Enable or disable one installed item; applies at the next start. */
export async function enable(
  target: EntryTarget,
  kind: ContentKind,
  item: string,
  enabled: boolean,
  worlds: string[] = [],
): Promise<void> {
  await call('content.enable', { target, kind, item, enabled, worlds });
}

/** Which platform items of the kind have a newer compatible version. */
export async function checkUpdates(
  target: EntryTarget,
  kind: ContentKind,
): Promise<ContentUpdate[]> {
  const result = await call('content.check_updates', { target, kind });
  return result.updates;
}

export async function matches(
  target: EntryTarget,
  kind: ContentKind,
  item: string,
): Promise<ContentMatch[]> {
  const result = await call('content.matches', { target, kind, item });
  return result.matches;
}

/** Re-pin one item to a specific published version (id or number). */
export function setVersion(
  target: EntryTarget,
  kind: ContentKind,
  item: string,
  version: string,
  source: string,
  job: JobRun,
): Promise<ContentDone> {
  return runJob(
    'content.set_version',
    { target, kind, item, version, source },
    job,
  );
}
