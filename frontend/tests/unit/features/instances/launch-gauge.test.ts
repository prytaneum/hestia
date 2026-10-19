/**
 * The launch readout's promises: the fill only moves forward across phases,
 * and a time estimate appears only after the rate has held steady.
 */
import { describe, expect, it } from 'vitest';
import type { ProvisionProgress } from '@/api';
import { LaunchGauge } from '@/features/instances/lib/launch-gauge';

const progress = (over: Partial<ProvisionProgress>): ProvisionProgress =>
  ({
    phase: 'assets',
    current: 0,
    total: 0,
    detail: '',
    item: 0,
    items: 0,
    overallCurrent: 0,
    overallTotal: 0,
    ...over,
  }) as ProvisionProgress;

describe('LaunchGauge', () => {
  it('has no fill until something measures the work', () => {
    const reading = new LaunchGauge().observe(progress({ phase: 'resolving' }));
    expect(reading.fill).toBeNull();
  });

  it('follows the launch-wide counter, not the phase', () => {
    const gauge = new LaunchGauge();
    const reading = gauge.observe(
      progress({ current: 10, total: 10, overallCurrent: 25, overallTotal: 100 }),
    );
    expect(reading.fill).toBe(0.25);
  });

  it('never moves back when a later phase starts over', () => {
    const gauge = new LaunchGauge();
    gauge.observe(progress({ phase: 'libraries', overallCurrent: 60, overallTotal: 100 }));
    const next = gauge.observe(
      progress({ phase: 'assets', current: 1, total: 100, overallCurrent: 61, overallTotal: 100 }),
    );
    expect(next.fill).toBe(0.61);
    const unplanned = gauge.observe(progress({ phase: 'extract', current: 1, total: 100 }));
    expect(unplanned.fill).toBe(0.61);
  });

  it('is starting once every planned byte is down', () => {
    const reading = new LaunchGauge().observe(
      progress({ overallCurrent: 100, overallTotal: 100 }),
    );
    expect(reading.starting).toBe(true);
  });

  it('offers a time estimate only after a steady rate', () => {
    const gauge = new LaunchGauge();
    const at = (seconds: number, current: number) =>
      gauge.observe(
        progress({ overallCurrent: current, overallTotal: 100_000_000 }),
        seconds * 1000,
      );
    at(0, 0);
    expect(at(1, 1_000_000).etaSeconds).toBeNull();
    for (let s = 2; s <= 4; s++) at(s, s * 1_000_000);
    expect(at(5, 5_000_000).etaSeconds).toBeCloseTo(95, 0);
  });
});
