export const INTERVAL_PRESETS = ['6h', '12h', '1d'];

export type IntervalUnit = 'm' | 'h' | 'd';

export interface IntervalDraft {
  /** `off`, one of `INTERVAL_PRESETS`, or `custom`. */
  mode: string;
  count: string;
  unit: IntervalUnit;
}

export type IntervalIssue = 'whole_number' | 'too_short';

const UNIT_SECONDS: Record<IntervalUnit, number> = {
  m: 60,
  h: 3600,
  d: 86400,
};

// Mirrors the daemon's floor (`backup-interval must be at least 5m`).
const MIN_SECONDS = 300;

function split(value: string): Pick<IntervalDraft, 'count' | 'unit'> | null {
  const match = /^(\d+)([mhd])$/.exec(value);
  return match ? { count: match[1], unit: match[2] as IntervalUnit } : null;
}

/** The draft a stored `backup-interval` opens as; empty means off. */
export function intervalDraft(stored: string): IntervalDraft {
  const amount = split(stored) ?? { count: '1', unit: 'h' as const };
  if (stored === '') return { mode: 'off', ...amount };
  if (INTERVAL_PRESETS.includes(stored)) return { mode: stored, ...amount };
  return { mode: 'custom', ...amount };
}

/** What is wrong with a custom draft's amount, if anything. */
export function intervalIssue(draft: IntervalDraft): IntervalIssue | null {
  if (draft.mode !== 'custom') return null;
  if (!/^\d+$/.test(draft.count) || Number(draft.count) < 1) {
    return 'whole_number';
  }
  return Number(draft.count) * UNIT_SECONDS[draft.unit] < MIN_SECONDS
    ? 'too_short'
    : null;
}

/** The `backup-interval` value a draft writes; empty turns the schedule off. */
export function intervalValue(draft: IntervalDraft): string {
  if (draft.mode === 'off') return '';
  if (draft.mode === 'custom') return `${Number(draft.count)}${draft.unit}`;
  return draft.mode;
}
