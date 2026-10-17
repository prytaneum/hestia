/**
 * `screenshot.*` — read where the game wrote them, so the mock keeps a few
 * rows and deletes from that list.
 */
import type { Screenshot } from '@/api/types';

import * as entries from '../state/entries';
import { type ChannelHandlers, str } from '../support';

let shots: Screenshot[] = entries
  .listInstances()
  .flatMap((instance, index: number) =>
    ['2026-09-01_18.42.10.png', '2026-08-30_21.05.55.png'].map(
      (file, shot) => ({
        instance: instance.id,
        instanceName: instance.name,
        file,
        path: `/home/you/.hestia/instances/${instance.id}/data/screenshots/${file}`,
        takenUnix: 1_757_000_000 - index * 86_400 - shot * 3_600,
        bytes: 1_482_000 + shot * 90_000,
      }),
    ),
  );

export const channels: ChannelHandlers = {
  'screenshot.list': (payload) => {
    const instance = str(payload, 'instance');
    return {
      screenshots: instance
        ? shots.filter(
            (shot) =>
              shot.instance === instance || shot.instanceName === instance,
          )
        : shots,
    };
  },
  'screenshot.delete': (payload) => {
    const instance = str(payload, 'instance');
    const file = str(payload, 'file');
    shots = shots.filter(
      (shot) => !(shot.instance === instance && shot.file === file),
    );
    return { screenshots: shots };
  },
};
