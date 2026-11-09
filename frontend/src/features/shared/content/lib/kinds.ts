import type { ContentKind } from '@/api';
import { m } from '@/paraglide/messages.js';

export type KindSlug =
  | 'mods'
  | 'modpacks'
  | 'resourcepacks'
  | 'shaders'
  | 'datapacks'
  | 'plugins'
  | 'behaviorpacks';

export const kindInfo: Record<
  ContentKind,
  { slug: KindSlug; label: () => string }
> = {
  mod: { slug: 'mods', label: m['domain.kind.mods'] },
  modpack: { slug: 'modpacks', label: m['domain.kind.modpacks'] },
  resource_pack: {
    slug: 'resourcepacks',
    label: m['domain.kind.resourcepacks'],
  },
  shader: { slug: 'shaders', label: m['domain.kind.shaders'] },
  data_pack: { slug: 'datapacks', label: m['domain.kind.datapacks'] },
  plugin: { slug: 'plugins', label: m['domain.kind.plugins'] },
  behavior_pack: {
    slug: 'behaviorpacks',
    label: m['domain.kind.behaviorpacks'],
  },
};

/**
 * Every kind the wire defines. Only for validating a URL search param, where
 * the entry — and so the set it actually accepts — is not loaded yet; an entry
 * carries its own `accepts` and that is what a surface renders.
 */
export const isContentKind = (value: unknown): value is ContentKind =>
  typeof value === 'string' && value in kindInfo;

export const contentKinds = Object.keys(kindInfo) as ContentKind[];

export const kindBySlug = (slug: string): ContentKind | undefined =>
  contentKinds.find((kind) => kindInfo[kind].slug === slug);

/**
 * The `?source=` param every browse route carries, so which platform is being
 * looked at survives navigation and a reload. Empty means the daemon's default
 * source; an id it does not serve falls back to that (`useSourceOptions`).
 */
export const sourceSearch = (
  search: Record<string, unknown>,
): { source?: string } =>
  typeof search.source === 'string' && search.source
    ? { source: search.source }
    : {};
