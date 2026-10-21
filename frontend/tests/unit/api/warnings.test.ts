import { describe, expect, it } from 'vitest';
import type { WarningInfo } from '@/api';
import { warningMessage } from '@/api';

const info = (value: Record<string, unknown>) =>
  value as unknown as WarningInfo;

describe('warningMessage', () => {
  it('interpolates the variant’s fields into the headline', () => {
    expect(
      warningMessage(info({ kind: 'export_files_embedded', count: 3 })),
    ).toBe('3 file(s) had no download to reference and were embedded in the archive.');
  });

  it('labels a token field through the warning vocabulary', () => {
    const rendered = warningMessage(
      info({
        kind: 'sync_unit_skipped',
        unit: 'options',
        detail: 'permission denied',
      }),
    );
    expect(rendered).toContain('the game settings');
    expect(rendered).not.toContain('options');
  });
});
