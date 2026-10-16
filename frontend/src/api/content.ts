/**
 * The `content.*` browse channels — discovery on a source platform (Modrinth,
 * CurseForge). Installing into an entry lives on `server.content` /
 * `instance.content`. An empty `source` selects the default source; `sources()`
 * answers only those that can serve, so one missing its API key never appears.
 */
import { call } from './core/ipc';
import type {
  ContentAddItem,
  ContentAddSpec,
  ContentInspectResult,
  ContentProject,
  ContentSource,
  ContentVersion,
  ResolvedUrl,
  SearchQuery,
  SearchResult,
  VersionQuery,
} from './types/content';

export type ContentAddInput = Partial<Omit<ContentAddSpec, 'items'>> & {
  items: Partial<ContentAddItem>[];
};

export async function sources(): Promise<ContentSource[]> {
  const result = await call<{ sources: ContentSource[] }>('content.sources');
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
  const result = await call<{ versions: ContentVersion[] }>(
    'content.versions',
    query,
  );
  return result.versions;
}

/** Wire value of `proto::content::LOCAL_SOURCE`. */
export const LOCAL_SOURCE = 'file';

/** Classify a daemon-local file for import (detected kind + validity). */
export function inspect(path: string): Promise<ContentInspectResult> {
  return call('content.inspect', { path });
}
