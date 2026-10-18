import type { ReactNode } from 'react';
import type {
  ContentKind,
  ContentVersion,
  InstalledContent,
  UntrackedFile,
} from '@/api';
import type { Job } from '@/queries';

/** The entry a content tab acts on. */
export interface EntryTarget {
  kind: 'server' | 'instance';
  id: string;
  flavor: string;
  gameVersion: string;
}

/**
 * The job kinds that hold the daemon's per-entry content lock — one content
 * change per entry at a time, so anything started beside one of these is
 * refused as busy rather than queued.
 */
const CONTENT_JOBS = ['content.add', 'content.update'];

/** Whether one of an entry's jobs is a content change still running. */
export const contentBusy = (jobs: Job[]): boolean =>
  jobs.some(
    (job) => job.status === 'running' && CONTENT_JOBS.includes(job.kind),
  );

/** How the daemon matches an item: its project id, else its filename. */
export const installedRef = (i: InstalledContent) => i.projectId || i.filename;

/** The installed pool narrowed by the kind filter and the quick search. */
export const filterContent = (
  items: InstalledContent[],
  kind: ContentKind | undefined,
  search: string,
): InstalledContent[] => {
  const q = search.trim().toLowerCase();
  return items.filter(
    (i) =>
      (!kind || i.kind === kind) &&
      (!q ||
        i.title.toLowerCase().includes(q) ||
        i.filename.toLowerCase().includes(q)),
  );
};

/** The untracked files in view, narrowed by the quick search. */
export const filterUntracked = (
  files: UntrackedFile[],
  search: string,
): UntrackedFile[] => {
  const q = search.trim().toLowerCase();
  return files.filter((file) => !q || file.name.toLowerCase().includes(q));
};

/** A stable identity for one installed row; the index keys an item by filename. */
export const rowKey = (i: InstalledContent) => `${i.kind}:${i.filename}`;

/**
 * Who put an item in the pool. The index tags provenance as `<scope>:<key>`,
 * where the key is an identity and not a label — a modpack's project id — so a
 * reader resolves it before showing it.
 */
export interface ContentOrigin {
  scope: 'modpack';
  key: string;
}

export const parseOrigin = (origin: string): ContentOrigin | null => {
  const sep = origin.indexOf(':');
  return origin.slice(0, sep) === 'modpack'
    ? { scope: 'modpack', key: origin.slice(sep + 1) }
    : null;
};

/**
 * The index stores a world data-relative (`saves/My World`) while the wire's
 * `worlds` scope names it bare — one conversion, here.
 */
export const worldName = (world: string) => world.split('/').pop() ?? world;

/**
 * The worlds a datapack loads in: its own selection, or — when it has none —
 * every world the entry has, now or later. Mirrors `install::target_worlds`.
 */
export const packWorlds = (
  item: InstalledContent,
  entryWorlds: string[],
): string[] =>
  item.worlds.length > 0 ? item.worlds.map(worldName) : entryWorlds;

/** Whether a pack is loaded in one world: the per-world twin of `enabled`. */
export const worldEnabled = (item: InstalledContent, world: string): boolean =>
  item.enabled && !item.disabledWorlds.map(worldName).includes(world);

/** The loader filter a kind's version lookup needs, given the entry's flavor. */
export const kindLoader = (
  kind: ContentKind,
  flavor: string,
): string | undefined =>
  kind === 'mod' || kind === 'plugin'
    ? flavor
    : kind === 'data_pack'
      ? 'datapack'
      : undefined;

/** One call's worth of work: the wire takes a batch of items of one kind. */
export interface ContentBatch {
  kind: ContentKind;
  items: string[];
}

/** Split rows into one batch per kind, in the order the kinds first appear. */
export const contentBatches = (rows: InstalledContent[]): ContentBatch[] => {
  const batches: ContentBatch[] = [];
  for (const row of rows) {
    const batch = batches.find((candidate) => candidate.kind === row.kind);
    if (batch) batch.items.push(installedRef(row));
    else batches.push({ kind: row.kind, items: [installedRef(row)] });
  }
  return batches;
};

export interface ContentHandlers {
  /** `worlds` narrows a datapack to those saves; omitted covers every one. */
  onEnable: (
    item: InstalledContent,
    enabled: boolean,
    worlds?: string[],
  ) => void;
  onRemove: (item: InstalledContent, worlds?: string[]) => void;
  onUpdate: (item: InstalledContent) => void;
  /** Picks a file and updates a local item from it. */
  onUpdateFromFile: (item: InstalledContent) => void;
  onSetVersion: (item: InstalledContent, version: ContentVersion) => void;
  /** The batch verbs, run one kind at a time. */
  onRemoveMany: (rows: InstalledContent[]) => void;
  onUpdateMany: (rows: InstalledContent[]) => void;
}

export interface SectionProps {
  entry: EntryTarget;
  kinds: ContentKind[];
  kind?: ContentKind;
  onKindChange: (kind?: ContentKind) => void;
  action?: ReactNode;
}
