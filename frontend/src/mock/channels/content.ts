/**
 * `content.*` — discovery on a source platform. Installing into an entry is
 * `instance.content.*` / `server.content.*` (see ./entry); these are the
 * read-only browse channels plus the two that classify something the user
 * pasted or picked.
 */
import type { ContentKind, ContentSource } from '@/api/types';

import * as content from '../state/content';
import * as settings from '../state/settings';
import { type Handlers, num, str } from '../support';

const MODRINTH: ContentSource = {
  id: 'modrinth',
  name: 'Modrinth',
  kinds: ['mod', 'modpack', 'resource_pack', 'shader', 'data_pack', 'plugin'],
};

const CURSEFORGE: ContentSource = {
  id: 'curseforge',
  name: 'CurseForge',
  kinds: ['mod', 'modpack', 'resource_pack', 'shader', 'data_pack'],
};

const ARCHIVE = /\.(jar|zip)$/i;

export const channels: Handlers = {
  // CurseForge is offered only once its key resolves — the same gate the
  // daemon applies, so the settings page's "source needs a key" hint is live.
  'content.sources': () => ({
    sources: settings.get('content.curseforge-key')
      ? [MODRINTH, CURSEFORGE]
      : [MODRINTH],
  }),

  'content.search': (p) =>
    content.search(
      p.kind as ContentKind | undefined,
      str(p, 'query'),
      num(p, 'limit', 20),
      num(p, 'offset'),
    ),

  'content.project': (p) => content.find(str(p, 'project')),

  'content.versions': (p) => ({
    versions: content.versionsOf(str(p, 'project')),
  }),

  'content.resolve_url': (p) => content.resolveUrl(str(p, 'url')),

  'content.modpack.resolve': (p) => content.resolveModpack(str(p, 'versionId')),

  'content.inspect': (p) => {
    const path = str(p, 'path');
    const filename = path.split(/[/\\]/).pop() ?? '';
    const valid = ARCHIVE.test(filename);
    return {
      valid,
      kind: valid ? 'mod' : undefined,
      filename,
      reason: valid ? '' : 'not a jar or zip',
      matches: [],
    };
  },
};
