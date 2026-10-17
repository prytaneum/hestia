/**
 * The content installed in an entry, for servers and instances alike: one
 * read per question and one mutation per operation, the side chosen by the
 * `kind` argument. Keys stay per entry and nest under its stable id.
 */
import { queryOptions } from '@tanstack/react-query';
import type {
  ContentAddInput,
  ContentDone,
  ContentKind,
  EntryKind,
} from '../api';
import * as api from '../api/content';
import { mutation } from './core';
import { jobMutation } from './jobs';
import { entryScope } from './keys';

export const entryContentQueries = {
  list: (kind: EntryKind, id: string, contentKind: ContentKind) =>
    queryOptions({
      queryKey: entryScope(kind).contentList(id, contentKind),
      queryFn: () => api.list({ kind, entry: id }, contentKind),
    }),
  matches: (
    kind: EntryKind,
    id: string,
    contentKind: ContentKind,
    item: string,
  ) =>
    queryOptions({
      queryKey: entryScope(kind).contentMatches(id, contentKind, item),
      queryFn: () => api.matches({ kind, entry: id }, contentKind, item),
      enabled: item.length > 0,
    }),
  updates: (kind: EntryKind, id: string, contentKind: ContentKind) =>
    queryOptions({
      queryKey: entryScope(kind).contentUpdates(id, contentKind),
      queryFn: () => api.checkUpdates({ kind, entry: id }, contentKind),
      // A network resolve per item — refetch only when explicitly asked.
      staleTime: Number.POSITIVE_INFINITY,
      enabled: false,
    }),
};

export const entryContentMutations = {
  /** Refused on a running or busy entry. */
  add: (kind: EntryKind, id: string) =>
    jobMutation<ContentDone, ContentAddInput>({
      mutationKey: [...entryScope(kind).content(id), 'add'],
      meta: (spec) => ({
        kind: 'content.add',
        label: `add ${spec.kind}`,
        entry: { kind, id },
      }),
      run: (spec, job) => api.add({ kind, entry: id }, spec, job),
      invalidates: () => invalidates(kind, id),
    }),
  /** One call for the whole batch; a name matching nothing removes none. */
  remove: (kind: EntryKind, id: string) =>
    mutation<void, { kind: ContentKind; items: string[]; worlds?: string[] }>({
      mutationKey: [...entryScope(kind).content(id), 'remove'],
      mutationFn: ({ kind: contentKind, items, worlds }) =>
        api.remove({ kind, entry: id }, contentKind, items, worlds),
      invalidates: () => invalidates(kind, id),
    }),
  /**
   * `items` empty updates every platform-sourced item of the kind; a `path`
   * updates the one named local item from that file.
   */
  update: (kind: EntryKind, id: string) =>
    jobMutation<
      ContentDone,
      { kind: ContentKind; items: string[]; path?: string }
    >({
      mutationKey: [...entryScope(kind).content(id), 'update'],
      meta: ({ kind: contentKind }) => ({
        kind: 'content.update',
        label: `update ${contentKind}s`,
        entry: { kind, id },
      }),
      run: ({ kind: contentKind, items, path = '' }, job) =>
        api.update({ kind, entry: id }, contentKind, items, path, job),
      invalidates: () => invalidates(kind, id),
    }),
  enable: (kind: EntryKind, id: string) =>
    mutation<
      void,
      { kind: ContentKind; item: string; enabled: boolean; worlds?: string[] }
    >({
      mutationKey: [...entryScope(kind).content(id), 'enable'],
      mutationFn: ({ kind: contentKind, item, enabled, worlds }) =>
        api.enable({ kind, entry: id }, contentKind, item, enabled, worlds),
      invalidates: () => invalidates(kind, id),
    }),
  setVersion: (kind: EntryKind, id: string) =>
    jobMutation<
      ContentDone,
      { kind: ContentKind; item: string; version: string; source: string }
    >({
      mutationKey: [...entryScope(kind).content(id), 'set-version'],
      meta: ({ kind: contentKind }) => ({
        kind: 'content.update',
        label: `pin ${contentKind}`,
        entry: { kind, id },
      }),
      run: ({ kind: contentKind, item, version, source }, job) =>
        api.setVersion(
          { kind, entry: id },
          contentKind,
          item,
          version,
          source,
          job,
        ),
      invalidates: () => invalidates(kind, id),
    }),
};

function invalidates(kind: EntryKind, id: string) {
  const scope = entryScope(kind);
  return [scope.content(id), scope.info(id)];
}
