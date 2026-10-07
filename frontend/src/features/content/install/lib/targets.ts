import { useQuery } from '@tanstack/react-query';
import { useMemo } from 'react';
import type {
  ContentKind,
  ContentProject,
  InstanceInfo,
  ServerInfo,
} from '@/api';
import { m } from '@/paraglide/messages.js';
import { instanceQueries, useInstances } from '@/queries/instance';
import { serverQueries, useServers } from '@/queries/server';

/** An entry the content can be installed into, drawn from every store. */
export interface Target {
  id: string;
  name: string;
  type: 'server' | 'instance';
  flavor: string;
  gameVersion: string;
  running: boolean;
  /** What this entry takes, as the daemon computed it from its flavor. */
  accepts: ContentKind[];
}

export const serverTarget = (s: ServerInfo): Target => ({
  id: s.id,
  name: s.name,
  type: 'server',
  flavor: s.flavor,
  gameVersion: s.gameVersion,
  running: s.process?.state === 'running',
  accepts: s.accepts ?? [],
});

export const instanceTarget = (i: InstanceInfo): Target => ({
  id: i.id,
  name: i.name,
  type: 'instance',
  flavor: i.flavor,
  gameVersion: i.gameVersion,
  running: (i.sessions ?? []).some((s) => s.state === 'running'),
  accepts: i.accepts ?? [],
});

export const targetTakesKind = (t: Target, kind: ContentKind): boolean =>
  t.accepts.includes(kind);

export const entryTypeLabel = (type: Target['type']): string =>
  type === 'server'
    ? m['domain.entry_type.server']()
    : m['domain.entry_type.instance']();

/**
 * A local file staged for import, carrying the daemon's inspection. `kind` is
 * the effective kind (detected, then user-overridable); null means an
 * unrecognised-but-valid archive whose kind must be chosen. `valid` false marks
 * an un-installable file (modpack/corrupt) shown with `reason`.
 */
export interface PickedFile {
  path: string;
  filename: string;
  kind?: ContentKind;
  detected?: ContentKind;
  valid: boolean;
  reason: string;
  /** Set when a source recognises the file; it then installs tracked. */
  project?: ContentProject;
  versionNumber?: string;
}

export const fileName = (path: string) => path.split(/[\\/]/).pop() ?? path;

/** Every entry, from both stores, merged into a common target shape. */
export function useTargets(): Target[] {
  const servers = useServers();
  const instances = useInstances();
  return useMemo(
    () => [
      ...(servers.data ?? []).map(serverTarget),
      ...(instances.data ?? []).map(instanceTarget),
    ],
    [servers.data, instances.data],
  );
}

/**
 * The installed pool of a target, keyed `source:projectId` — the same match the
 * CLI's browse session uses to flag an already-installed hit.
 */
export function useInstalledRefs(
  target: Target,
  kind: ContentKind,
): Set<string> {
  const server = useQuery({
    ...serverQueries.content(target.id, kind),
    enabled: target.type === 'server',
  });
  const instance = useQuery({
    ...instanceQueries.content(target.id, kind),
    enabled: target.type === 'instance',
  });
  const items = (target.type === 'server' ? server : instance).data?.items;
  return useMemo(
    () =>
      new Set(
        (items ?? [])
          .filter((i) => i.projectId)
          .map((i) => `${i.source}:${i.projectId}`),
      ),
    [items],
  );
}
