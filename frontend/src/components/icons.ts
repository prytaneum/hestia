import type { Icon } from '@phosphor-icons/react';
import {
  CubeIcon,
  DatabaseIcon,
  HardDrivesIcon,
  ImagesIcon,
  PackageIcon,
  PlugIcon,
  PuzzlePieceIcon,
  SparkleIcon,
} from '@phosphor-icons/react';

import type { ContentKind } from '@/api';
import { m } from '@/paraglide/messages.js';

/** Icon for a library entry kind — instances and hosted servers. */
export function entryIcon(kind: 'instance' | 'server'): Icon {
  return kind === 'server' ? HardDrivesIcon : CubeIcon;
}

/** Icon for a piece of content, so the type reads at a glance. */
export function contentIcon(kind: ContentKind): Icon {
  switch (kind) {
    case 'mod':
      return PuzzlePieceIcon;
    case 'resource_pack':
      return ImagesIcon;
    case 'shader':
      return SparkleIcon;
    case 'data_pack':
      return DatabaseIcon;
    case 'modpack':
      return PackageIcon;
    case 'plugin':
      return PlugIcon;
  }
}

const NAMED_ICONS: Record<string, Icon> = {
  instance: entryIcon('instance'),
  server: entryIcon('server'),
  mod: contentIcon('mod'),
  resource_pack: contentIcon('resource_pack'),
  shader: contentIcon('shader'),
  data_pack: contentIcon('data_pack'),
  modpack: contentIcon('modpack'),
  plugin: contentIcon('plugin'),
};

/** A name from the app's icon set, as a plugin page names its icon; unknown names fall back to the plugin glyph. */
export function namedIcon(name: string | undefined): Icon {
  return (name && NAMED_ICONS[name]) || PlugIcon;
}

export const contentKindLabel: Record<ContentKind, () => string> = {
  mod: m['domain.kind.mod'],
  resource_pack: m['domain.kind.resourcepack'],
  shader: m['domain.kind.shader'],
  data_pack: m['domain.kind.datapack'],
  modpack: m['domain.kind.modpack'],
  plugin: m['domain.kind.plugin'],
};
