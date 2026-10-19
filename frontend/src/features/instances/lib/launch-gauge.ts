import type { ProvisionPhase, ProvisionProgress } from '@/api';
import {
  isMeasurable,
  overallRatio,
} from '@/features/shared/entry/components/provision-progress';
import { RateMeter } from '@/lib/format';

const STABLE_WINDOW_MS = 5000;
const STABLE_SPAN_MS = 3000;
const STABLE_SPREAD = 0.5;

export interface LaunchReading {
  /** 0..1; `null` when nothing measures the work yet. */
  fill: number | null;
  /** Every byte is down; what remains is unpacking and spawning the game. */
  starting: boolean;
  phase: ProvisionPhase;
  /** Launch-wide bytes; `total` is 0 until the daemon has planned the downloads. */
  current: number;
  total: number;
  rate: number;
  /** Set only once the rate has held steady for a few seconds. */
  etaSeconds: number | null;
}

/**
 * Turns a launch's progress events into one forward-only readout. The fill
 * follows the launch-wide byte counter so it never resets between phases; a
 * phase's own ratio stands in only while that counter is absent.
 */
export class LaunchGauge {
  private peak = 0;
  private readonly meter = new RateMeter();
  private samples: { at: number; rate: number }[] = [];

  observe(
    p: ProvisionProgress,
    now: number = performance.now(),
  ): LaunchReading {
    const planned = p.overallTotal > 0;
    const measured = planned
      ? Math.min(1, p.overallCurrent / p.overallTotal)
      : isMeasurable(p)
        ? overallRatio(p)
        : null;
    this.peak = Math.max(this.peak, measured ?? 0);

    const rate = planned ? this.meter.observe(p.overallCurrent, now) : 0;
    return {
      fill: measured === null && this.peak === 0 ? null : this.peak,
      starting: planned && p.overallCurrent >= p.overallTotal,
      phase: p.phase,
      current: p.overallCurrent,
      total: p.overallTotal,
      rate,
      etaSeconds: planned ? this.eta(p, rate, now) : null,
    };
  }

  private eta(p: ProvisionProgress, rate: number, now: number): number | null {
    this.samples = this.samples.filter(
      (sample) => now - sample.at <= STABLE_WINDOW_MS,
    );
    if (rate <= 0) {
      this.samples = [];
      return null;
    }
    this.samples.push({ at: now, rate });
    const rates = this.samples.map((sample) => sample.rate);
    const mean = rates.reduce((sum, value) => sum + value, 0) / rates.length;
    const steady =
      (Math.max(...rates) - Math.min(...rates)) / mean <= STABLE_SPREAD;
    const span = now - this.samples[0].at;
    const remaining = p.overallTotal - p.overallCurrent;
    return steady && span >= STABLE_SPAN_MS && remaining > 0
      ? remaining / mean
      : null;
  }
}
