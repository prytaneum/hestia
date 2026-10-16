/**
 * `modpack.*` — what an entry runs and how its pack moves, with the side named
 * by a target; and `instance.modpack.install` / `server.modpack.install`, a
 * pack into a new or existing entry. Install and update are jobs on the shared
 * `modpack.*` topics; their done event carries the entry the pack landed in,
 * which for a creating install is the only way the caller learns its id.
 */
import type {
  InstalledContent,
  InstalledModpack,
  ModpackFilePreview,
  ModpackMove,
} from '@/api/types';

import { jobIdOf, startJob } from '../job';
import * as content from '../state/content';
import * as entries from '../state/entries';
import { bool, type Handlers, now } from '../support';
import { targetEntry } from './entry';

const PACK_MODS = ['sodium', 'fabric-api', 'lithium'];
const LATEST = '1.5.0';
const FILE_VERSION = '1.6.0-local';

interface Target {
  mode?: string;
  name?: string;
  entry?: string;
}

function pack(name: string, version = '1.4.0'): InstalledModpack {
  const project = content.find('fabulously-optimized');
  return {
    source: project.source,
    projectId: project.id,
    slug: project.slug,
    name,
    versionId: `${project.id}-v0`,
    versionNumber: version,
    gameVersion: '1.21.1',
    loader: 'fabric',
    loaderVersion: '0.16.5',
    iconUrl: project.iconUrl,
    installedUnix: now(),
    files: [],
    overrides: [],
  };
}

/** Where a pack install lands: an existing entry, or one the job creates. */
function landing(
  payload: Record<string, unknown>,
  server: boolean,
): { id: string; name: string } {
  const target = (payload.target ?? {}) as Target;
  if (target.mode === 'existing' && target.entry) {
    const entry = server
      ? entries.findServer(target.entry)
      : entries.findInstance(target.entry);
    return { id: entry.id, name: entry.name };
  }
  const name = target.name || 'Fabulously Optimized';
  const created = server
    ? entries.addServer({ name, flavor: 'fabric', version: '1.21.1' })
    : entries.addInstance({
        name,
        flavor: 'fabric',
        version: '1.21.1',
        loaderVersion: '0.16.5',
      });
  return { id: created.id, name: created.name };
}

function installJob(
  payload: Record<string, unknown>,
  server: boolean,
): { id: string } {
  return startJob({
    id: jobIdOf(payload, 'modpack-install'),
    family: 'modpack',
    steps: [
      { phase: 'resolving', detail: 'reading the pack index' },
      { phase: 'java', detail: 'Java runtime' },
      { phase: 'content', detail: 'pack files' },
      { phase: 'overrides', detail: 'config overrides' },
    ],
    done: () => {
      const where = landing(payload, server);
      const installed: InstalledContent[] = content.install(
        where.id,
        'mod',
        PACK_MODS,
        'modpack:fabulously-optimized',
      );
      const record = pack(where.name);
      record.files = installed.map((item) => item.filename);
      entries.setPack(where.id, record);
      return {
        entry: where.id,
        entryName: where.name,
        pack: record,
        failures: [],
        warnings: [],
      };
    },
  });
}

function updateJob(payload: Record<string, unknown>): { id: string } {
  const entry = targetEntry(payload);
  return startJob({
    id: jobIdOf(payload, 'modpack-update'),
    family: 'modpack',
    steps: [
      { phase: 'resolving', detail: 'the published version' },
      { phase: 'content', detail: 'pack files' },
    ],
    done: () => {
      const to = (payload.to ?? {}) as Partial<ModpackMove>;
      const record = pack(
        entries.packOf(entry.id)?.name ?? entry.name,
        to.path ? FILE_VERSION : to.version || LATEST,
      );
      entries.setPack(entry.id, record);
      return {
        entry: entry.id,
        entryName: entry.name,
        pack: record,
        failures: [],
        warnings: [],
      };
    },
  });
}

/** A picked file is always a newer build of the pack the entry runs. */
function previewUpdate(payload: Record<string, unknown>): ModpackFilePreview {
  const entry = targetEntry(payload);
  const installed = entries.packOf(entry.id);
  return {
    name: installed?.name ?? entry.name,
    versionNumber: FILE_VERSION,
    gameVersion: installed?.gameVersion ?? '',
    samePack: true,
    downgrade: false,
  };
}

function status(payload: Record<string, unknown>) {
  return { pack: entries.packOf(targetEntry(payload).id) };
}

function checkUpdate(payload: Record<string, unknown>) {
  const entry = targetEntry(payload);
  const installed = entries.packOf(entry.id);
  if (!installed?.projectId) return {};
  return {
    update: {
      currentVersionId: installed.versionId,
      currentVersionNumber: installed.versionNumber,
      latestVersionId: `${installed.projectId}-v1`,
      latestVersionNumber: LATEST,
      updatable: installed.versionNumber !== LATEST,
    },
  };
}

function remove(payload: Record<string, unknown>) {
  const entry = targetEntry(payload);
  const keepContent = bool(payload, 'keepContent');
  const origin = `modpack:${entries.packOf(entry.id)?.slug ?? ''}`;
  entries.clearPack(entry.id);
  const none = { removedFiles: 0, removedOverrides: 0, releasedFiles: 0 };
  return keepContent
    ? {
        ...none,
        releasedFiles: content.releaseByOrigin(entry.id, origin),
        kept: [],
      }
    : {
        ...none,
        removedFiles: content.removeByOrigin(entry.id, origin),
        kept: [],
      };
}

export const channels: Handlers = {
  'instance.modpack.install': (p) => installJob(p, false),
  'server.modpack.install': (p) => installJob(p, true),
  'modpack.update': updateJob,
  'modpack.status': status,
  'modpack.preview_update': previewUpdate,
  'modpack.check_update': checkUpdate,
  'modpack.remove': remove,
};
