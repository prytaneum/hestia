import { describe, expect, it } from 'vitest';
import type { InstalledContent } from '@/api';
import { contentBatches, contentBusy } from '@/features/shared/entry/lib';
import type { Job } from '@/queries';

const item = (
  kind: InstalledContent['kind'],
  projectId: string,
  filename = `${projectId}.jar`,
): InstalledContent =>
  ({ kind, projectId, filename }) as InstalledContent;

const job = (channel: string, status: Job['status']): Job =>
  ({ channel, status }) as Job;

describe('contentBatches', () => {
  it('groups rows into one call per kind', () => {
    expect(
      contentBatches([
        item('mod', 'sodium'),
        item('shader', 'complementary'),
        item('mod', 'iris'),
      ]),
    ).toEqual([
      { kind: 'mod', items: ['sodium', 'iris'] },
      { kind: 'shader', items: ['complementary'] },
    ]);
  });

  it('falls back to the filename for an item with no project', () => {
    expect(contentBatches([item('mod', '', 'hand-dropped.jar')])).toEqual([
      { kind: 'mod', items: ['hand-dropped.jar'] },
    ]);
  });
});

describe('contentBusy', () => {
  it('reads a running content job as busy, whatever started it', () => {
    expect(contentBusy([job('content.update', 'running')])).toBe(true);
    expect(contentBusy([job('content.add', 'running')])).toBe(true);
  });

  it('ignores a settled one, and jobs that hold no content lock', () => {
    expect(contentBusy([job('content.update', 'done')])).toBe(false);
    expect(contentBusy([job('content.add', 'failed')])).toBe(false);
    expect(contentBusy([job('instance.launch', 'running')])).toBe(false);
    expect(contentBusy([])).toBe(false);
  });
});
