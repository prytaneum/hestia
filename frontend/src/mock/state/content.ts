/**
 * Content: the platform catalogue a browse answers from, and the installed
 * pool each entry carries. The catalogue is fixed; the pools are the mutable
 * half — an add, an enable, a re-pin and a removal all land here, so the
 * content tab reflects what was just done to it.
 */
import type {
  ContentKind,
  ContentProject,
  ContentUpdate,
  ContentVersion,
  InstalledContent,
  ResolvedModpack,
  ResolvedUrl,
  SearchResult,
  UntrackedFile,
} from '@/api/types';

import { ago, fail, now } from '../support';

const ICON = (id: string, hash: string) =>
  `https://cdn.modrinth.com/data/${id}/${hash}`;

function project(
  partial: Pick<
    ContentProject,
    'id' | 'slug' | 'kind' | 'title' | 'author' | 'description' | 'iconUrl'
  > &
    Partial<ContentProject>,
): ContentProject {
  return {
    source: 'modrinth',
    kinds: [partial.kind],
    body: `## ${partial.title}\n\n${partial.description}`,
    categories: ['optimization'],
    downloads: 1_200_000,
    follows: 4_300,
    gallery: [],
    clientSide: 'required',
    serverSide: 'optional',
    ...partial,
  };
}

const catalogue: ContentProject[] = [
  project({
    id: 'AANobbMI',
    slug: 'sodium',
    kind: 'mod',
    title: 'Sodium',
    author: 'jellysquid3',
    description:
      'A high-performance rendering engine replacement for Minecraft, which greatly improves frame rates and reduces micro-stutter.',
    iconUrl: ICON(
      'AANobbMI',
      '295862f4724dc3f78df3447ad6072b2dcd3ef0c9_96.webp',
    ),
    downloads: 196_995_125,
    follows: 39_314,
  }),
  project({
    id: 'P7dR8mSH',
    slug: 'fabric-api',
    kind: 'mod',
    title: 'Fabric API',
    author: 'modmuss50',
    description:
      'Lightweight and modular API providing common hooks and intercompatibility measures utilized by mods using the Fabric toolchain.',
    iconUrl: ICON('P7dR8mSH', 'icon.png'),
    categories: ['library'],
    downloads: 220_119_403,
    follows: 34_535,
    serverSide: 'required',
  }),
  project({
    id: 'gvQqBUqZ',
    slug: 'lithium',
    kind: 'mod',
    title: 'Lithium',
    author: 'jellysquid3',
    description:
      'No-compromises game logic optimization mod, useful for both single-player games and multi-player servers.',
    iconUrl: ICON(
      'gvQqBUqZ',
      'bcc8686c13af0143adf4285d741256af824f70b7_96.webp',
    ),
    downloads: 113_567_219,
    follows: 22_797,
    serverSide: 'required',
  }),
  project({
    id: 'YL57xq9U',
    slug: 'iris',
    kind: 'mod',
    title: 'Iris Shaders',
    author: 'coderbot',
    description:
      'A modern shader pack loader for Minecraft intended to be compatible with existing OptiFine shader packs.',
    iconUrl: ICON(
      'YL57xq9U',
      '18d0e7f076d3d6ed5bedd472b853909aac5da202_96.webp',
    ),
    categories: ['decoration', 'optimization'],
    downloads: 153_625_014,
    follows: 28_267,
    serverSide: 'unsupported',
  }),
  project({
    id: 'HVnmMxH1',
    slug: 'complementary-reimagined',
    kind: 'shader',
    title: 'Complementary Shaders — Reimagined',
    author: 'EminGT',
    description:
      'Preserving the elements of Minecraft with exceptional quality, detail, and performance.',
    iconUrl: ICON(
      'HVnmMxH1',
      '79cb7c8123bbc54945305b2ebad6b8881efdf5f8_96.webp',
    ),
    categories: ['colored-lighting', 'vanilla-like'],
    downloads: 58_971_667,
    follows: 10_348,
    serverSide: 'unsupported',
  }),
  project({
    id: 'w0TnApzs',
    slug: 'faithful-32x',
    kind: 'resource_pack',
    title: 'Faithful 32x',
    author: 'Faithful Team',
    description:
      'The original Minecraft texture feel, with double the resolution and double the fun!',
    iconUrl: ICON('w0TnApzs', 'e8403d1fb2f55321ae74402c1e8c90a3a5670856.png'),
    categories: ['32x', 'vanilla-like'],
    downloads: 4_129_699,
    follows: 1_432,
  }),
  project({
    id: '8oi3bsk5',
    slug: 'terralith',
    kind: 'data_pack',
    title: 'Terralith',
    author: 'Starmute',
    description:
      'Explore almost 100 new biomes consisting of both realism and light fantasy, using just Vanilla blocks.',
    iconUrl: ICON(
      '8oi3bsk5',
      '1959d924a1088944bbf07a06ba523726112d7e7a_96.webp',
    ),
    categories: ['worldgen'],
    downloads: 20_467_954,
    follows: 8_139,
    serverSide: 'required',
  }),
  project({
    id: 'hXiIvTyT',
    slug: 'essentialsx',
    kind: 'plugin',
    title: 'EssentialsX',
    author: 'EssentialsX Team',
    description: 'The essential plugin suite for Paper and Spigot servers.',
    iconUrl: ICON(
      'hXiIvTyT',
      'e621675be1d0421b43b65ab8082507532d937009_96.webp',
    ),
    categories: ['economy', 'social', 'utility'],
    downloads: 680_449,
    follows: 760,
    clientSide: 'unsupported',
    serverSide: 'required',
  }),
  project({
    id: '1KVo5zza',
    slug: 'fabulously-optimized',
    kind: 'modpack',
    title: 'Fabulously Optimized',
    author: 'Robotkoer',
    description:
      'Beautiful graphics, speedy performance and familiar features in a simple package.',
    iconUrl: ICON(
      '1KVo5zza',
      'd8152911f8fd5d7e9a8c499fe89045af81fe816e_96.webp',
    ),
    categories: ['lightweight', 'multiplayer', 'optimization'],
    downloads: 15_230_434,
    follows: 4_668,
  }),
];

