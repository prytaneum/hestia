/**
 * `instance.export.*` and `instance.import.*` — moving an instance out as one
 * file and bringing one back. Both are jobs; `inspect` and `contents` are the
 * cheap reads a dialog runs before either starts.
 */
import type { ArchiveEntry, ArchiveInfo, ImportFormat } from '@/api/types';

import { jobIdOf, startJob } from '../job';
import * as entries from '../state/entries';
import { type Handlers, str } from '../support';

const TREE: ArchiveEntry[] = [
  { path: 'config', name: 'config', directory: true, sizeBytes: 2_400_000 },
  { path: 'mods', name: 'mods', directory: true, sizeBytes: 46_000_000 },
  {
    path: 'options.txt',
    name: 'options.txt',
    directory: false,
    sizeBytes: 3_100,
  },
  {
    path: 'resourcepacks',
    name: 'resourcepacks',
    directory: true,
    sizeBytes: 18_000_000,
  },
  {
    path: 'saves/New World',
    name: 'New World',
    directory: true,
    sizeBytes: 128_000_000,
  },
  {
    path: 'saves/creative-flats',
    name: 'creative-flats',
    directory: true,
    sizeBytes: 12_000_000,
  },
];

const formatOf = (path: string): ImportFormat => {
  if (path.endsWith('.mrpack')) return 'mrpack';
  if (path.endsWith('.hestia')) return 'hestia';
  if (path.includes('curseforge')) return 'curseforge';
  return 'prism';
};

export const channels: Handlers = {
  'instance.export.contents': (p) => {
    entries.findInstance(str(p, 'instance'));
    return { entries: TREE };
  },

  'instance.import.inspect': (p) => {
    const path = str(p, 'path');
    const name = (path.split(/[/\\]/).pop() ?? 'archive').replace(
      /\.[^.]+$/,
      '',
    );
    const info: ArchiveInfo = {
      format: formatOf(path),
      name,
      gameVersion: '1.21.1',
      loader: 'fabric',
      loaderVersion: '0.16.5',
      nameTaken: entries
        .listInstances()
        .some((instance) => instance.name === name),
    };
    return info;
  },

  'instance.export': (p) => {
    const instance = entries.findInstance(str(p, 'instance'));
    const format = str(p, 'format', 'hestia');
    const destination = str(
      p,
      'destination',
      `${entries.HOME}/exports/${instance.id}.${format}`,
    );
    const excluded = Array.isArray(p.exclude) ? p.exclude.length : 0;
    return startJob({
      id: jobIdOf(p, 'instance-export'),
      family: 'instance.export',
      steps: [
        { phase: 'resolving', detail: instance.name },
        { phase: 'archive', detail: 'writing the archive' },
      ],
      done: () => ({
        path: destination,
        sizeBytes: 206_000_000,
        files: TREE.length - excluded,
        warnings: [],
      }),
    });
  },

  'instance.import': (p) => {
    const path = str(p, 'path');
    const name =
      str(p, 'name') ||
      (path.split(/[/\\]/).pop() ?? 'Imported').replace(/\.[^.]+$/, '');
    return startJob({
      id: jobIdOf(p, 'instance-import'),
      family: 'instance.import',
      steps: [
        { phase: 'resolving', detail: 'reading the archive' },
        { phase: 'extract', detail: 'extracting' },
        { phase: 'content', detail: 'installing content' },
      ],
      done: () => ({
        format: formatOf(path),
        instance: entries.addInstance({
          name,
          flavor: 'fabric',
          version: '1.21.1',
          loaderVersion: '0.16.5',
        }),
        failures: [],
        warnings: [],
      }),
    });
  },
};
