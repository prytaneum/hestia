/**
 * The per-entry content mutations, shared by servers and instances. The five
 * operations (`add`/`remove`/`update`/`enable`/`setVersion`) are identical
 * across both kinds bar the entry tag, the api namespace and the key prefixes —
 * so they live once here and each domain file calls `entryContentFactories`
 * with its own config.
 */
import type { QueryKey } from '@tanstack/react-query';
import type {
  ContentAddInput,
  ContentDoneEvent,
  ContentKind,
  JobRun,
} from '../api';
import { mutation } from './core';
import { type JobEntryKind, jobMutation } from './jobs';

/** The content api surface both `serverApi.content` and `instanceApi.content` satisfy. */
interface EntryContentApi {
  add(
    id: string,
    spec: ContentAddInput,
    job: JobRun,
  ): Promise<ContentDoneEvent>;
  remove(
    id: string,
    kind: ContentKind,
    items: string[],
    worlds?: string[],
  ): Promise<void>;
  update(
    id: string,
    kind: ContentKind,
    items: string[],
    path: string,
    job: JobRun,
  ): Promise<ContentDoneEvent>;
  enable(
    id: string,
    kind: ContentKind,
    item: string,
    enabled: boolean,
    worlds?: string[],
  ): Promise<void>;
  setVersion(
    id: string,
    kind: ContentKind,
    item: string,
    version: string,
    source: string,
    job: JobRun,
  ): Promise<ContentDoneEvent>;
}

interface EntryContentConfig {
  /** The entry tag jobs carry, so an activity surface groups by entry. */
  kind: JobEntryKind;
  api: EntryContentApi;
  /** The entry's content key prefix (`keys.<kind>.content`). */
  contentKey: (id: string) => QueryKey;
  /** The entry's footprint key (`keys.<kind>.info`). */
  infoKey: (id: string) => QueryKey;
}

/** The `content.*` mutation factories for one entry kind. */
export function entryContentFactories(cfg: EntryContentConfig) {
  const invalidates = (id: string): QueryKey[] => [
    cfg.contentKey(id),
    cfg.infoKey(id),
  ];
  return {
    /** Refused on a running or busy entry. */
    add: (id: string) =>
      jobMutation<ContentDoneEvent, ContentAddInput>({
        mutationKey: [...cfg.contentKey(id), 'add'],
        meta: (spec) => ({
          kind: 'content.add',
          label: `add ${spec.kind}`,
          entry: { kind: cfg.kind, id },
        }),
        run: (spec, job) => cfg.api.add(id, spec, job),
        invalidates: () => invalidates(id),
      }),
    /** One call for the whole batch; a name matching nothing removes none. */
    remove: (id: string) =>
      mutation<void, { kind: ContentKind; items: string[]; worlds?: string[] }>(
        {
          mutationKey: [...cfg.contentKey(id), 'remove'],
          mutationFn: ({ kind, items, worlds }) =>
            cfg.api.remove(id, kind, items, worlds),
          invalidates: () => invalidates(id),
        },
      ),
    /**
     * `items` empty updates every platform-sourced item of the kind; a `path`
     * updates the one named local item from that file.
     */
    update: (id: string) =>
      jobMutation<
        ContentDoneEvent,
        { kind: ContentKind; items: string[]; path?: string }
      >({
        mutationKey: [...cfg.contentKey(id), 'update'],
        meta: ({ kind }) => ({
          kind: 'content.update',
          label: `update ${kind}s`,
          entry: { kind: cfg.kind, id },
        }),
        run: ({ kind, items, path = '' }, job) =>
          cfg.api.update(id, kind, items, path, job),
        invalidates: () => invalidates(id),
      }),
    enable: (id: string) =>
      mutation<
        void,
        { kind: ContentKind; item: string; enabled: boolean; worlds?: string[] }
      >({
        mutationKey: [...cfg.contentKey(id), 'enable'],
        mutationFn: ({ kind, item, enabled, worlds }) =>
          cfg.api.enable(id, kind, item, enabled, worlds),
        invalidates: () => invalidates(id),
      }),
    setVersion: (id: string) =>
      jobMutation<
        ContentDoneEvent,
        { kind: ContentKind; item: string; version: string; source: string }
      >({
        mutationKey: [...cfg.contentKey(id), 'set-version'],
        meta: ({ kind }) => ({
          kind: 'content.update',
          label: `pin ${kind}`,
          entry: { kind: cfg.kind, id },
        }),
        run: ({ kind, item, version, source }, job) =>
          cfg.api.setVersion(id, kind, item, version, source, job),
        invalidates: () => invalidates(id),
      }),
  };
}