export function search(
  kind: ContentKind | undefined,
  query: string,
  limit: number,
  offset: number,
): SearchResult {
  const needle = query.trim().toLowerCase();
  const matched = catalogue.filter(
    (entry) =>
      (!kind || entry.kind === kind) &&
      (needle === '' ||
        entry.title.toLowerCase().includes(needle) ||
        entry.slug.includes(needle)),
  );
  return {
    hits: matched.slice(offset, offset + limit),
    offset,
    limit,
    total: matched.length,
  };
}

export function find(ref: string): ContentProject {
  const found = catalogue.find(
    (entry) => entry.id === ref || entry.slug === ref,
  );
  if (!found) fail('not_found', `no such project: ${ref}`);
  return found;
}

const VERSION_NUMBERS = ['1.4.0', '1.3.2', '1.2.9'];

export function versionsOf(ref: string): ContentVersion[] {
  const found = find(ref);
  return VERSION_NUMBERS.map((number, index) => ({
    source: found.source,
    id: `${found.id}-v${index}`,
    projectId: found.id,
    name: `${found.title} ${number}`,
    versionNumber: number,
    channel: index === 0 ? 'release' : 'beta',
    gameVersions: ['1.21.4', '1.21.1'],
    loaders: ['fabric', 'neoforge'],
    featured: index === 0,
    datePublished: new Date((now() - index * 86_400 * 30) * 1000).toISOString(),
    downloads: 900_000 - index * 120_000,
    files: [
      {
        artifact: {
          url: `https://cdn.modrinth.com/data/${found.id}/versions/${number}/${found.slug}-${number}.jar`,
          filename: `${found.slug}-${number}.jar`,
          size: 412_000,
          checksum: { algorithm: 'sha1', hex: '0'.repeat(40) },
        },
        primary: true,
      },
    ],
    dependencies: [],
  }));
}

