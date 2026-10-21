import { describe, expect, it } from 'vitest';
import type { ProvisionProgress } from '@/api';
import { overallRatio } from '@/features/shared/entry/components';

const progress = (over: Partial<ProvisionProgress> = {}): ProvisionProgress =>
  ({ phase: 'client', current: 0, total: 0, ...over }) as ProvisionProgress;

describe('overallRatio', () => {
  it('is the unit fraction for a single-unit phase', () => {
    expect(overallRatio(progress({ current: 50, total: 200 }))).toBe(0.25);
  });

  it('is zero when the extent is unknown', () => {
    expect(overallRatio(progress({ current: 100, total: 0 }))).toBe(0);
  });

  it('never exceeds one, even if the daemon overshoots', () => {
    expect(overallRatio(progress({ current: 300, total: 200 }))).toBe(1);
  });

  it('advances across a batch rather than resetting per unit', () => {
    const atUnitStart = overallRatio(
      progress({ item: 3, items: 4, current: 0, total: 100 }),
    );
    const midUnit = overallRatio(
      progress({ item: 3, items: 4, current: 50, total: 100 }),
    );
    expect(atUnitStart).toBe(0.5);
    expect(midUnit).toBe(0.625);
    expect(midUnit).toBeGreaterThan(atUnitStart);
  });

  it('advances on a cached unit that reports no bytes', () => {
    expect(
      overallRatio(progress({ item: 2, items: 4, current: 0, total: 0 })),
    ).toBe(0.25);
  });
});
