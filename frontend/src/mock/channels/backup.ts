/**
 * `server.backup.*`. Create and restore are jobs; the list is per server,
 * newest first, so a create shows up where the tab expects it.
 */
import type { BackupInfo } from '@/api/types';

import { jobIdOf, startJob, subjectOf } from '../job';
import { ago, type ChannelHandlers, now, ok, str } from '../support';
import type { Resolve } from './entry';

const backups = new Map<string, BackupInfo[]>([
  [
    'smp',
    [
      {
        id: '2026-07-30-0400',
        kind: 'scheduled',
        createdUnix: ago(86_400),
        size: 220 * 1024 * 1024,
      },
      {
        id: '2026-07-24-1812',
        kind: 'update',
        createdUnix: ago(86_400 * 7),
        size: 198 * 1024 * 1024,
      },
    ],
  ],
]);

function listOf(id: string): BackupInfo[] {
  const existing = backups.get(id);
  if (existing) return existing;
  const fresh: BackupInfo[] = [];
  backups.set(id, fresh);
  return fresh;
}

const stamp = (): string =>
  new Date().toISOString().slice(0, 16).replace(/[-:T]/g, '').slice(0, 12);

export function channels(resolve: Resolve): ChannelHandlers {
  const job = (
    payload: Record<string, unknown>,
    channel: 'server.backup.create' | 'server.backup.restore',
    phase: 'backup' | 'extract',
    done: () => BackupInfo,
  ) =>
    startJob({
      id: jobIdOf(payload, 'server-backup'),
      channel,
      subject: subjectOf('server', resolve(payload)),
      steps: [
        { phase: 'resolving', detail: 'pausing world saves' },
        { phase, detail: 'world' },
        { phase: 'archive', detail: 'writing the archive' },
      ],
      done: () => ({ backup: done() }),
    });

  return {
    'server.backup.list': (p) => listOf(resolve(p)),

    'server.backup.create': (p) =>
      job(p, 'server.backup.create', 'backup', () => {
        const backup: BackupInfo = {
          id: stamp(),
          kind: 'manual',
          createdUnix: now(),
          size: 214 * 1024 * 1024,
        };
        listOf(resolve(p)).unshift(backup);
        return backup;
      }),

    'server.backup.restore': (p) =>
      job(p, 'server.backup.restore', 'extract', () => {
        const wanted = str(p, 'backup');
        const found = listOf(resolve(p)).find((entry) => entry.id === wanted);
        return (
          found ?? {
            id: wanted,
            kind: 'manual',
            createdUnix: now(),
            size: 0,
          }
        );
      }),

    'server.backup.remove': (p) => {
      const list = listOf(resolve(p));
      const at = list.findIndex((entry) => entry.id === str(p, 'backup'));
      if (at >= 0) list.splice(at, 1);
      return ok();
    },
  };
}