/** The project (and pinned version) a source page URL names. */
export function resolveUrl(url: string): ResolvedUrl {
  const [, slug = '', version = ''] =
    /\/(?:mod|modpack|resourcepack|shader|datapack|plugin)\/([^/?#]+)(?:\/version\/([^/?#]+))?/.exec(
      url,
    ) ?? [];
  const found = catalogue.find((entry) => entry.slug === slug) ?? catalogue[0];
  return { project: found, versionId: version };
}

export function resolveModpack(versionId: string): ResolvedModpack {
  const pack =
    catalogue.find((entry) => entry.kind === 'modpack') ?? catalogue[0];
  return {
    source: pack.source,
    projectId: pack.id,
    versionId: versionId || `${pack.id}-v0`,
    versionNumber: '1.4.0',
    name: pack.title,
    summary: pack.description,
    gameVersion: '1.21.1',
    loader: 'fabric',
    loaderVersion: '0.16.5',
    files: [],
  };
}

function installed(
  entry: ContentProject,
  kind: ContentKind,
  origin = '',
): InstalledContent {
  const version = versionsOf(entry.id)[0];
  return {
    kind,
    source: entry.source,
    projectId: entry.id,
    slug: entry.slug,
    title: entry.title,
    versionId: version.id,
    versionNumber: version.versionNumber,
    filename: version.files[0].artifact.filename,
    sha1: '0'.repeat(40),
    url: version.files[0].artifact.url,
    iconUrl: entry.iconUrl,
    installedUnix: ago(86_400 * 2),
    worlds: [],
    origin,
    enabled: true,
    disabledWorlds: [],
  };
}

const pools = new Map<string, InstalledContent[]>([
  [
    'fabric-playground',
    [
      installed(find('sodium'), 'mod'),
      installed(find('fabric-api'), 'mod'),
      installed(find('iris'), 'mod'),
      installed(find('complementary-reimagined'), 'shader'),
      installed(find('faithful-32x'), 'resource_pack'),
    ],
  ],
  ['smp', [installed(find('terralith'), 'data_pack')]],
  ['creative', [installed(find('essentialsx'), 'plugin')]],
]);

const untracked = new Map<string, UntrackedFile[]>([
  [
    'fabric-playground',
    [
      {
        name: 'hand-dropped.jar',
        path: '/mock/.hestia/instances/fabric-playground/data/mods/hand-dropped.jar',
      },
    ],
  ],
]);

const pool = (id: string): InstalledContent[] => {
  const existing = pools.get(id);
  if (existing) return existing;
  const fresh: InstalledContent[] = [];
  pools.set(id, fresh);
  return fresh;
};

export function listPool(
  id: string,
  kind: ContentKind,
): { items: InstalledContent[]; untracked: UntrackedFile[] } {
  return {
    items: pool(id).filter((item) => item.kind === kind),
    untracked: kind === 'mod' ? (untracked.get(id) ?? []) : [],
  };
}

/** Install one item per requested reference, ignoring ones already in the pool. */
export function install(
  id: string,
  kind: ContentKind,
  refs: string[],
  origin = '',
): InstalledContent[] {
  const items = pool(id);
  const added: InstalledContent[] = [];
  for (const ref of refs) {
    const entry = catalogue.find(
      (candidate) => candidate.id === ref || candidate.slug === ref,
    );
    if (!entry || items.some((item) => item.projectId === entry.id)) continue;
    const item = { ...installed(entry, kind, origin), installedUnix: now() };
    items.push(item);
    added.push(item);
  }
  return added;
}

/** Install a local file — no project, so it can never be updated. */
export function installFile(
  id: string,
  kind: ContentKind,
  path: string,
): InstalledContent {
  const filename = path.split(/[/\\]/).pop() ?? 'content.jar';
  const item: InstalledContent = {
    kind,
    source: 'file',
    projectId: '',
    slug: '',
    title: filename.replace(/\.(jar|zip)$/i, ''),
    versionId: '',
    versionNumber: '',
    filename,
    sha1: '0'.repeat(40),
    url: '',
    iconUrl: '',
    installedUnix: now(),
    worlds: [],
    origin: '',
    enabled: true,
    disabledWorlds: [],
  };
  pool(id).push(item);
  return item;
}

const isItem = (item: InstalledContent, ref: string): boolean =>
  item.filename === ref ||
  item.projectId === ref ||
  item.slug === ref ||
  item.title === ref;

export function remove(id: string, kind: ContentKind, refs: string[]): void {
  const items = pool(id);
  for (const ref of refs) {
    const at = items.findIndex(
      (item) => item.kind === kind && isItem(item, ref),
    );
    if (at >= 0) items.splice(at, 1);
  }
}

export function setEnabled(
  id: string,
  kind: ContentKind,
  ref: string,
  enabled: boolean,
): void {
  const item = pool(id).find(
    (candidate) => candidate.kind === kind && isItem(candidate, ref),
  );
  if (item) item.enabled = enabled;
}

/** Re-pin an item; an unknown version number is taken at face value. */
export function setVersion(
  id: string,
  kind: ContentKind,
  ref: string,
  version: string,
): InstalledContent[] {
  const item = pool(id).find(
    (candidate) => candidate.kind === kind && isItem(candidate, ref),
  );
  if (!item) return [];
  const match = versionsOf(item.projectId).find(
    (candidate) =>
      candidate.id === version || candidate.versionNumber === version,
  );
  item.versionId = match?.id ?? version;
  item.versionNumber = match?.versionNumber ?? version;
  return [item];
}

/** Point a local item at a newer file, keeping how the entry holds it. */
export function updateFromFile(
  id: string,
  kind: ContentKind,
  ref: string,
  path: string,
): InstalledContent[] {
  const item = pool(id).find(
    (candidate) =>
      candidate.kind === kind &&
      candidate.source === 'file' &&
      isItem(candidate, ref),
  );
  if (!item) return [];
  item.filename = path.split('/').pop() ?? path;
  item.title = item.filename;
  return [item];
}

/** Move every platform-sourced item of the kind (or the named ones) to its newest pin. */
export function update(
  id: string,
  kind: ContentKind,
  refs: string[],
): InstalledContent[] {
  const items = pool(id).filter(
    (item) =>
      item.kind === kind &&
      item.source !== 'file' &&
      (refs.length === 0 || refs.some((ref) => isItem(item, ref))),
  );
  for (const item of items) {
    const latest = versionsOf(item.projectId)[0];
    item.versionId = latest.id;
    item.versionNumber = latest.versionNumber;
  }
  return items;
}

/** The pool's update status — the second item of each kind is behind. */
export function updates(id: string, kind: ContentKind): ContentUpdate[] {
  return pool(id)
    .filter((item) => item.kind === kind && item.source !== 'file')
    .map((item, index) => {
      const latest = versionsOf(item.projectId)[0];
      return {
        filename: item.filename,
        projectId: item.projectId,
        currentVersionId: item.versionId,
        currentVersionNumber: item.versionNumber,
        latestVersionId: latest.id,
        latestVersionNumber: latest.versionNumber,
        updatable: index % 2 === 1,
      };
    });
}

/** Drop everything a pack installed, for `*.modpack.remove`. */
export function removeByOrigin(id: string, origin: string): number {
  const items = pool(id);
  const kept = items.filter((item) => item.origin !== origin);
  const removed = items.length - kept.length;
  pools.set(id, kept);
  return removed;
}

/** Hand a pack's content to the user, for a `*.modpack.remove` that keeps it. */
export function releaseByOrigin(id: string, origin: string): number {
  const items = pool(id);
  const released = items.filter((item) => item.origin === origin).length;
  pools.set(
    id,
    items.map((item) =>
      item.origin === origin ? { ...item, origin: '' } : item,
    ),
  );
  return released;
}

export const poolOf = (id: string): InstalledContent[] => pool(id);
